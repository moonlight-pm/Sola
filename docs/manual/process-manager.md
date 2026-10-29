# Process manager

On Oath, PID 1 is Sola's process manager. Each of Sola's long-running
processes is a service, a `svc:*` object in Oath's catalog. Oath's `desk`
profile defines them, and PID 1 (Oath's init) runs them as the desk user,
`home` (uid 1, home directory `/home`, runtime directory `/run/user/1`).
PID 1 starts them at boot, restarts them by their restart policy, restarts
those whose program moved when an apply changes a package, and stops them
when the machine shuts down. Applications are not services: the
[session manager](session.md) starts them.

```bash
oath ls --kind svc            # every service on the machine
oath get svc:sola-shell       # its spec, and actual: state, pid, restarts, last exit, package
oath logs svc:sola-shell      # its last 50 lines of output; -n 200 for more
```

Sola's own process manager, `sola`, does not run on Oath; see
[The sola program](#the-sola-program).

## The services

The desk profile's services for the session are below. A machine's
catalog can differ from the profile; `oath get` shows what it runs.

| Service | Runs | Wants | Restart |
|---|---|---|---|
| `svc:seatd` | `seatd -u home -g home`, as root: opens the GPU and input devices for the session | none | always |
| `svc:rivulet` | `rivulet -log-level info -c :`, the compositor | `svc:seatd` | always |
| `svc:sola-bus` | `sola-bus`, the [bus](bus.md) | none | always |
| `svc:sola-call` | `sola-call`, the [call host](call.md) | `svc:sola-bus` | always |
| `svc:sola-river` | `sola-river`, the window manager, a client of rivulet's River protocols ([Compositor](compositor.md)) | `svc:rivulet`, `svc:sola-bus`, `svc:sola-call` | always |
| `svc:sola-shell` | `sola-shell`: the top row, the bar, the launcher ([Shell](sola-shell.md)) | `svc:sola-river`, `svc:pipewire-pulse`, `svc:wireplumber` | on-failure |
| `svc:sola-tmux` | `tmux -L sola -f /home/.config/sola/tmux.conf -D`, the tmux server that holds Terminal's shells | none | always |
| `svc:sola-tmux-ws` | The same on socket `sola-ws`, for Projects' tabs | none | always |
| `svc:sola-session` | `sola-session`, the [session manager](session.md) | `svc:sola-bus`, `svc:sola-call`, `svc:sola-tmux`, `svc:sola-tmux-ws` | on-failure |
| `svc:sola-kvm` | `sola-kvm listen --bind 0.0.0.0:4242`, which takes another machine's keyboard and mouse ([KVM](sola-kvm-operator.md)) | `svc:sola-river` | always |

Each Sola service runs its program from `/bin`, where `pkg:sola` links
it. The two tmux servers are services of their own so that shells
outlive a restart of the session manager: the applications it starts
share its cgroup, and PID 1 stops a service's whole cgroup.

Beside these, the desk runs `svc:dbus-session`, `svc:pipewire`,
`svc:wireplumber`, and `svc:pipewire-pulse` as `home`, for sound and a
session D-Bus; Oath's own manual covers them.

The bots daemon, `sola-botsd`, is in `pkg:sola`, but no service runs it,
so Bots and `solactl bots` have nothing to talk to on Oath.

## Start-up

At boot, once it has brought up the network, PID 1 starts every enabled
service in `wants` order: a service starts after the enabled services it
wants. `wants` orders start-up only; PID 1 does not wait for a service to
be ready. The display drivers load after the services start; rivulet
waits for the GPU, and its `restart: always` covers one that takes
longer.

Each Sola process waits for what it needs:

| Process | Waits for |
|---|---|
| `sola-bus`, `sola-call` | Nothing. Each deletes a leftover socket file and binds its socket, `/run/user/1/sola-bus` or `/run/user/1/sola-call`. |
| `sola-river` | Rivulet's socket: it tries each `wayland-N` in `/run/user/1` every 500 ms for up to 15 seconds, then waits up to 5 seconds more for the socket file. Without one it exits with status 1, and PID 1 starts it again. Then the bus, retrying. |
| `sola-shell`, and every application | The same socket for up to 20 seconds, then up to 10 seconds for the socket file, then connects anyway. The bus, retrying every 250 ms. |
| `sola-session` | The bus, retrying every second. It then collects saved state for 750 ms before it reopens the applications that were open. |
| `sola-kvm` | Rivulet's socket, for up to 30 seconds. Reads `~/.config/sola-kvm/config.toml`, or uses defaults when it is missing. |

Nothing writes `$XDG_RUNTIME_DIR/sola-wayland` or `sola-display` on Oath;
those files are the `sola` program's. Sola's processes find rivulet's
socket by connecting to each `wayland-N` in name order, skipping
XWayland's `wayland-x*`, and take the first that answers. Applications find XWayland's display by probing
`/tmp/.X11-unix/`.

## Restarts

PID 1 restarts a service by its `restart`:

- `always`: after any exit.
- `on-failure`: after an exit with a non-zero status or by a signal. An
  exit with status 0 leaves the service `stopped`.

A run shorter than 10 seconds is a quick failure. Restarts after quick
failures wait 1, 2, 4, then 8 seconds; a run of 10 seconds or more
restarts at once. After five quick failures in a row the service is
`failed`, and it stays down until its spec or its program's package
changes. `oath get svc:<name> --actual` shows its `state` (`running`,
`restarting`, `stopped`, or `failed`), its `restarts` this boot, and its
`last_exit`.

To restart a service by hand, kill its process. An exit by a signal is a
failure, so PID 1 starts it again under either policy:

```bash
pkill -x sola-shell
```

When a service's main process exits, PID 1 also kills what is left in its
cgroup. Killing `sola-session` therefore ends the applications it
started, and the new session manager reopens them.

Not restarted by PID 1: applications. When one exits, the session
manager publishes its exit and leaves it closed. A stopped shell or
session manager comes back only when PID 1 next starts the services that
are not running: at boot, or when an apply that changes a service or a
package makes it converge them.

## Updates

Nothing watches Sola's binaries on Oath. PID 1 follows each service's
program through its `/bin` link into the store, and when an apply moves
that program's package to another hash, it restarts the service. An
apply of `pkg:sola` (`cargo make oath apply` does one) therefore restarts
`svc:sola-bus`, `svc:sola-call`, `svc:sola-river`, `svc:sola-shell`,
`svc:sola-session`, and `svc:sola-kvm`. It does not restart rivulet or the
two tmux servers, whose programs are in other packages.

Stopping `svc:sola-session` stops the applications in its cgroup with it.
The new session manager reopens the applications that were open, and
Terminal and Projects reattach to their tmux sessions, which kept running.
An application does not restart itself: its program is in the store
directory it started from, which never changes, so an application started
before an apply runs the old program until it is opened again or the
session manager restarts. `sudo oath undo` takes an apply back the same
way, restarting the same services on the previous programs.

## Environment

PID 1 gives every service `PATH=/bin`, `HOME`, `USER`, `LOGNAME`, `SHELL`,
and `XDG_RUNTIME_DIR` for its account, then `host:local`'s environment,
then `TZ`, then the service's own `env`, which wins on the same name. The
desk profile sets, in `host:local`, `LANG=en_US.UTF-8` (tmux refuses a
locale that is not UTF-8), `SSL_CERT_FILE`, `DBUS_SESSION_BUS_ADDRESS`, and
`PKG_CONFIG_PATH`. Each Sola service's own `env` has:

| Variable | Value |
|---|---|
| `XCURSOR_PATH` | `/oath/store/pkg/sola/share/cursors` |
| `XCURSOR_THEME` | `McMojave` |
| `FONTCONFIG_FILE` | `/oath/store/pkg/fontconfig/live/etc/fonts/fonts.conf` ([Fonts](distribution.md)) |
| `SHELL` | `/bin/zsh`, so terminals start the login shell |

Each Wayland client sets `WAYLAND_DISPLAY` for itself to the socket it
found, and `XDG_SESSION_TYPE=wayland` unless it is already `wayland` or
`x11`. Applications built on sola-kit also set `GSETTINGS_BACKEND=memory`
when it is unset. The session manager builds the environment of the
applications it launches from its own; see [Session manager](session.md).

## Ending the session

| Action | What happens |
|---|---|
| **Restart Computer** or **Shut Down** in the flower menu | The shell runs `sudo oath set host:local power=reboot` (`power=halt` for Shut Down), then `sudo oath apply --confirm host:local`. PID 1 stops every service (SIGTERM, up to 5 seconds, then SIGKILL) and reboots or powers off. If either command fails, the shell shows **Could not restart the computer** or **Could not shut down**. |
| `sudo reboot` or `sudo poweroff`, over SSH | The same shutdown by PID 1. |
| **Quit Sola** in the flower menu | The shell publishes `Shutdown` on the bus and exits with status 0. The session manager sends SIGTERM to every application, sends SIGKILL to any still running after 6 seconds, and exits with status 0. `sola-river` exits with status 0. |
| **Restart Shell** in the flower menu | The shell exits with status 1, and PID 1 starts it again. |
| `solactl emit Shutdown null` | As **Quit Sola**, except that the shell keeps running: it does not act on a `Shutdown` it did not send. |

After **Quit Sola**, the shell and the session manager stay stopped,
since their policy is `on-failure` and they exited cleanly. `sola-river`
is started again at once, and the bus, the call host, `sola-kvm`, rivulet,
and the tmux servers keep running, so the screen has no desktop until
PID 1 starts the two again ([Restarts](#restarts)). Restarting the
machine is the plain way back.

Neither **Quit Sola** nor a shutdown clears the saved set of open
applications, so the session manager reopens them when it next starts.

## Logs

| File | Written by | Limit |
|---|---|---|
| `~/.local/state/sola/sola.log` | Every Sola process, through `tracing`: plain text, UTC times | None on Oath: only the `sola` program rotates it |
| `~/.local/state/sola/app-<app id>.log` | Standard output and error of each application the session manager launched | Emptied at the next launch once it is larger than 8 MiB |
| `/oath/log/svc/<service>.log` | Standard output and error of each service, as PID 1 records them, each line with a UTC time in front. Sola's processes write their log lines here too, without colour. | Rotated to `<service>.log.1` past 256 KiB |

`~` is `/home`. Rivulet's output is only in `/oath/log/svc/rivulet.log`.
Read a service's file with `oath logs svc:<name>`, and Sola's with
`solactl logs <name>`, which prints `~/.local/state/sola/<name>.log`
(`sola.log` when no name is given); `-f` keeps following it:

```bash
oath logs svc:sola-session -n 200
oath logs svc:rivulet
solactl logs -f
solactl logs app-sola-mail -f
```

A line in `sola.log`:

```text
2026-10-29T09:12:03.512204Z  INFO [river]    sola::river    sola-river starting
```

The bracket is the process: the binary name without `sola-` (`[bus]`,
`[call]`, `[river]`, `[shell]`, `[session]`, `[kvm]`), or an
application's app id without `sola-` (`[mail]`). The next column is the
module: `sola::bus` is the bus library in any process.

Each process's default filter is its own crate and Sola's shared
libraries (`sola_core`, `sola_bus`, `sola_call`, `sola_kit`,
`sola_browser`, `sola_assets`) at `info`; other crates log nothing.
`RUST_LOG` replaces that filter, in `tracing` filter syntax. On Oath, set
it in a service's `env` and apply it, which restarts the service.
`oath set` replaces a field whole, so the new `env` must repeat the
variables `oath get svc:<name> --desired` shows:

```bash
oath get svc:sola-river --desired
sudo oath set svc:sola-river --from-json '{"env": {…the variables it has…, "RUST_LOG": "info,sola_river=debug"}}'
sudo oath apply svc:sola-river
sudo oath undo            # back to the previous env
```

`RUST_LOG=info,sola_bus::traffic=trace` on `svc:sola-bus` logs every bus
message; `RUST_LOG=debug` logs everything and is very large.

## Files and variables

| Path | Role |
|---|---|
| `/bin/sola-*`, `/bin/solactl` | Links to the programs in the store; PID 1 follows them to their package |
| `/oath/store/pkg/sola/share/cursors/` | The McMojave cursor theme |
| `/run/user/1/sola-bus`, `/run/user/1/sola-call` | The bus and call sockets |
| `/run/user/1/wayland-N` | Rivulet's socket |
| `~/.config/sola/state.yaml` | The bus's saved state, including `SessionApps`, the open-application set the session manager reopens |
| `~/.config/sola/tmux.conf` | The configuration both tmux services load |
| `~/.config/sola-kvm/config.toml` | `sola-kvm` settings |
| `~/.local/state/sola/` | Sola's logs |
| `/oath/log/svc/` | PID 1's record of each service's output |

| Variable | Effect |
|---|---|
| `RUST_LOG` | Log filter, per service |
| `SOLA_BIN_DIR`, `SOLA_SHARE_DIR`, `SOLA_LOG_DIR` | Replace `/bin`, `/oath/store/pkg/sola/share`, and `~/.local/state/sola` |
| `XDG_STATE_HOME` | When absolute, logs go to `$XDG_STATE_HOME/sola` |
| `XDG_RUNTIME_DIR` | Where the sockets are. When unset, as in an SSH login on Oath, the user's `/run/user/<uid>` when it exists, else `/tmp` |
| `XDG_CONFIG_HOME` | Where `sola/` and `sola-kvm/` live (`~/.config` when unset) |
| `SOLA_BUS_PATH`, `SOLA_CALL_PATH` | Replace the bus and call socket paths |

Sola takes Oath's paths when `/oath/INDEX.md` is a file.

## Troubleshooting

| Symptom | Cause |
|---|---|
| `oath get svc:<name> --actual` shows `failed` | It exited within 10 seconds of starting five times in a row. `oath logs svc:<name>` says why; it stays down until its spec or its package changes. |
| `restarting`, with `restarts` climbing | It fails at start. See its `oath logs` and `ERROR` or `panicked` lines with its tag in `sola.log`. |
| `stopped` for `svc:sola-shell` and `svc:sola-session` | **Quit Sola** ended them cleanly. See [Ending the session](#ending-the-session). |
| `wayland socket never appeared` from `[river]` | Rivulet opened no socket in `/run/user/1` in time. See `oath logs svc:rivulet`. |
| `bus connect failed, retrying` from `[session]` | `sola-bus` is not running. See `oath get svc:sola-bus --actual`. |
| A new build does not run | It was not applied. `oath get pkg:sola` shows the live hash; `cargo make install` writes `/opt/sola/bin`, which nothing on Oath runs. |
| An open application runs an old build after an apply | It started before the apply, and the session manager did not restart. Close it and open it again. |
| Nothing in `sola.log` | `~/.local/state/sola` is not writable by `home`. Each process then logs only to its standard error, which PID 1 records in the service's file. |

## The sola program

Sola's source also builds `sola`, a process manager that starts the
compositor and Sola's processes itself, for a session started by hand
from a text console. It is not in `pkg:sola`. On Oath it prints
`sola: not on Oath; PID 1 runs Sola's services (oath ls --kind svc)` and
exits with status 1 before it starts anything.
