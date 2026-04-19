# sola-session — Design

**Date:** 2026-04-17
**Branch:** `sola-shell`

## Problem

The launcher silently fails to launch apps whenever the bus is under load.
Root cause: `sola-bus` drops `LaunchApp` broadcasts because `sola`'s
per-client queue is full.

Two underlying defects drive the queue to stay full:

1. **No subscription model.** Every client receives every topic. `sola`
   receives `Frame`, `Composition`, `Focus`, `Mouse*`, `Chord*` — high-rate
   topics it discards — yet the queue fills with them.
2. **`BusClient` notify-pipe deadlock.** The reader thread writes a
   wakeup byte per message into a pipe the caller never drains (`sola`
   uses `recv_timeout` rather than the `notify_fd`). After ~65k messages
   the pipe is full, `notify.write_all` blocks, the reader thread halts,
   the kernel send buffer on the bus side fills, the bus writer thread
   blocks, and the 256-slot queue stays full. Every subsequent broadcast
   drops with `queue full`.

Separately, `sola` today violates its stated role of "pure process
manager" by spawning and reaping user apps. This work moves that
responsibility out.

## Scope

In scope:

- Fix `sola-bus` protocol: add topic subscriptions.
- Fix `BusClient` notify-pipe deadlock.
- Introduce a handler-registration API in `sola-app` that builds the
  subscription list as a side effect.
- New crate `sola-session` that owns user-app lifecycle: spawn, close
  (graceful → force escalation), reap.
- Remove user-app launching from `sola`.
- Meta+Q closes the focused app via new `CloseApp` topic.
- Shell persists and restores the running-app set via `session.json`.

Out of scope:

- XDG `.desktop` file support.
- Fine-grained multi-window restore.
- Escalation for apps sola-session didn't spawn (compositor's graceful
  close still works).
- Auto-start entries beyond whatever was running last session.

## Architecture

After this change the process tree is:

```
sola  (boot daemon)
├── sola-bus         infrastructure
├── sola-river       compositor
├── sola-shell       UI authority + session state owner
└── sola-session     user-app lifecycle
         ├── sola-terminal   (restored via session.json)
         ├── sola-browser
         └── external apps (brave, obsidian, …)
```

`sola`'s MANAGED list becomes exactly `[sola-bus, sola-river, sola-shell,
sola-session]`. Terminal is no longer managed by `sola`; it is a
regular user app restored from `session.json`.

### Data-flow highlights

**Launch:**

```
shell  —LaunchApp{app_id, command}—>  bus  —>  sola-session
                                                      |
                                                      V
                                                  fork/exec
```

**Close (Meta+Q on focused window):**

```
shell  —CloseApp(app_id)—>  bus  —fan-out—>  sola-river  (xdg_toplevel.close
                                              for external apps)
                                              sola-session (5s SIGTERM, +5s SIGKILL)
                                              sola apps   (self-exit via on_close_app)
```

**Restore (shell startup):**

```
shell loads session.json  ->  waits for ClientConnected("sola-session")
                              -> emits LaunchApp per entry
                              -> applies saved zone as each window maps
```

## Bus changes

### Topic subscriptions

- New `TopicKind` enum in `sola-bus/src/topics.rs`, one variant per
  `Topic` variant, payload-less. `Topic::kind(&self) -> TopicKind`.
- New control messages `Subscribe(Vec<TopicKind>)` and
  `Identify(String)` — bus-internal, distinct from `Topic`. Encoded
  either as reserved topic names (`$subscribe`, `$identify`) or as
  variants on a widened message type. Choice left to implementation;
  protocol consumers never see them.
- `BusState` gains `subscriptions: HashMap<ClientId, HashSet<TopicKind>>`.
- **No back-compat.** Every client subscribes explicitly on connect.
  Without a subscription a client receives nothing beyond the sticky
  replay it asked for.
- `broadcast` filters by subscription. Sticky replay filters too; when
  a client later adds a subscription for a kind it didn't have, the
  server replays the matching stickies.
- `TopicKind::ALL: &[TopicKind]` static slice, generated from the
  `Topic` enum (by macro or hand-maintained). `sola-monitor` calls
  `bus.subscribe(TopicKind::ALL)` — recompile adds new topics.

### Notify-pipe fix

In `BusClient::read_loop`, the call
`notify.write_all(&[1u8])` is replaced with a non-blocking write that
ignores `WouldBlock`. The notify pipe write end is set non-blocking at
creation. A byte already in the pipe is a sufficient wakeup; dropping
additional bytes costs nothing.

This prevents the reader thread from halting, which was the primary
driver of the queue-full condition. Subscriptions further reduce the
fill rate so drops become rare in practice.

### New topics

- `Topic::CloseApp(String)` — app_id (Wayland app_id, == `applications.json`
  entry).
- `Topic::ClientConnected(String)` / `Topic::ClientDisconnected(String)` —
  presence events, emitted by the bus. See "Client roster" below.

### Client roster

The bus tracks a roster of connected clients keyed by `ClientId` with
value `app_id`. A client joins the roster by sending an `Identify`
control message carrying its `app_id`. `BusClient::set_app_id` sends
`Identify` automatically — if the client is already connected, it is
sent immediately; otherwise it is queued and sent on connect. Clients
that never set an `app_id` do not appear in the roster.

- When a client's `app_id` is first observed, the bus broadcasts
  `ClientConnected(app_id)` to every subscriber of that topic.
- When a client disconnects, the bus broadcasts
  `ClientDisconnected(app_id)` to every subscriber of that topic.
- When a client subscribes to `ClientConnected`, the bus immediately
  enqueues one `ClientConnected(app_id)` for every currently-rostered
  client. This is the replay semantic that replaces stickies for
  presence — no stale-sticky cleanup is needed.

`LaunchApp` changes shape: `Topic::LaunchApp { app_id: String, command: String }`
so sola-session can track spawns by identifier instead of by command.

## `sola-app` handler registration

Replaces the single `on_bus_event` match pattern with per-topic
handler registration. The registry's keys are the subscription set.

### Trait change

```rust
fn register_bus(&mut self, bus: &mut BusRegistry<Self>, ctx: &mut AppCtx);
```

Handler signature: `fn(&mut Self, &Topic, &mut AppCtx)`. The full
`Topic` is passed; the handler destructures the variant it registered
for. Registering the same kind twice is a dev-build panic and a
release-build warn-and-skip.

`on_bus_event` is removed. `on_raw_bus_message` remains for apps that
need message metadata (e.g. monitor).

### Framework plumbing (`sola-app::run`)

1. Create empty registry.
2. Call `app.register_bus(&mut registry, &mut ctx)`.
3. Collect registry keys → `bus.subscribe(kinds)` on connect.
4. On every parsed `Topic` → `registry.dispatch(&topic, &mut app, &mut ctx)`.

### Default `CloseApp` handling for sola apps

Trait default:

```rust
fn on_close_app(&mut self, topic: &Topic, ctx: &mut AppCtx) {
    if let Topic::CloseApp(app_id) = topic {
        if app_id == Self::APP_ID {
            ctx.shutdown();
        }
    }
}
```

Every sola app registers `bus.on(TopicKind::CloseApp, Self::on_close_app)`.
Apps that need pre-shutdown logic (persist tabs, etc.) override
`on_shutdown`; they do not override `on_close_app`.

Sola apps **do not** react to `xdg_toplevel.close`. The sola-app
framework installs a `close_request` handler on every window that
returns `Propagation::Stop`, blocking the default close-then-quit
path.

## `sola-session` crate

New binary crate at `crates/sola-session/`.

### Responsibilities

- Subscribe to `LaunchApp`, `CloseApp`, `Shutdown`.
- On `LaunchApp { app_id, command }`: fork/exec. Set `WAYLAND_DISPLAY`
  and `DISPLAY` from the sockets `sola-river` published. Set
  `PR_SET_PDEATHSIG, SIGTERM` so orphans die with the daemon.
- Track spawns in `HashMap<String, Vec<ChildRecord>>` where
  `ChildRecord = { app_id, command, pid, child: Child, launched_at, state: CloseState }`.
- Reap via `child.try_wait()`; emit `UserAppExited { app_id, code, signal }`.
- On `CloseApp(app_id)`: start the close state machine for every live
  child under that app_id.

### Close state machine

Per `ChildRecord`:

```
state = Live
on CloseApp(app_id):
    state = Closing { since: now }
at T+5s and state == Closing and child alive:
    SIGTERM child; state = Terminated { since: now }
at T+10s and state == Terminated and child alive:
    SIGKILL child; state = Killed
on child.try_wait() == Some(status):
    emit UserAppExited; remove record
```

Duplicate `CloseApp` for an app_id already in `Closing`/`Terminated` is
a no-op.

### Bus subscriptions

```rust
bus.subscribe(&[
    TopicKind::LaunchApp,
    TopicKind::CloseApp,
    TopicKind::Shutdown,
]);
```

### Event loop

Single-threaded; watches the bus client's `notify_fd` and a timer
source (GLib `MainLoop`, `calloop`, or `poll` with computed deadlines
— choice left to implementation). No extra threads beyond the bus
client's read thread.

### Emits

- `LaunchResult { app_id, ok, error }`
- `UserAppExited { app_id, code, signal }`

Both already exist on the bus as concepts (shell consumes them today
from `sola`); the source just moves.

## `sola` crate changes

Remove:

- `launch_user_app`, `UserApp`, `user_apps: Vec<UserApp>`, reaping logic.
- `Topic::LaunchApp` handling in the main loop.
- Emission of `LaunchResult` and `UserAppExited`.
- User-app env resolution calls (WAYLAND/DISPLAY) — now sola-session's job.

Change:

- `MANAGED = ["sola-bus", "sola-river", "sola-shell", "sola-session"]`.
  Explicitly not `sola-terminal`.
- Bus subscription: `bus.subscribe(&[TopicKind::Shutdown])`.

Unchanged: river supervision, binary-change watcher and self-restart,
managed-process supervision loop (restart-on-crash, backoff).

## `sola-shell` changes

### Meta+Q binding

Add Meta+Q to the registered chord list (`keys.rs`). Handler:

- If an overlay is active (launcher/switcher/menu), ignore.
- Look up focused window's Wayland `app_id` via `known_windows` and
  `focused_window_id`.
- Emit `CloseApp(app_id)`.

Shell does not wait or escalate — that is sola-session's job.

### Session file

Path: `~/.local/state/sola/session.json`.

```json
{
  "entries": [
    { "app_id": "brave",         "zone": "Top" },
    { "app_id": "sola-terminal", "zone": "BottomLeft" }
  ]
}
```

One entry per open window. Zone uses the existing `Zone` enum.
Written atomically via temp-file + `rename`.

### In-memory state

```rust
struct SessionEntry {
    app_id: String,
    zone: Zone,
    window_id: Option<u32>, // runtime-only, not persisted
}

session_entries: Vec<SessionEntry>
```

Invariants:

- Entries with `window_id: None` are pending — apps launched or to-be-
  launched that have not yet mapped a window.
- Entries with `window_id: Some(wid)` are live — a mapped window.
- Persistence serializes only `{app_id, zone}` fields.

### Lifecycle rules

- **Startup:** load `session.json` into `session_entries` with
  `window_id: None` on each entry.
- **On `ClientConnected("sola-session")`:** for each entry, look up
  the command in `applications.json` and emit
  `LaunchApp { app_id, command }`. Entries whose app_id is not in
  `applications.json` are logged and pruned from `session_entries`.
- **On window map for `app_id`:** find the first entry with matching
  `app_id` and `window_id: None`; set its `window_id` and apply its
  `zone`. If none exists, push a new entry with the default zone.
- **On zone change for a live window:** update its entry's `zone`.
- **On window vanish (without a matching `UserAppExited`):** demote
  the entry to pending (`window_id = None`). This keeps the entry
  recoverable when `sola-session` has died and its PDEATHSIG cascade
  took the windows with it.
- **On `UserAppExited(app_id)`:** remove one matching entry (prefer
  the first live entry; fall back to the first pending). This is the
  authoritative "app is gone" signal.
- **Persist:** write after any mutation to `session_entries`.

Unclaimed pending entries survive across restarts: if an app crashes
before mapping, its entry is still in the serialized set and will be
retried next boot.

### Corrupt/missing session.json

- Missing → start with empty `session_entries`.
- Unparseable → log warn, back up to `session.json.bak-<timestamp>`,
  start empty.

## `sola-river` (compositor) changes

Subscribe to `CloseApp` (in addition to its existing subscriptions).

Handler: iterate mapped toplevels; for each whose Wayland `app_id`
matches, send `xdg_toplevel.close`. No filtering between sola and
external apps — sola apps ignore the close_request via their framework
handler.

## Startup ordering

The restore flow depends on sola-session being on the bus before shell
emits `LaunchApp` calls. This is handled by the new `ClientConnected`
topic and the bus's roster replay:

1. `sola` launches managed processes in parallel. Ordering within a
   batch is not guaranteed.
2. Each process connects to the bus and calls `set_app_id`, which
   sends the `Identify` control message; the bus adds the client to
   the roster.
3. Shell reads `session.json` during `ShellApp::new`.
4. Shell subscribes to `ClientConnected`; its handler for
   `ClientConnected("sola-session")` runs the restore loop.

If sola-session is already rostered when shell subscribes, the bus's
roster replay delivers `ClientConnected("sola-session")` immediately.
If shell is first, it waits passively until sola-session joins.

## applications.json invariant

The `app_id` field in each entry **must equal** the Wayland `app_id`
the program reports when it maps a window. Existing entries where this
is not true must be updated (e.g. `brave` → whatever Brave actually
reports; check via compositor logs).

This is the single identifier used by shell, sola-session, and the
compositor. No mapping layer is maintained.

## Error handling

- **Spawn failure (sola-session):** emit `LaunchResult { ok: false, error: "..." }`;
  shell may surface a toast.
- **CloseApp for an app_id with no live children:** no-op. Compositor
  still sends xdg_toplevel.close to matching windows; if there are no
  matching windows either, the user gets no feedback. Acceptable for
  MVP.
- **Shell session.json write failure:** log error, keep running.
  Corrupt file recovery on next boot handles the result.
- **sola-session crash:** `sola` restarts it. Its PDEATHSIG cascade
  SIGTERMs all user apps. Shell's `session.json` remains on disk;
  `ClientConnected("sola-session")` fires on reconnect; shell re-emits
  `LaunchApp` for every entry. Full session restored automatically.

## Test plan

- **Bus subscriptions:** client subscribed to `[A]` does not receive
  topic `B`. Client subscribed to `[A, B]` receives both. Sticky
  replay respects the filter.
- **Notify-pipe fix:** simulate a client that never calls
  `drain_notify`, push >65k messages; reader thread stays live and
  continues to drain the socket.
- **sola-session spawn:** emit `LaunchApp`, verify child process
  exists with the expected env; `LaunchResult { ok: true }` is
  emitted.
- **sola-session close (external):** emit `CloseApp`; child exits
  (from xdg_toplevel.close or SIGTERM at T+5s). `UserAppExited`
  emitted.
- **sola-session close (sola app):** same test against a sola app
  that overrides `on_shutdown` to save state; verify save ran and
  exit happened before T+5s SIGTERM path.
- **sola-session force-kill:** spawn a process that ignores SIGTERM;
  verify SIGKILL at T+10s and `UserAppExited { signal: 9 }`.
- **Shell Meta+Q:** with a focused external window, Meta+Q fires one
  `CloseApp(app_id)`; compositor logs the xdg_toplevel.close; window
  closes.
- **Session restore:** run three apps, kill sola-session, watch them
  come back with the same zones after `sola` restarts it.
- **ClientConnected replay:** restart sola-session mid-session;
  shell's `ClientConnected("sola-session")` handler fires on each
  reconnect and restore loop runs.

## Migration notes

- `applications.json` entries may need their `app_id` fields updated
  to match actual Wayland app_ids. Audit before cutover.
- Terminal is removed from `sola`'s MANAGED list; first boot after
  this change will have no terminal unless the previous session.json
  already includes it or the user launches it manually.
