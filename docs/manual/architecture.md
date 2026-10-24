# Architecture

A Sola session is a set of separate processes on one machine, started by one
process manager from a text console. River is the Wayland compositor; every
other part is a Sola binary in `/opt/sola/bin`. The parts talk over three
channels: the bus for facts, the call plane for requests, and Wayland for
surfaces and input. Each managed component can be restarted on its own.
River cannot: when it exits, the session ends.

## Processes

```text
login shell on a console
└─ sola                  process manager
   ├─ river              Wayland compositor
   ├─ sola-bus           event bus
   ├─ sola-call          call host
   ├─ sola-botsd         bots daemon
   ├─ sola-river         River's window manager and bus bridge
   ├─ sola-shell         menubar, launcher, switcher, zones
   ├─ sola-session       application launcher and tracker
   │  └─ applications    each in its own systemd user scope
   └─ sola-kvm           KVM server
```

| Process | Owns |
|---|---|
| `sola` | Starting River and the managed components, restarting them when they exit or their binary changes, and ending the session. See [Process manager](process-manager.md). |
| `river` | Outputs, rendering, input devices, and XWayland. An external program found on `PATH`; the NixOS module installs a patched build. |
| `sola-bus` | The bus socket, and the current value of every sticky and persistent topic. See [The bus](bus.md). |
| `sola-call` | The call socket, and the registry of live owners and their methods. See [The call plane](call.md). |
| `sola-river` | River's window manager (`river-window-management-v1`). It places, stacks, and focuses windows as the shell asks, binds key chords, and reports windows and outputs on the bus. Call owner `compositor`. See [Compositor](compositor.md). |
| `sola-shell` | The menubar, launcher, switcher, notifications, and zones. It decides which windows are shown and where, and tells `sola-river`. See [Shell](sola-shell.md). |
| `sola-session` | Launching, tracking, and closing applications, and relaunching the previous session's applications at start. Call owner `session`. See [Session manager](session.md). |
| `sola-botsd` | Named agent sessions. Call owner `bots`, and an HTTP API on port 27419. See [solactl](solactl.md). |
| `sola-kvm` | Sharing this machine's keyboard and mouse with another machine. See [KVM](sola-kvm-operator.md). |
| Applications | Their own windows. Sola's applications are built on sola-kit and are Wayland clients and bus clients; `sola-projects`, `sola-browser`, and `sola-design` are also call owners. Other programs started through the session manager are plain Wayland or X11 clients. See [Applications](applications.md). |

## The three planes

| Plane | Socket | Shape | Carries |
|---|---|---|---|
| Bus | `$XDG_RUNTIME_DIR/sola-bus` | One sender, every subscriber, no reply | Window lists, focus, outputs, theme, application menus, launch and close requests, notifications, shutdown |
| Call | `$XDG_RUNTIME_DIR/sola-call` | One caller, one owner, one reply | `solactl` commands, agent tools, screenshots, synthetic input, application control |
| Wayland | `$XDG_RUNTIME_DIR/wayland-N` | Client and compositor | Buffers, windows, layer surfaces, keyboard and pointer input |

The bus carries facts. A message goes to every process subscribed to its
topic kind, and the sender gets no answer. A topic is ephemeral, sticky, or
persistent. The bus keeps the latest value of each sticky topic and replays
it to every new subscriber; it also writes persistent topics to disk. A
process that starts late, or restarts, therefore learns the current state as
soon as it subscribes.

The call plane carries requests. An owner (`compositor`, `session`,
`projects`, …) advertises its methods. A caller names an owner and a
method and gets exactly one reply, or an error: `<owner> is not running`,
`<owner> has no method <method>`, or `timeout` (8 seconds unless the caller
sets another). The bus has no reply path, so requests do not travel on it.

Wayland carries pixels and input between River and its clients. Every Sola
window is an ordinary Wayland surface; `sola-river` is the one client that
also manages everybody else's windows.

The planes are separate processes and sockets, so one can restart without
the others: windows stay on screen while the bus restarts, and the bus keeps
delivering while the call host restarts.

## Sockets and files

### Runtime directory

| Path | What |
|---|---|
| `$XDG_RUNTIME_DIR/wayland-N` | River's Wayland socket |
| `$XDG_RUNTIME_DIR/sola-wayland` | The name of that socket (`wayland-1`), written by `sola` once River accepts connections |
| `$XDG_RUNTIME_DIR/sola-display` | The X display River's XWayland serves (`:0`); absent when XWayland did not come up within 3 seconds of River |
| `$XDG_RUNTIME_DIR/sola-bus` | The bus |
| `$XDG_RUNTIME_DIR/sola-call` | The call plane (mode 0600) |
| `$XDG_RUNTIME_DIR/sola/` | Single-instance sockets of Arcade, Paint, and Design |

If `XDG_RUNTIME_DIR` is unset, Sola's processes use `/tmp` instead.
`SOLA_BUS_PATH` and `SOLA_CALL_PATH` override the bus and call socket paths
for every process that has them in its environment.

### Configuration and state

`~/.config` below means `$XDG_CONFIG_HOME` when that is an absolute path.

| Path | What |
|---|---|
| `~/.config/sola/state.yaml` | Persistent bus topics, one section each: zones, floating window geometry, the open-application set, the application catalog, terminal sessions, and the mail, calendar, terminal, and monitor settings |
| `~/.config/sola/theme/current.yaml` | The current theme |
| `~/.config/sola/theme/presets/<name>.yaml` | Saved themes |
| `~/.config/sola/browser.yaml`, `browser/`, `paint.yaml` | Browser and Paint topics that have files of their own |
| `~/.config/sola/<app>/`, `~/.local/state/sola/<app>/`, `~/.cache/sola/<app>/` | Application files; each application's notes list its own ([Applications](applications.md)) |
| `~/.config/sola-kvm/config.toml` | `sola-kvm` |

The bus reads its files only when it starts, and rewrites a topic's entry
each time a new value is published.

### Logs

| Path | What |
|---|---|
| `/opt/sola/log/sola.log` | The log of every Sola process, one line per event, tagged with the process |
| `/opt/sola/log/river.log` | River's own output |
| `/opt/sola/log/app-<app id>.log` | The output of each application the session manager launched |

Formats, rotation, and `RUST_LOG` are in [Logs](process-manager.md#logs).

## Start-up

1. You run `/opt/sola/bin/sola` on a text console. It rotates the log, sets
   the cursor theme variables, and starts watching `/opt/sola/bin`.
2. It kills any River the same user is already running, deletes stale
   `wayland-*` sockets, and starts `river`.
3. It waits up to 30 seconds for River's socket to accept a connection and
   writes its name to `sola-wayland`, then waits up to 3 seconds for
   XWayland and writes `sola-display`. If River exits first or never opens a
   socket, `sola` exits.
4. It starts `sola-bus`, `sola-call`, `sola-botsd`, `sola-river`,
   `sola-shell`, and `sola-session` one after another without waiting for
   any of them, then `sola-kvm` 750 ms later. Each waits for what it needs:
   bus and call clients retry until the socket accepts them, and Wayland
   clients read `sola-wayland`.
5. `sola-river` binds River's window-management protocol and publishes the
   outputs and windows. `sola-shell` maps the menubar.
6. `sola-session` collects the saved open-application set and the current
   window list for 750 ms, then launches every saved application that has
   no window.

## When a part stops

| What stops | What happens |
|---|---|
| Any managed component | `sola` starts it again: at once if it ran for 5 seconds or more, after 2 seconds if not. |
| `sola-bus` | Sticky values held only in memory are lost; persistent ones are reloaded from disk. Every client reconnects, subscribes again, and publishes its own sticky values again, so menus, chords, and window lists return. |
| `sola-call` | Calls in flight fail. Owners reconnect every 400 ms and advertise their methods again. |
| `sola-river` | River and all windows keep running. The new `sola-river` binds the window-management protocol again. |
| `sola-shell` | The menubar and overlays disappear until the new shell maps them; windows stay. **Restart Shell** in the flower menu does this on purpose. |
| `sola-session` | Running applications keep running. The new session manager launches saved applications that have no window, but does not track the ones still running, so their exits are not reported. |
| An application | `sola-session` publishes its exit (`UserAppExited`). Nothing restarts it. |
| River | The session ends. `sola` kills the managed components and exits with status 1. |
| `sola` | The kernel sends SIGTERM to River and to every managed component. |

## How an application attaches

A Sola application is an Iced program built on sola-kit. Before it opens a
window, it:

1. Calls `startup(app_id)`, which starts logging, reads `sola-wayland` (for
   up to 20 seconds, then falls back to `WAYLAND_DISPLAY`, then
   `wayland-0`), waits up to 10 seconds for that socket file, sets
   `XDG_SESSION_TYPE=wayland` if it was `tty`, points the graphics
   libraries at `/run/opengl-driver/`, loads the system fonts, and, unless
   `SOLA_NO_SELF_WATCH` is set, starts watching its own binary.
2. Connects to the bus with `BusSetup`: it identifies with its app id,
   subscribes to the topic kinds it handles, and publishes its application
   menu. `BusSetup` blocks until the bus accepts the connection.
   `.calls(owner, methods)` on it also advertises call methods; `CallSetup`
   advertises call methods without the bus.
3. Opens its window with the same app id as the Wayland `app_id`, which is
   how the shell matches menus and windows to the application.

When the bus goes away, the kit reconnects every 250 ms, subscribes again,
and republishes the sticky values the application published. The call
provider reconnects every 400 ms. Theme and fonts arrive as a persistent bus
topic and apply live; see [Theme and toolkit](theme.md).

## Updates in place

`sola` watches its own binary and the managed components' binaries. When a
managed component's file is replaced, `sola` kills that process and starts
the new binary. When `sola` itself is replaced, it stops every managed
component and executes the new `sola`, which restarts the whole session. An
application built on sola-kit watches its own binary and executes the new
file in place, keeping its process id and arguments. Details, including
release installs, are in [Binary watch](process-manager.md#binary-watch).
