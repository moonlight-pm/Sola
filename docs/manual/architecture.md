# Architecture

A Sola session is a set of separate processes on one machine. On Oath,
PID 1 starts them as the desk user, together with the compositor,
rivulet: Oath's port of River 0.4.5, which speaks the same protocols.
Sola's programs are in `/bin`. The parts talk over three planes: the bus
for facts, the call plane for requests, and Wayland for surfaces and
input. Each Sola process can stop and start again on its own while the
others carry on.

## Processes

```text
PID 1 (Oath)
├─ rivulet           Wayland compositor
├─ sola-bus          event bus
├─ sola-call         call host
├─ sola-river        window manager and bus bridge
├─ sola-shell        top row, bar, launcher, workspaces, zones
├─ sola-session      application launcher and tracker
│  └─ applications   its child processes
└─ sola-kvm          KVM server
```

| Process | Owns |
|---|---|
| rivulet | Outputs, rendering, and input devices. Oath runs it, not Sola. See [Compositor](compositor.md). |
| `sola-bus` | The bus socket, and the current value of every sticky and persistent topic. See [The bus](bus.md). |
| `sola-call` | The call socket, and the registry of live owners and their methods. See [The call plane](call.md). |
| `sola-river` | The compositor's window manager (`river-window-management-v1`). It places, stacks, and focuses windows as the shell asks, binds key chords, and reports windows and outputs on the bus. Call owner `compositor`. See [Compositor](compositor.md). |
| `sola-shell` | The top row, the bar, the launcher, notifications, desktop workspaces, and zones. It decides which windows are shown and where, tells `sola-river`, and saves the layout that puts windows back after a restart. See [Shell](sola-shell.md). |
| `sola-session` | Launching, tracking, and closing applications, and relaunching the previous session's applications at start. Call owner `session`. See [Session manager](session.md). |
| `sola-kvm` | Sharing this machine's keyboard and mouse with another machine. See [KVM](sola-kvm-operator.md). |
| Applications | Their own windows. Sola's applications are built on sola-kit and are Wayland clients and bus clients. Terminal, Browser, Paint, Projects, and Design open a window per tab (a terminal tab, a page, an image, a project) and announce each one to the shell, which lists it in the bar. Projects, Browser, and Design are also call owners. Other programs started through the session manager are plain Wayland or X11 clients. See [Applications](applications.md). |

Some processes run outside that tree:

- `sola-botsd` keeps named agent sessions, each a directory in `~/Bots`
  on Grok, Claude, or Gab AI. It is call owner `bots` and serves an HTTP
  API on the socket `$XDG_RUNTIME_DIR/sola-bots`. It ships with Sola,
  but Oath runs no service for it, so it runs only when you start it.
  See [solactl](solactl.md).
- Terminal's shells run in a tmux server on the socket `sola`, and
  Projects' on `sola-ws`. Each application starts its server when it
  needs one, and the server outlives the application.
- `sola`, Sola's own process manager, starts and supervises these
  processes where nothing else does. On Oath it refuses to start, since
  PID 1 does that job. See [Process manager](process-manager.md).

## The three planes

| Plane | Socket | Shape | Carries |
|---|---|---|---|
| Bus | `$XDG_RUNTIME_DIR/sola-bus` | One sender, every subscriber, no reply | Window lists, focus, outputs, tabs and the bar's layout, workspaces, theme, application menus, launch and close requests, notifications, shutdown |
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

Wayland carries pixels and input between the compositor and its clients.
Every Sola window is an ordinary Wayland surface; `sola-river` is the one
client that also manages everybody else's windows.

The planes are separate processes and sockets, so one can restart without
the others: windows stay on screen while the bus restarts, and the bus keeps
delivering while the call host restarts.

## Programs and files

### Packages

Sola ships on Oath as three packages. Their programs appear in `/bin`, and
Sola's icons and cursors are in `/oath/store/pkg/sola/share`.

| Package | Programs |
|---|---|
| `pkg:sola` | `sola-bus`, `sola-call`, `sola-river`, `sola-shell`, `sola-session`, `sola-kvm`, `sola-botsd`, `solactl`, and Terminal, Projects, Paint, Calendar, Mail, Settings, Bots, and Oath |
| `pkg:sola-browser` | Browser, which also hosts the web apps |
| `pkg:sola-design` | Design |

The launcher also lists Monitor, Kit, Preview, Arcade, Scope, and Spotify;
none of these packages carries them. `SOLA_BIN_DIR`, `SOLA_SHARE_DIR`, and
`SOLA_LOG_DIR` override the program, share, and log directories.

### Runtime directory

If `XDG_RUNTIME_DIR` is unset, Sola's processes use `/run/user/<uid>` when
that directory exists, else `/tmp`. `SOLA_BUS_PATH` and `SOLA_CALL_PATH`
override the bus and call socket paths for every process that has them in
its environment.

| Path | What |
|---|---|
| `$XDG_RUNTIME_DIR/wayland-N` | The compositor's Wayland socket. Sola's processes use the first one, by name, that accepts a connection (`wayland-x…` and `.lock` files are skipped). |
| `$XDG_RUNTIME_DIR/sola-bus` | The bus |
| `$XDG_RUNTIME_DIR/sola-call` | The call plane (mode 0600) |
| `$XDG_RUNTIME_DIR/sola-bots` | `sola-botsd`'s HTTP API |
| `$XDG_RUNTIME_DIR/sola-ws-hooks.sock` | Projects' agent hooks; `sola-ws-hooks-<host>.sock` for each computer a project runs on |
| `$XDG_RUNTIME_DIR/sola-ssh/` | Projects' shared ssh connections |
| `$XDG_RUNTIME_DIR/sola/` | Single-instance sockets: `terminal.sock`, `projects.sock`, `paint.lock.sock`, `design.sock`, `calendar.sock` |

The Browser's single-instance socket is
`~/.local/share/sola/browser/chrome.sock` (under `$XDG_DATA_HOME` when that
is set). Where `sola` runs the compositor, it also writes the files
`sola-wayland` and `sola-display` here, naming the Wayland socket and the X
display; on Oath they are absent, and each process finds the sockets
itself.

### Configuration and state

`~/.config` below means `$XDG_CONFIG_HOME` when that is an absolute path.

| Path | What |
|---|---|
| `~/.config/sola/state.yaml` | Persistent bus topics without a file of their own, one section each: the open-application set, the application catalog, floating window geometry, terminal tabs, the background picture, appearance, and the mail, calendar, terminal, and monitor settings |
| `~/.config/sola/shell/tabs.yaml` | The bar's layout: the workspaces, which one shows, and each window's workspace, zone, and place |
| `~/.config/sola/theme/current.yaml` | The current theme |
| `~/.config/sola/theme/presets/<name>.yaml` | Saved themes |
| `~/.config/sola/paint.yaml` | Paint's open images |
| `~/.config/sola/calendar/agenda.yaml` | Calendar's coming days, for the clock's panel |
| `~/.config/sola/background/` | Sola's copy of the background picture |
| `~/.config/sola/<app>/`, `~/.local/share/sola/<app>/`, `~/.local/state/sola/<app>/`, `~/.cache/sola/<app>/` | Application files; each application's notes list its own ([Applications](applications.md)) |
| `~/.config/sola-kvm/config.toml` | `sola-kvm` |

The bus reads its files only when it starts, and rewrites a topic's entry
each time a new value is published.

### Logs

Sola's log directory on Oath is `$XDG_STATE_HOME/sola` when that is an
absolute path, else `~/.local/state/sola`.

| Path | What |
|---|---|
| `~/.local/state/sola/sola.log` | The log of every Sola process, one line per event, tagged with the process |
| `~/.local/state/sola/app-<app id>.log` | The standard output and standard error of each application the session manager launched |

Every Sola process also writes its log lines to its standard error.
`RUST_LOG` sets the filter; without it, a process logs its own crate and
Sola's shared libraries at `info`. Only `sola` rotates `sola.log`, when it
starts, so on Oath the file is not rotated.

## Start-up

Sola's processes do not depend on the order they start in: each one waits
for what it needs.

1. PID 1 starts the compositor and the services `sola-bus`, `sola-call`,
   `sola-river`, `sola-shell`, `sola-session`, and `sola-kvm`.
2. `sola-bus` binds its socket and loads the persistent topics from disk.
   `sola-call` binds its socket with an empty registry.
3. `sola-river` waits up to 20 seconds for a live `wayland-N` socket and
   exits with an error if none comes. It binds the window-management
   protocol, reports outputs and windows, and tries the bus every 20 ms
   until it connects.
4. `sola-shell` starts like any kit application (below), maps the top row
   and the bar, and takes the saved layout when the bus replays it.
5. `sola-session` tries the bus every second until it connects, collects
   the saved open-application set and the current window list for 750 ms,
   then launches every saved application that has no window.
6. Each application reopens its own tabs, and the shell puts each window
   back in its saved workspace, zone, and place
   ([Session manager](session.md#restoring-the-last-session)).

Call owners connect to `sola-call` whenever it answers, retrying every
400 ms, so their methods appear as soon as both are up.

## When a part stops

PID 1 starts a service again when it exits with an error. A service that
exits cleanly stays down; that is how **Exit Sola** ends the session.

| What stops | What happens |
|---|---|
| `sola-bus` | Sticky values held only in memory are lost; persistent ones are reloaded from disk. Every client reconnects, subscribes again, and publishes its own sticky values again, so menus, chords, tabs, and window lists return. |
| `sola-call` | Calls in flight fail. Owners reconnect every 400 ms and advertise their methods again. |
| `sola-river` | The compositor and every window keep running. The new `sola-river` binds the window-management protocol again and reports outputs and windows. |
| `sola-shell` | The top row, the bar, and the overlays go until the new shell maps them; windows stay. The new shell takes the saved layout and pairs the open windows with their tabs again. |
| `sola-session` | The new one launches saved applications that have no window. It does not track the applications the previous one started: their exits are not reported, `solactl session close` does not find them, and they leave the saved set at its next change. |
| An application | `sola-session` publishes its exit (`UserAppExited`) and drops it from the saved set. Nothing restarts it. Its windows' places in the bar wait 30 seconds, so a copy started again within that time gets them back. |
| The compositor | Every Wayland client loses its connection. `sola-river` and the shell exit with an error, PID 1 starts them again, and they wait for a live Wayland socket. |

### Ending the session

**Exit Sola** in the flower menu publishes `Shutdown`, and the shell exits
cleanly. `sola-session` stops every application it launched (SIGTERM, then
SIGKILL to any still running after 6 seconds) and exits; `sola-river`
exits. None of the three is started again. `sola-bus`, `sola-call`, and
`sola-kvm` do not act on `Shutdown` and keep running. **Restart Computer**
and **Shut Down** are described in [Shell](sola-shell.md).

## How an application attaches

A Sola application is an Iced program built on sola-kit. Before it opens a
window, it:

1. Calls `startup(app_id)`, which starts logging, finds the compositor's
   socket (the first `wayland-N` in the runtime directory that accepts a
   connection, looked for during up to 20 seconds; then
   `WAYLAND_DISPLAY`; then `wayland-0`), waits up to 10 seconds for that
   socket file, sets `WAYLAND_DISPLAY`, sets `XDG_SESSION_TYPE=wayland`
   unless it is already `wayland` or `x11`, sets
   `GSETTINGS_BACKEND=memory` if unset, loads the system fonts, and,
   unless `SOLA_NO_SELF_WATCH` is set, starts watching its own
   executable.
2. Connects to the bus with `BusSetup`: it identifies with its app id,
   subscribes to the topic kinds it handles, and publishes its
   application menu, which its windows' titlebars draw. `BusSetup` blocks
   until the bus accepts the connection. `.calls(owner, methods)` on it
   also advertises call methods; `CallSetup` advertises call methods
   without the bus.
3. Opens its windows with the same app id as the Wayland `app_id`, which
   is how the shell matches menus, icons, and windows to the application.
   An application with a window per tab opens each through the kit's
   `TabWindows`, which first announces the tab on the bus (`Tab`, keyed
   `<app_id>/<tab id>`). The shell pairs the announcement with the new
   window, answers with `TabWindow` (the window's id and whether it is on
   screen), and lists it in the bar.

When the bus goes away, the kit reconnects every 250 ms, subscribes again,
and republishes the sticky values the application published. The call
provider reconnects every 400 ms. Theme and fonts arrive as a persistent bus
topic and apply live; see [Theme and toolkit](theme.md).

## Updates in place

A new version of a package becomes live on Oath with `oath apply`. PID 1
then restarts the services that run from a package that moved. An
application built on sola-kit watches its own executable file; when that
file changes, it executes the new one in place, keeping its process id and
arguments, so `sola-session` goes on tracking it. The application then
reopens its tabs as it does at start, and the shell puts its windows back
in their places. `sola-session` removes `SOLA_NO_SELF_WATCH` from the
environment of the applications it launches, so each of them watches its
own file.
