# Graceful shutdown — Design

**Date:** 2026-04-27
**Branch:** `graceful-shutdown`

## Problem

When `sola` exits — whether via `Ctrl-C` on its TTY, the menubar's
"Quit Sola" action, or a crash — user apps that sola spawned through
`sola-session` keep running, orphaned and reparented to PID 1.
Concretely, observed today:

```
$ ps -eo pid,ppid,cmd | awk '$2 == 1' | grep wine
35042  1 C:\windows\system32\winedevice.exe
106960 1 C:\windows\system32\winedevice.exe
114399 1 C:\windows\system32\services.exe
114402 1 C:\windows\system32\winedevice.exe
114411 1 C:\windows\system32\plugplay.exe
114417 1 C:\windows\system32\svchost.exe -k LocalServiceNetworkRestricted
114433 1 C:\windows\system32\winedevice.exe
114448 1 C:\windows\system32\rpcss.exe
```

These are stranded Wine helpers from past Steam sessions. Wine and Steam
each call `setsid()` very early, putting the entire descendant tree
into a fresh process group with no controlling TTY. The current shutdown
mechanisms can't reach them:

1. **`PR_SET_PDEATHSIG` is per-process and reset across `fork()`.** It
   only fires for the *direct* child of the dying parent, not
   grandchildren. By the time Wine has forked twice, pdeathsig is gone.
2. **`Ctrl-C` SIGINT goes to the TTY's foreground process group.** Sola's
   direct managed children (`sola-bus`, `sola-river`, `sola-shell`,
   `sola-session`) inherit sola's group and receive the signal. But user
   apps spawned via `sola-session` call `setsid()` themselves and leave
   that group, so the SIGINT never reaches them.
3. **`Ctrl-C` does not run sola's `Topic::Shutdown` handler.** Sola dies
   abruptly via SIGINT without unwinding. Even when `Topic::Shutdown`
   does fire (via the menu), `sola-session` reacts with a bare
   `std::process::exit(0)` — it never tears down its children.

The result: some apps (audio servers, daemonized helpers, Steam under
NixOS's bwrap) outlive the desktop indefinitely, accumulating across
sessions until the user manually kills them.

## Goals

- **All user-app processes die when sola dies, including grandchildren
  that have re-`setsid`'d.** Kernel-enforced, not signal-based.
- **`Ctrl-C` triggers a graceful shutdown** equivalent to the menu
  "Quit Sola" action — apps get `SIGTERM` and a window to save state
  before `SIGKILL`.
- **Unclean sola crashes leave a trivially-cleanable trail** — single
  command to reap stragglers.

## Non-goals

- Crash-proof tracking (if sola SIGKILLs, leaving stragglers is OK as
  long as recovery is one command).
- Restart-state persistence — that's separate work tied to whatever
  replaces `session.json` later.
- Replacing the existing managed-process lifecycle for sola's own
  children (`sola-bus`, `sola-river`, `sola-shell`, `sola-session`).
  Those work today; this is only about *user* apps.
- Cgroup delegation outside of what `systemd --user` already provides.

## Approach

Use `systemd-run --user --scope` to launch each user app inside its own
transient scope unit. A scope is a cgroup managed by the user systemd
manager; processes inside cannot escape the cgroup via `setsid`,
double-fork, or any other userspace trick. Stopping the scope sends
`SIGTERM` to the entire cgroup, then `SIGKILL` after a configurable
timeout — the kernel-level analogue of what `pdeathsig` was supposed to
do, but applied to the whole tree.

Hook `Ctrl-C` (SIGINT) and SIGTERM in `sola` to emit `Topic::Shutdown`
on the bus, so the existing menu-driven shutdown path becomes the *only*
shutdown path. `sola-session` reacts to `Topic::Shutdown` by stopping
every scope it owns, *then* exiting.

This is the standard pattern on systemd hosts. On NixOS specifically,
user systemd is always running (`systemctl --user is-active
default.target` returns active), so there's no fallback path to design.

### Why scopes, not services

Both put processes in cgroups. Scopes are owned by the launching client
(sola-session), inherit its environment, and disappear when the
last process exits. Services are owned by systemd, follow a stricter
lifecycle, and would force us to model env/working-dir/dependencies
through unit properties. For "wrap a user-launched command in a cgroup"
the scope is exactly the right primitive.

### Why the user manager, not the system manager

User services see the user's `DBUS_SESSION_BUS_ADDRESS`,
`WAYLAND_DISPLAY`, `XDG_RUNTIME_DIR`, audio session, etc. without
extra work. They also can't accidentally affect anything outside the
user's session. There's no scenario where we'd want to run a desktop
app in the system manager.

## Architecture

### `sola-session`: launch and close via systemd-run

Each `LaunchApp` becomes:

```
systemd-run --user --scope \
  --quiet \
  --collect \
  --unit=sola-app-<app_id>-<launch_idx>.scope \
  --description="Sola app: <app_id>" \
  --property=TimeoutStopSec=5s \
  --property=KillSignal=SIGTERM \
  -- <user_command> [args...]
```

- `--scope` puts the process in its own cgroup.
- `--collect` lets the unit be garbage-collected once it exits, so we
  don't accumulate failed units.
- `--unit=...` gives us a deterministic handle to stop it. The naming
  is `sola-app-<app_id>-<idx>` where `<idx>` is a sola-session-local
  counter so multiple windows of the same app get distinct units.
  (`<launch_idx>` is monotonically incremented per launch, never reused.)
- `--description` shows up in `systemctl --user status` output for
  debugging.
- `TimeoutStopSec=5s` — after `systemctl stop`, systemd sends
  `SIGTERM`, waits 5s, then `SIGKILL`s the cgroup. Replaces the
  hand-rolled `GRACEFUL` (5s) + `FORCE_AFTER_TERM` (5s) state machine
  in `sola-session`.
- `--quiet` suppresses systemd-run's own progress chatter on stderr;
  failures still surface via the exit status.

`sola-session` keeps an in-memory map:

```rust
struct AppRecord {
    app_id: String,
    command: String,
    unit: String,        // e.g. "sola-app-steam-3.scope"
    pid: u32,            // PID of systemd-run, or the process inside;
                         // see "PID semantics" below
    launched_at: Instant,
    closing: bool,       // true once `systemctl stop` has been issued
}
```

The state-machine fields (`Closing { since }`, `Terminated { since }`,
`Killed`) and `run_close_timers()` are removed — systemd handles the
escalation now.

`CloseApp(app_id)` becomes:

```rust
for r in records_for(app_id) where !r.closing {
    r.closing = true;
    Command::new("systemctl")
        .args(["--user", "stop", "--no-block", &r.unit])
        .spawn()?;
}
```

`--no-block` returns immediately; we don't want to stall the
`sola-session` event loop on the 5s stop timeout. Reaping happens via
the existing `try_wait()` polling, which now watches the `systemd-run`
client process — when the scope's last member exits, `systemd-run`
returns and we emit `UserAppExited` as today.

### `sola-session`: shutdown response

```rust
Topic::Shutdown => {
    self.shutdown_all_apps();
    std::process::exit(0);
}
```

`shutdown_all_apps`:

1. Issue `systemctl --user stop --no-block <unit>` for every record.
2. Poll `try_wait()` on every `systemd-run` child for up to a hard
   ceiling of `TimeoutStopSec + 1s = 6s`.
3. Anything still alive after that gets `kill -9` on the `systemd-run`
   PID and the unit name explicitly stopped a second time. (Belt-and-
   braces: the cgroup will already be empty in practice because systemd
   has SIGKILLed it, but we don't want to wait forever.)

### `sola`: signal handlers

Install handlers for `SIGINT`, `SIGTERM`, and `SIGHUP` in `sola/src/main.rs`
on startup. Each handler atomically sets a `shutdown_requested: AtomicBool`.
The main loop checks it once per supervision tick.

When set, the main loop:

1. Best-effort emit `Topic::Shutdown` on the bus (so `sola-session`,
   `sola-shell`, etc. get the same notice the menu sends today).
2. Sleep briefly (200ms) so subscribers have a chance to react.
3. Run the existing `shutdown_all(&mut managed)` for sola's own
   children. (They'll mostly already be in graceful teardown by now.)
4. `river_sup.shutdown()`.
5. `std::process::exit(0)`.

Use `signal-hook` (mature, async-signal-safe, already idiomatic Rust).
Add to `sola`'s dependencies.

A second `Ctrl-C` while shutdown is in progress should not stack
another handler invocation. The atomic-bool gate handles that — second
press is a no-op.

### `sola-shell`: no functional changes

The "Quit Sola" menu item already emits `Topic::Shutdown`. After this
work, that path will additionally cause every user app to be cgroup-
killed via the new `sola-session` behavior. No changes needed in shell.

## PID semantics

`systemd-run --user --scope` is *itself* a short-lived process that
delegates to dbus, which asks user systemd to start the scope. The PID
returned by `Command::spawn()` is the `systemd-run` client process,
which sticks around for the lifetime of the scope (it inherits the
target command's stdio and proxies it). We use this PID for `try_wait()`
and exit-status reporting; from sola-session's perspective the
`systemd-run` invocation is the user app.

We do not need the PID of the actual user binary. `UserAppExited` only
needs to know "did app X exit, and with what status" — the systemd-run
client's exit status reflects the scope's exit status, which reflects
the leader process's exit status. Good enough.

## Failure modes

| Mode | Behavior |
|------|----------|
| `systemd-run` exits non-zero (couldn't talk to user systemd) | Emit `LaunchResult { ok: false }` with stderr; user sees a toast. App not launched. |
| `systemctl stop` fails | Log warn, scope continues; on next sola shutdown it'll be retried. |
| User-app process double-forks daemons | Already in the cgroup — they get killed regardless. ✓ |
| sola crashes via SIGKILL or panic without running shutdown | All scopes survive. User runs `systemctl --user stop 'sola-app-*'` to reap. Document this in the vault. |
| `Ctrl-C` pressed twice rapidly | First press sets atomic, second press is observed and ignored by the gate. The kernel still delivers SIGINT each time, but the handler dedupes. If the user really wants to abort the graceful path, a third SIGINT after a short delay can map to immediate SIGKILL of sola itself — out of scope here. |
| User systemd is not running (theoretical on non-systemd hosts) | We're NixOS-only and systemd is unconditional. Treat this as unsupported; no fallback. |

## Files affected

- `crates/sola-session/Cargo.toml` — no new deps; we shell out.
- `crates/sola-session/src/session.rs` — replace launch/close/state-
  machine internals; remove `GRACEFUL` and `FORCE_AFTER_TERM` constants
  and `CloseState`. Add scope-unit naming and `systemctl stop` paths.
- `crates/sola-session/tests/session_lifecycle.rs` — update existing
  test to match new behavior. (See "Testing".)
- `crates/sola/Cargo.toml` — add `signal-hook = "0.3"` (or whatever the
  current version is) to dependencies.
- `crates/sola/src/main.rs` — install signal handlers; consult the
  atomic-bool in the supervision loop; refactor the existing
  `Topic::Shutdown` arm into a shared `do_shutdown()` helper that the
  signal path also calls.
- `crates/sola-core/src/process.rs` — keep `set_pdeathsig_sigterm` for
  sola's own managed children; nothing changes there. The user-app
  hook (currently `set_pdeathsig_sigterm` again) is no longer used
  because we shell out via `systemd-run`.
- `docs/vault/Sola.md` — short note in the runtime-environment section
  about scope units and the `systemctl --user stop sola-app-*` recovery
  command.

No bus-protocol changes. No shell changes.

## Testing

Existing `session_lifecycle.rs` covers spawn/close/exit. Adapt to the
new path:

- Replace the test app (currently a sleep loop, presumably) with a
  simple shell script that double-forks a child sleep, so we can
  assert the grandchild dies when the parent unit is stopped.
- Verify that after `CloseApp`, `systemctl --user is-active <unit>`
  goes to `inactive` within ~6s (the 5s stop timeout + slack).
- Verify `UserAppExited` is emitted exactly once per launch, with a
  signal-style exit status (signal 15 = SIGTERM for graceful) when
  closed via systemctl.
- Verify a second `CloseApp` for an already-closing app is a no-op.

New manual test for the `Ctrl-C` path (not automatable in CI):

- Run sola from a TTY, launch a Wine app via the launcher, `Ctrl-C` the
  sola process group.
- After 6s, `ps -ef | grep wine` and `systemctl --user list-units
  'sola-app-*'` should both be empty.

## Open questions

1. **Shutdown timeout from sola main.** I picked 200ms for "let
   subscribers see Topic::Shutdown before we tear down infra." Long
   enough? Probably yes — sola-session and sola-shell pop messages every
   500ms tick max, but they should be much faster on a Shutdown burst.
   Confirm during implementation.
2. **Should `--collect` be on?** It garbage-collects the unit when the
   last process exits *and* it's inactive. With it on, `systemctl --user
   list-units 'sola-app-*'` won't show stale exited entries — generally
   what you want. Leaving it on; pull it back if it complicates
   debugging.
3. **Multi-window apps.** Today the shell can ask sola-session to launch
   the same `app_id` more than once. Each invocation gets its own scope
   (`sola-app-<id>-<n>`). `CloseApp(app_id)` stops *every* matching
   scope. That matches existing behavior. ✓

## Implementation order

1. Add `signal-hook` to sola's deps; install handlers; route to a
   shared `do_shutdown()` that wraps the existing teardown. Confirm
   `Ctrl-C` no longer leaves managed children running. (Doesn't yet
   help user apps — that's step 2.)
2. Rewrite `sola-session::Session::launch` to shell out via
   `systemd-run --user --scope`; track unit names; rewrite `close` to
   call `systemctl stop`. Drop the `CloseState` machine and timer.
3. Wire `sola-session`'s `Topic::Shutdown` handler to stop all scopes
   before exit.
4. Update `session_lifecycle.rs` test.
5. Manual end-to-end: launch Steam → game → `Ctrl-C` sola → verify no
   stragglers. Repeat for menu "Quit Sola".
6. Vault note.

Each step is independently mergeable.
