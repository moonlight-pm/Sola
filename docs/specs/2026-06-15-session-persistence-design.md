# Session Persistence — Design

**Date:** 2026-06-15
**Status:** Approved, implementing

## Goal

When Sola restarts, relaunch the user apps that were open, and restore each
app's on-screen position. "Position" includes non-Sola apps (e.g. Helium),
which today open in an arbitrary place because their layout is discarded each
session.

## Background — what already exists

Two facts make this small:

1. **Positioning is zone-based.** Sola's compositor has no free drag-to-pixel
   move. A window's position is the *zone* it's snapped to (Meta+Numpad →
   half / quarter / etc.); unzoned windows are auto-centered by `sola-river`.
   Zone assignments already persist per `app_id` via the `#[persistent]`
   `Topic::Zones`, and the shell already re-applies a saved zone when a
   window appears (`ZoningState::apply_config_zone`, called from
   `on_windows`). **Both paths are gated by `app_id.starts_with("sola-")`**,
   deliberately excluding external apps ("External apps are zoned manually
   each session").

2. **`sola-session` owns the launch set.** It tracks
   `children: HashMap<app_id, Vec<AppRecord{command, …}>>`, handling
   `LaunchApp` / `CloseApp` / `Shutdown`. It is the single source of truth
   for *what is running and how it was launched*.

`Topic::Windows` is `#[sticky]` (current window list replays to any new
subscriber); `Topic::Zones` is `#[persistent]` (survives restart, replays on
subscribe).

The two halves of this feature are independent: relaunch keys off the
*launch* `app_id` + command; zone memory keys off the *wayland* `app_id`
(unchanged). Neither depends on the other.

## Position memory

Remove the two `app_id.starts_with("sola-")` guards in
`crates/sola-shell/src/zoning.rs`:

- `handle_key` — persist the zone for **every** app that gets snapped (so the
  `Topic::Zones` map records external apps too).
- `apply_config_zone` — re-apply a saved zone to **every** matching window,
  not just `sola-*` ones.

The persistence (`Topic::Zones`, on disk) and the re-apply-on-window-appear
machinery already exist and are otherwise general. After this change, a
snapped Helium records its zone, and a relaunched Helium lands back in that
zone when its window appears. Unzoned apps still auto-center as before.

## Relaunch

### New persistent topic

In `crates/sola-bus/src/topics.rs`:

```rust
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionApp {
    pub app_id: String,
    pub command: String,
}
```

```rust
// Open user apps to restore on next start. sola-session owns the list and
// emits a fresh copy whenever its child set changes. Persistent so the set
// survives a full restart and replays on subscribe.
#[persistent]
SessionApps(Vec<SessionApp>),
```

Plain `#[persistent]`, so it lands in `state.yaml` like `Zones`.

### Capture (continuous, in `sola-session`)

`sola-session` emits a fresh `SessionApps` derived from `children` **whenever
the child set changes** — inside `launch`, `close`, and `reap_exited` (only
when something was actually added/removed). One `SessionApp` per `app_id`
(most-recent command); multiple live instances of the same app collapse to a
single restore entry. (Restoring N instances of one app is out of scope —
see Limitations.)

Crucially, `SessionApps` is **never emitted standalone on startup**, and the
restore step itself does not emit. The only emitters are real launch/close/
reap events. This is what keeps a stale-empty `children` from clobbering the
persisted set (see Restore + Limitations).

### Restore (one-shot, deduped, in `sola-session`)

On startup, after connecting and subscribing (now also to `Windows` and
`SessionApps`), `sola-session` runs a one-shot `restore_session`:

1. Drain incoming messages for a short settle (~750 ms — also gives the
   compositor time to come up), recording:
   - `persisted: Vec<SessionApp>` — the last `SessionApps` replay.
   - `running: HashSet<String>` — `app_id`s from the last `Windows` replay.
   - Any `LaunchApp` / `CloseApp` that arrive are handled normally (not
     dropped).
2. For each `persisted` app whose `app_id` is **not** in `running` and **not**
   already in `children`, call `launch(LaunchAppPayload { app_id, command })`.

The "minus what's already running" dedup makes restore self-correcting:

- **Fresh boot** → nothing running → relaunch all. Each `launch` emits an
  updated `SessionApps`, rebuilding the persisted set correctly.
- **Bare `sola-session` restart mid-session** → every app already running
  (per the sticky `Windows` replay) → relaunch nothing → no launch event →
  `SessionApps` is *not* re-emitted → the persisted set is preserved.

## Limitations (documented, out of scope)

- **Bare `sola-session` crash-restart loses capture accuracy.** Its in-memory
  `children` is empty after restart (the apps keep running in their systemd
  scopes, which `sola-session` no longer tracks). Restore won't duplicate them
  (dedup via `Windows`), and the persisted set is preserved as long as no
  launch/close/reap event fires; but a subsequent reap/close would emit a set
  missing the forgotten apps. This matches `sola-session`'s existing
  restart-amnesia. Rebuilding tracking from live scopes is a separate effort.
- **Multi-instance apps restore as one.** Two terminals → one terminal on
  restore. Avoids double-launching single-instance apps (browsers) and zone
  conflicts; faithful multi-instance restore is a later refinement.
- **Only apps launched via `Topic::LaunchApp` are captured** (i.e. anything
  `sola-session` spawned). Processes started outside that path aren't tracked.

## Testing

- **Bus:** round-trip `SessionApps` through `to_message`/`parse` and through
  `to_yaml_value`/`from_yaml_section`; assert `behavior() == Persistent`.
- **sola-session (pure helpers):**
  - `session_apps_from_pairs` — collapses `(app_id, command)` pairs (fed from
    `children`) to one sorted entry per `app_id`.
  - `restore_plan(persisted, running, children_keys)` — returns exactly the
    apps to launch (not running, not already a child); empty when all running,
    all of them on a cold boot.
- **zoning:** update the existing tests so an external `app_id` now persists
  its zone (dirty flag set) and `apply_config_zone` returns a frame for it.
