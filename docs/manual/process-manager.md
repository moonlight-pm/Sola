# Process manager

`sola` is the program you start. It starts River and the session's managed
components, starts again any that exit, restarts any whose binary is
replaced, and stops everything when the session ends. It has no window, no
configuration file, and no call-plane methods. Its only bus traffic is the
session's `Shutdown` message. The source is
[crates/sola/src/main.rs](../../crates/sola/src/main.rs) and
[crates/sola/src/river.rs](../../crates/sola/src/river.rs).

## Starting a session

Log in on a text console as the user the session is for, and run:

```bash
/opt/sola/bin/sola
```

`sola` needs:

- `XDG_RUNTIME_DIR`, which a console login sets. Sola's own sockets fall
  back to `/tmp` without it; Wayland does not.
- `river` on `PATH`. The NixOS module installs it.
- The managed binaries in the same directory as `sola`. It starts them from
  the directory its own executable is in, not from `PATH`.
- Write access to `/opt/sola/log` (the NixOS module makes it mode 1777).
  Without it, every process logs to the console only.

There is one session per user. At start, `sola` sends SIGKILL to every
`river` executable the same user is running, and deletes every `wayland-*`
entry in `XDG_RUNTIME_DIR` except XWayland's `wayland-x*`. Starting `sola`
on a second console while a session runs ends the first session.

## Start-up sequence

1. Rotates `sola.log` (see [Logs](#logs)) and starts logging.
2. Sets the cursor variables (see [Environment](#environment)).
3. Registers SIGINT, SIGTERM, and SIGHUP as shutdown requests.
4. Starts watching its own directory (see [Binary watch](#binary-watch)).
5. Starts River (see [River](#river)) and waits for its socket. It looks
   for a `wayland-N` socket that accepts a connection, waiting 10 ms
   between tries at first and doubling up to 1 s, for at most 30 seconds.
   It writes the socket's name to `$XDG_RUNTIME_DIR/sola-wayland`. If River
   exits first, or 30 seconds pass, `sola` stops River and exits with
   status 1.
6. Polls `/tmp/.X11-unix/` every 100 ms, for up to 3 seconds, for an X
   display that accepts a connection, and writes its name (`:0`) to
   `$XDG_RUNTIME_DIR/sola-display`. Without XWayland the file is absent.
7. Starts the managed components in order. It does not wait for any of
   them to be ready, except for a 750 ms pause before `sola-kvm`, which
   gives `sola-river` time to bind River's layer-shell protocol.
8. Connects to the bus as app id `sola`, retrying once a second until it
   succeeds, and subscribes to `Shutdown`.
9. Supervises until the session ends.

## Managed components

| Order | Binary | Arguments | Role |
|---|---|---|---|
| 1 | `sola-bus` | none | The bus ([The bus](bus.md)) |
| 2 | `sola-call` | none | The call host ([The call plane](call.md)) |
| 3 | `sola-botsd` | none | Bots daemon: call owner `bots`, HTTP on `0.0.0.0:27419` ([solactl](solactl.md)) |
| 4 | `sola-river` | none | River's window manager; call owner `compositor` ([Compositor](compositor.md)) |
| 5 | `sola-shell` | none | Menubar, launcher, switcher, zones ([Shell](sola-shell.md)) |
| 6 | `sola-session` | none | Applications; call owner `session` ([Session manager](session.md)) |
| 7 | `sola-kvm` | `server --input evdev` | KVM server ([KVM](sola-kvm-operator.md)) |

The list is compiled into `sola`. A component cannot be added, removed, or
disabled without rebuilding it, so `sola-botsd` and `sola-kvm` run in every
session. With the NixOS firewall on, its default, port 27419 is reachable
from other machines only if you open it.

Each component is started with:

- `sola`'s environment, plus `SOLA_NO_SELF_WATCH=1`;
- `sola`'s standard output and error, so their console lines interleave on
  the console `sola` runs on;
- a session of its own (`setsid`), so Ctrl-C on the console reaches only
  `sola`;
- a parent-death signal (`PR_SET_PDEATHSIG`) of SIGTERM, so it is stopped
  if `sola` dies.

Each waits for its own dependencies:

| Binary | Waits for |
|---|---|
| `sola-bus`, `sola-call` | Nothing. Each deletes a leftover socket file and binds its socket. |
| `sola-botsd` | The call socket file, for up to 10 seconds; its call provider then retries every 400 ms. |
| `sola-river` | Reads `sola-wayland` once, and exits if it is missing. Retries the bus every 20 ms. |
| `sola-shell` | `sola-wayland` for up to 20 seconds and the socket file for 10 more; the bus, retrying every 250 ms. |
| `sola-session` | The bus, retrying every second. It then collects saved state for 750 ms before it relaunches applications. |
| `sola-kvm` | `sola-wayland` and the socket file, up to 30 seconds each. Reads `~/.config/sola-kvm/config.toml`, or uses defaults when it is missing. |

### River

`sola` runs River as:

```bash
river -log-level info -c :
```

- `river` is looked up on `PATH`.
- Its standard output and error are appended to `/opt/sola/log/river.log`.
- `WAYLAND_DISPLAY` and `DISPLAY` are removed from its environment, so River
  drives the screen and input devices directly instead of opening a window
  inside another display server.
- `-c :` runs the no-op command `:` as River's init, so River's own init
  script (`~/.config/river/init`) is not run.
- It gets its own session and SIGTERM as its parent-death signal, like the
  managed components. It is not watched for binary changes and is never
  restarted.

## Environment

`sola` itself sets two variables, which River, the managed components, and
the applications they start all inherit:

| Variable | Value |
|---|---|
| `XCURSOR_PATH` | `/opt/sola/share/cursors` put in front of the existing value; `/opt/sola/share/cursors:~/.icons:/usr/share/icons` when unset |
| `XCURSOR_THEME` | `McMojave`, only if unset |

It does not set `WAYLAND_DISPLAY` for its children. Each Wayland client reads
the socket name from `$XDG_RUNTIME_DIR/sola-wayland` and sets
`WAYLAND_DISPLAY` for itself. Applications built on sola-kit also set, each
only if unset, `__EGL_VENDOR_LIBRARY_DIRS`, `LIBVA_DRIVERS_PATH`, and
`VK_ICD_FILENAMES` to paths under `/run/opengl-driver/`, and
`GSETTINGS_BACKEND=memory`. `VK_ICD_FILENAMES` names the NVIDIA Vulkan
driver. On other hardware, set it before starting `sola` to the driver's
file in `/run/opengl-driver/share/vulkan/icd.d/`; every process inherits it,
and the kit leaves a set value alone.

`sola-session` builds the environment of the applications it launches:
`WAYLAND_DISPLAY`, `DISPLAY` (from `sola-display`, or a live probe),
`XDG_SESSION_TYPE=wayland`, and `XDG_RUNTIME_DIR`, without
`SOLA_NO_SELF_WATCH`. See [Session manager](session.md).

## Supervision and restarts

The supervisor wakes when a bus message arrives, or every 500 ms. On each
pass it:

1. Shuts down if a signal asked for it.
2. Reconnects to the bus if the connection dropped (at most once a second),
   and shuts down on a `Shutdown` message.
3. Checks River. If River has exited, it sends SIGKILL to every managed
   component and exits with status 1.
4. Handles binary changes (see [Binary watch](#binary-watch)).
5. Starts again every managed component that has exited, whatever its exit
   status.

A component that ran for less than 5 seconds is started after a 2-second
wait, logged as `crashed quickly, waiting before restart`. One that ran
longer is started at once, logged as `exited, restarting`. A binary that
could not be started at all, because it is missing or not executable, is
logged as `failed to launch` and tried again on the next pass.

There is no limit on restarts and the wait does not grow: a component that
fails at start is started again roughly every 2.5 seconds until it stops
failing. The 2-second wait blocks the supervisor, so signals, binary
changes, and other exits are handled after it. The restart log lines record
the uptime, not the exit status; the reason is in the component's own lines
in `sola.log`, including a panic message with a backtrace if it panicked.

Not restarted:

- River. Its exit ends the session.
- `sola`. Nothing supervises it.
- Applications. `sola-session` publishes their exit and leaves them closed.

`sola` has no call-plane methods, so `solactl` cannot list, stop, or
restart managed components. To restart one by hand, stop it and let `sola`
start it again:

```bash
pkill -x sola-shell
```

## Binary watch

`sola` watches the directory its executable is in (normally
`/opt/sola/bin`) for its own file and the managed components' files. A
file being created, modified, or removed starts a 500 ms settle period.
Every further event in that period is absorbed, including events for other
files, so of two binaries replaced within half a second only the first is
restarted. After the period:

- For a managed component, `sola` sends SIGKILL to the running process,
  waits for it, and starts the new binary at once, without the restart
  wait.
- For `sola` itself, it sends SIGKILL to every managed component and
  executes the new `sola` with the same arguments, in the same process. The
  new `sola` runs the whole start-up sequence: it kills the old River as a
  leftover and starts a fresh one. Applications lose the River they were
  connected to, and `sola-session` relaunches the saved open-application
  set. The session manager was killed rather than told to shut down, so the
  saved set is the one from before the restart.

Other binaries in the directory, such as applications and `solactl`, are not
watched by `sola`. An application built on sola-kit watches its own file and
executes the new one in place, keeping its process id and arguments. It
skips this when `SOLA_NO_SELF_WATCH` is set, which is how the managed
`sola-shell` avoids restarting twice.

`cargo make install` copies only binaries whose contents changed, so an
unchanged file restarts nothing. It replaces them in this order, waiting
1 second between replacements so each restart settles: `sola-bus`,
`sola-call`, `sola-botsd`, `sola-river`, `sola-shell`, `sola-session`,
`sola-kvm`, `sola`, then everything else. It asks for confirmation before
replacing `sola`, because that restarts the session.

When the release is installed by the NixOS module (`installRelease`, the
default), `/opt/sola/bin` is a link into the Nix store. `sola` watches, and
starts its components from, the store directory its executable resolves
to, which never changes. Switching the system to a new release restarts
nothing; quit the session and start it again.

## Ending the session

| Action | How the request reaches `sola` |
|---|---|
| **Quit Sola** in the flower menu | `sola-shell` publishes `Shutdown` on the bus |
| Ctrl-C on `sola`'s console | SIGINT |
| `pkill -x sola` from another console | SIGTERM |
| A hangup of `sola`'s terminal | SIGHUP |
| `solactl emit Shutdown null` | `Shutdown` on the bus |
| **Restart Computer** or **Shut Down** in the flower menu | logind reboots or powers off the machine; `sola` receives SIGTERM with the rest of the system |

All of these take the same path:

1. `sola` publishes `Shutdown` on the bus, if it is connected.
2. Subscribers react. `sola-session` asks systemd to stop every
   application's scope, which sends SIGTERM and, after 5 seconds, SIGKILL;
   without a systemd user manager it sends SIGTERM to each application
   directly. `sola-river` exits.
3. After 200 ms, `sola` sends SIGKILL to every managed component still
   running.
4. It sends SIGTERM to River, waits up to 2 seconds, then sends SIGKILL.
5. It exits with status 0.

The saved open-application set is not cleared, so the next session
relaunches the applications that were open.

`pkill sola` without `-x` also matches `sola-bus`, `sola-session`, and every
other process whose name contains `sola`. They die at once, before the
session manager can stop applications, and programs that are not Sola's
keep running.

Two other ways the session ends skip this path:

- River exits. `sola` sends SIGKILL to the managed components and exits
  with status 1. No `Shutdown` is published, so `sola-session` does not stop
  applications; they lose their display.
- `sola` is killed with SIGKILL or crashes. The kernel sends SIGTERM to
  River and to every managed component. Applications are not stopped by the
  session manager.

## Logs

| File | Written by | Rotation |
|---|---|---|
| `/opt/sola/log/sola.log` | Every Sola process, through `tracing`: plain text, UTC times | When `sola` starts and the file is 100,000 bytes or larger, it becomes `sola.log.1`, older files shift up, and `sola.log.10` is deleted. Not rotated during a session. |
| `/opt/sola/log/river.log` | River's standard output and error | None; appended to |
| `/opt/sola/log/app-<app id>.log` | Standard output and error of each application `sola-session` launched | Truncated at the next launch once it is larger than 8 MiB |

Every Sola process also writes coloured lines to its standard error. The
managed components share `sola`'s, so its console shows all of them; River
and the applications write to their own files instead.

A line in `sola.log`:

```text
2026-10-19T09:12:03.512204Z  INFO [sola]     sola::river    river socket is live path=/run/user/1000/wayland-1
```

The bracket is the process: the binary name without `sola-` (`[sola]`,
`[bus]`, `[call]`, `[botsd]`, `[shell]`, `[session]`, `[kvm]`), or an
application's app id without `sola-` (`[mail]`). `[river]` is `sola-river`;
River's own output is only in `river.log`. The next column is the module:
`sola::river` is the process manager's River supervisor, `sola::bus` the bus
library in any process.

Each process's default filter is its own crate and Sola's shared libraries
(`sola_core`, `sola_bus`, `sola_call`, `sola_kit`, `sola_browser`,
`sola_assets`) at `info`; other crates log nothing. `RUST_LOG` replaces that
filter, in `tracing` filter syntax, for `sola` and for every process it
starts, since they inherit it:

```bash
RUST_LOG=info,sola_river=debug /opt/sola/bin/sola       # one component in detail
RUST_LOG=info,sola_bus::traffic=trace /opt/sola/bin/sola  # every bus message
RUST_LOG=debug /opt/sola/bin/sola                        # everything; very large
```

`sola-botsd` sets its own filter only when `RUST_LOG` is unset.

`solactl logs <name>` prints `/opt/sola/log/<name>.log` (`sola.log` when no
name is given); `-f` keeps following it:

```bash
solactl logs -f
solactl logs river
solactl logs app-sola-mail -f
```

## Files and variables

| Path | Role |
|---|---|
| `/opt/sola/bin/` | `sola` and the managed binaries (the directory `sola` was started from) |
| `/opt/sola/log/` | Logs |
| `/opt/sola/share/cursors/` | The bundled cursor theme |
| `$XDG_RUNTIME_DIR/sola-wayland` | River's socket name; deleted and rewritten at each start |
| `$XDG_RUNTIME_DIR/sola-display` | XWayland's display; deleted and rewritten at each start |
| `~/.config/sola/state.yaml` | Holds `SessionApps`, the open-application set `sola-session` relaunches |
| `~/.config/sola-kvm/config.toml` | `sola-kvm` settings |

| Variable | Effect |
|---|---|
| `RUST_LOG` | Log filter for every process in the session |
| `XDG_RUNTIME_DIR` | Where sockets and name files live (`/tmp` when unset) |
| `XDG_CONFIG_HOME` | Where `sola/state.yaml` and `sola-kvm/config.toml` live (`~/.config` when unset) |
| `PATH` | Where `river` is found |
| `SOLA_BUS_PATH`, `SOLA_CALL_PATH` | Override the bus and call socket paths for every process that inherits them |
| `SOLA_NO_SELF_WATCH` | Set to `1` for managed components; stops sola-kit's own binary watch |
| `XCURSOR_PATH`, `XCURSOR_THEME` | See [Environment](#environment) |

## Troubleshooting

| Symptom | Cause |
|---|---|
| `failed to spawn river` with `river not found in PATH` | River is not installed or not on `PATH`. Enable the NixOS module and switch to the new configuration. |
| `river socket never appeared` | River exited before opening a socket, or opened none within 30 seconds. `river.log` says why; another session holding the display is a common cause. |
| `river exited; shutting down sola` | River stopped during the session. See `river.log`. |
| `crashed quickly, waiting before restart` for the same process every few seconds | That component fails at start. Search `sola.log` for its tag (`[shell]`, `[session]`, …) and `ERROR` or `panicked`. |
| `failed to launch` twice a second | The binary is missing from `sola`'s directory, or is not executable. |
| Repeated `bus reconnect failed` from `[sola]` | `sola-bus` is not running. Look for `[bus]` lines, such as `failed to bind bus socket`. |
| A new binary did not take effect | It is not a managed component and not a sola-kit application; its contents were unchanged; or `/opt/sola/bin` is a Nix store link (see [Binary watch](#binary-watch)). |
| The running session ended when `sola` was started on another console | One session per user; see [Starting a session](#starting-a-session). |
| Nothing in `sola.log`, only console output | `/opt/sola/log` is not writable by the session's user. |
