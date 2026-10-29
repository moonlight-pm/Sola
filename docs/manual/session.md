# Session manager

The session manager, `sola-session`, starts, tracks, and stops your
applications: everything opened from the launcher, and the applications
restored when a session starts. It has no window. It is a bus client and a
call-plane provider with the owner name `session`. On Oath, PID 1 runs it
as a service and starts it again when it exits with an error
([Architecture](architecture.md)).

The session manager does not decide which applications exist or what they
are called. The application catalog belongs to the shell's launcher and to
Settings; `sola-session` runs whatever command a request carries.

## Launching

### Requests

A launch arrives in one of two ways:

| Path | Sent by | Command run |
|---|---|---|
| `LaunchApp {app_id, command}` on the bus | The shell (the launcher, its Projects chooser, and the clock's panel, which opens Calendar); a web app's `--app` process when no Browser is running | The one in the request |
| The `launch` call: `solactl session launch <app_id> [--command CMD]` | You, scripts, agents | `--command`; else `/bin/<app_id>` if that file exists; else the app id itself |

The call does not read the catalog: without `--command` it runs the
program the app id names (`solactl session launch sola-mail` runs
`/bin/sola-mail`). A program that needs arguments needs `--command`.

```bash
solactl session launch sola-terminal
solactl session launch foot --command "foot -e htop"
```

`LaunchApp` is not kept on the bus. If `sola-session` is not running when
the launcher sends it, nothing starts, and the launcher's ghost row in
the bar goes after 20 s.

### The command

The command is split at whitespace into a program and its arguments.
There is no shell: no quoting, no variables, no `~`, no pipes. A program
name without a slash is looked up on `PATH`. An empty command fails with
`empty command`.

### Processes

On Oath each application is a direct child process of `sola-session`.
Where a systemd user manager runs (`$XDG_RUNTIME_DIR/systemd/private`
exists, or `systemctl --user is-system-running` reports running or
degraded), `sola-session` instead starts each application in a transient
scope of its own, `sola-app-<app_id>-<n>.scope`. Oath has no systemd, so
the rest of this chapter describes direct children.

### Environment and working directory

Applications inherit `sola-session`'s environment, which on Oath is the
one PID 1 gives the service. `sola-session` adds:

| Variable | Value |
|---|---|
| `WAYLAND_DISPLAY` | The first `wayland-N` socket in the runtime directory that accepts a connection, else `sola-session`'s own `WAYLAND_DISPLAY` |
| `XDG_SESSION_TYPE` | `wayland`, when it found a Wayland socket. Chromium-based applications and desktop portals treat other values as not Wayland. |
| `DISPLAY` | The first live X socket in `/tmp/.X11-unix`, as `:N`, when there is one |
| `XDG_RUNTIME_DIR` | Passed through when set |

It also removes `SOLA_NO_SELF_WATCH`, so a kit application watches its own
executable and executes the new file in place when it changes
([Architecture](architecture.md#updates-in-place)).

Applications start in `sola-session`'s working directory. Standard input
is `/dev/null`. Standard output and standard error go to
`~/.local/state/sola/app-<app_id>.log` (Sola's log directory), appended,
and emptied at the next launch once the file passes 8 MiB. In the file
name, characters of the app id other than letters, digits, `_`, and `-`
become `_`. If the file cannot be opened, the output goes to
`sola-session`'s own standard error.

### The result

After starting the process, `sola-session` emits
`LaunchResult {app_id, command, ok, error}`. `ok: true` means the process
started, not that it stayed up; an application that fails at once arrives
as an exit (see [Tracking](#tracking)). A program that does not exist
fails here, with `ok: false` and the system's error. The `launch` call
replies `{app_id, command, pid}`, or fails with the error.

For `ok: false` the shell shows `Failed to launch <app_id>: <error>` in
the top row for 5 s.

## Application ids

An app id names an application in four places:

- the catalog entry, built in or added in Settings → Applications;
- the launch request, which `sola-session` tracks the process under;
- the Wayland app id the program gives its windows, which `sola-river`
  reports in `Windows`;
- `CloseApp`, which the shell sends with the focused window's app id.

The shell matches windows to catalog entries exactly, then ignoring case,
for icons, names, and the ghost row of an app that is starting.
`sola-session` and `sola-river` match exactly. If a catalog id is `Orca`
and the window says `orca`, Super+Q still closes the window, but
`sola-session` does not stop the process it started, and a restart of
`sola-session` starts the application again although its window is open. Use
the window's own app id as the catalog id: the **Running, not in the
launcher** list in Settings shows it.

## The application catalog

The launcher lists the catalog: Sola's built-in applications first, then
the entries you add, in the order they arrive. It searches the names,
ignoring case. Each entry has:

| Field | Meaning |
|---|---|
| `app_id` | The identifier; see [Application ids](#application-ids) |
| `label` | The name in the launcher and the bar |
| `command` | What `LaunchApp` carries |
| `icon` | An icon pack name under `/oath/store/pkg/sola/share/icons` (`lucide/terminal`, `simpleicons/firefox`), or a path to an image |
| `kind` | `command`, or `web` for a [web app](web-apps.md) |
| `url` | A web app's start page |
| `profile` | The Browser profile a web app runs on |

### Built-in entries

The shell defines these in code: Oath, Settings, Monitor, Terminal,
Projects, Browser, Kit, Preview, Paint, Mail, Calendar, Bots, Arcade,
Scope, Design, and Spotify. Oath is listed only on Oath (where
`/oath/INDEX.md` exists). Each app id is `sola-` and the name in lower
case (`sola-settings`, `sola-browser`), and each runs `/bin/<app_id>`.
They are not stored, they do not appear in Settings, and they cannot be
edited there.

Monitor, Kit, Preview, Arcade, Scope, and Spotify are not in Sola's Oath
packages ([Architecture](architecture.md#packages)): on Oath, choosing one
fails with a launch error.

An entry you add with the same app id as a built-in replaces the built-in
in the launcher.

### Entries you add

Settings → Applications edits the other entries. Saving publishes the
entry as `Application`, a persistent bus topic with one slot per app id.
The bus stores it under `Application:` in `~/.config/sola/state.yaml` and
replays it to the shell and to Settings when they start. Changing an
entry's app id retracts the old entry and publishes the new one;
**Remove** retracts it. The launcher shows changes at once.

An entry needs an app id, a name, and a command. On save, a first word of
the command that is found on `PATH` is replaced by its absolute path.

**Running, not in the launcher** lists open windows whose app id has no
entry (Sola's own applications, every `sola-…` app id, excepted), with a
command guessed from the app id or from the window's process. **Add**
starts a new entry with both filled in.

Entries with no command never appear in the launcher.

### Web apps

An entry of kind `web` runs `/bin/sola-browser --app <app_id>`. Browser
writes these entries when you wrap a page and retracts them when you
remove the app; Settings does not list them. A running Browser opens the
app's window. With none running, the `--app` process sends `LaunchApp` for
`sola-browser`, so `sola-session` starts, tracks, and restores the Browser
itself, then hands the app over and exits. See [Web apps](web-apps.md).

### Desktop files

Sola does not read `.desktop` files. An application installed on the
system appears in the launcher only once you add it in Settings.

## Tracking

`sola-session` keeps a record of every process it starts, filed under the
launch's app id; several copies of one application have several records.
At least every 500 ms it checks each one. When a process has exited, it
drops the record and emits `UserAppExited {app_id, command, code, signal}`.
Exactly one of `code` (a normal exit) and `signal` (killed by a signal) is
set.

The shell shows `<app_id> exited (code N)` for a non-zero exit and
`<app_id> killed (signal N)` for a signal. A clean exit shows nothing.

A process started any other way is not tracked: from a terminal, by
`solactl open`, by another application, or by an earlier run of
`sola-session`. It has no exit fact and no place in the saved set, and
`solactl session close` does not find it. Its windows are ordinary
windows, and Super+Q still closes them through `sola-river`.

## Closing

`CloseApp` with an app id on the bus, or the `close` call, sends SIGTERM to
every tracked process of that app id that is not already stopping. Only
the process itself gets the signal, not the processes it started.

```bash
solactl session close sola-mail
```

A process that is stopping stays tracked until it exits, and then
`UserAppExited` follows. Closing it again in the meantime does nothing.
The call replies `{app_id, stopped}` with the number of processes stopped,
or fails with `no live children for <app_id>` when nothing tracked matches.

Super+Q sends `CloseApp` for the focused application, which reaches three
receivers at once: `sola-session` stops the process, `sola-river` asks
every window with that app id to close ([Compositor](compositor.md)), and
a kit application quits by itself. `solactl session close` does only the
first.

## Ending the session

On `Shutdown` (**Quit Sola** in the flower menu), `sola-session` sends
SIGTERM to every application it is tracking, waits up to 6 s for them to
exit, sends SIGKILL to any that remain, then exits. It does not change the
saved set, so the applications open at **Quit Sola** are the ones
restored.

## Restoring the last session

Restore has three parts, each kept by a different process:

| Part | Kept by | Saved in |
|---|---|---|
| Which applications run | `sola-session` | `SessionApps` in `~/.config/sola/state.yaml` |
| What each application has open: its tabs, a window each | The application | Its own files (below) |
| Where each window goes: its workspace, zone, and place in the bar, which tab each zone shows, the workspaces themselves, and which one shows | The shell | `TabLayout` in `~/.config/sola/shell/tabs.yaml` |

### Applications

`sola-session` publishes the applications it is running as `SessionApps`,
a persistent, sorted list of `{app_id, command}` with one entry per app id.
It publishes the list again after every launch, close, and exit. An
application you close, or one that quits or crashes, leaves the list.

When it starts, `sola-session` spends 750 ms collecting the stored
`SessionApps` and the current `Windows`. Then it launches every listed
application that has no window with that app id and is not already
tracked, once, with its stored command. A stored entry for an application
that has since been renamed launches under the new name (`sola-workspaces`
runs as `sola-projects`). After a fresh start everything on the list
opens. After a restart of `sola-session` alone, the applications that
still have windows are not launched again.

### Tabs

Each application reopens what it had open by itself, a window per tab:

| Application | Reopens | From |
|---|---|---|
| Terminal | Every saved tab, its panes attached to their tmux sessions again; sessions the tmux server lost (after a reboot) are made again with new shells | `TerminalSession` entries in `~/.config/sola/state.yaml` |
| Browser | Every page of the active profile and of each workspace's profile; a web app's pages open as that app | `session.json` in each profile's folder under `~/.local/share/sola/browser/profiles/` |
| Paint | Every saved image that still exists | `~/.config/sola/paint.yaml` |
| Projects | Every open project, with its selected tab attached | `~/.config/sola/projects/catalog.json` |
| Design | A window for each project in its list | `~/.config/sola/design/projects.json` |

The other applications open one window. What a tab holds is described in
each application's notes ([Applications](applications.md)).

### Places

A window an application opens as a tab is announced first, with a key
that stays the same across restarts: `<app_id>/<tab id>`
(`sola-terminal/<tab id>`, `sola-browser/<page id>`, `<web app id>/<page id>`,
`sola-paint/<path>`, `sola-projects/<project id>`,
`sola-design/<project id>`). A window that is not an announced tab goes by
its app id.

The shell saves the bar as `TabLayout` whenever it changes. When the shell
starts, it takes the saved layout once. For the next 2 minutes, a window
that arrives takes its saved workspace, zone, and place, matched by its key
and then by its app id, and a zone shows the tab it showed. A window with
no saved place joins the showing workspace, after the focused tab. A
floating window also gets back its saved rectangle
([Compositor](compositor.md)).

A window that closes keeps its place for 30 s, so an application that
restarts or executes a new version in that time gets its windows back where
they were.

## One instance per application

`sola-session` has no notion of a single instance: every launch starts a
new process. Applications that keep one instance do it themselves:
Browser (with its web apps), Terminal, Paint, Projects, Design, and
Calendar. A second process hands its request to the running one and
exits; `sola-session` reaps it like any other exit.

| Application | A second process |
|---|---|
| Browser | Hands over its URL, a request for a new page, or a web app to open |
| Terminal | Asks for a new tab, which opens as a new window |
| Paint | Hands over its image paths |
| Projects | Asks the running one to show its selected project's window |
| Design | Asks the running one to come forward |
| Calendar | Hands over its file paths |

The launcher does not start a second copy of an application that has a
window: choosing it brings that application's last-used window forward
(a hidden one is shown again). Terminal is always started again, so its
launch opens a new window. Choosing Projects lists your projects
([Shell](sola-shell.md)). Choosing Browser asks for an address or a search
and opens it as a new page through `open_url` ([URLs](#urls)); when no
Browser is running, that starts the Browser outside `sola-session`, which
then does not track it or restore it. Other ways in
(`solactl session launch`, opening a file) still reach the application's
own single-instance handling.

## Opening URLs and files

Opening a URL, an image, or a calendar file does not go through
`sola-session`. Every Sola component that opens one uses the same
functions in `sola-core`, and they start the target application directly.

### URLs

`sola_core::open_url`:

1. Turns a local path into a `file://` URL: an absolute path always, a
   relative path when the file exists or the path starts with `./` or
   `../`.
2. If the Browser is running, writes the URL to its socket,
   `~/.local/share/sola/browser/chrome.sock` (under `$XDG_DATA_HOME` when
   that is set). The Browser opens it as a new page, in front.
3. Otherwise starts `/bin/sola-browser <url>` in the background, or the
   binary that `SOLA_BROWSER` names.

There is no fallback to another browser. If the binary is missing, the
open fails.

Applications that want the shell involved emit `OpenUrl {url, activate}`
on the bus instead. A running Browser opens the page itself, as the active
page with `activate`; when the Browser is not running, the shell calls
`open_url`.

### Images

`sola_core::open_image` treats a path, or a `file://` URL, ending in
`.png`, `.jpg`, `.jpeg`, `.jpe`, `.gif`, `.webp`, `.bmp`, `.tif`,
`.tiff`, or `.tga` (any case) as an image. It starts
`/bin/sola-paint <path>`, or the binary that `SOLA_PAINT` names. A running
Paint receives the path from the new process and opens it in a window of
its own ([Paint](sola-paint.md)).

### Calendar files

`sola_core::open_calendar` treats a path, or a `file://` URL, ending in
`.ics`, `.ical`, or `.icalendar` (any case) as a calendar file. It starts
`/bin/sola-calendar <path>`; a running Calendar receives the path and
drafts the event ([Calendar](sola-calendar.md)).

### solactl open

`solactl open <target>` sends an image to Paint, a calendar file to
Calendar, and anything else to `open_url`. It exits with status 3 when the
open fails.

```bash
solactl open https://example.com
solactl open ~/Pictures/shot.png
solactl open ~/Downloads/invite.ics
```

Sola's Oath packages carry no `.desktop` files, so on Oath Sola registers
no default handlers for `xdg-open`.

## Bus topics and call methods

[The bus](bus.md) documents the encoding and the persistent rules.

| Topic | Direction | Payload |
|---|---|---|
| `LaunchApp` | Read | `{app_id, command}` |
| `CloseApp` | Read (also by `sola-river` and kit applications) | An app id |
| `Shutdown` | Read | None |
| `SessionApps` | Read at start; published | List of `{app_id, command}`, persistent |
| `Windows` | Read at start | See [Compositor](compositor.md) |
| `LaunchResult` | Published | `{app_id, command, ok, error}` |
| `UserAppExited` | Published | `{app_id, command, code, signal}` |

Call methods, owner `session` (also the MCP tools `session_launch` and
`session_close`):

| Method | Parameters | Reply |
|---|---|---|
| `launch` | `app_id`; `command` (optional) | `{app_id, command, pid}` |
| `close` | `app_id` | `{app_id, stopped}` |

## Restarts

| What restarts | What happens |
|---|---|
| `sola-session` | The new one does not know the applications the previous one started: no `UserAppExited`, `solactl session close` does not find them, and they leave `SessionApps` at the next launch, close, or exit. Super+Q still closes their windows. Restore launches only listed applications that have no window. |
| `sola-bus` | `sola-session` reconnects and subscribes again. A `LaunchApp` sent while the bus was down is lost. |
| The compositor | See [Architecture](architecture.md#when-a-part-stops). |

## Logs

| File | Contents |
|---|---|
| `~/.local/state/sola/sola.log` | `sola-session`'s log, lines tagged `[session]`: `launch`, `user app launched` with the pid, `CloseApp: stopping`, `user app exited`, and what restore decided |
| `~/.local/state/sola/app-<app_id>.log` | Each application's standard output and standard error |

```bash
solactl logs app-sola-mail --follow
```

## Limits

- No `.desktop` files are read; the launcher lists only built-in entries
  and entries added in Settings or by Sola applications.
- Commands are split at whitespace. There is no quoting and no shell.
- Matching between catalog, launch, windows, and `CloseApp` is by exact
  app id.
- A restart of `sola-session` loses track of the applications it started.
- Applications opened through `open_url`, `open_image`, `open_calendar`, a
  terminal, or another program are not tracked and are not restored. That
  includes a Browser started from the launcher's Browser field, or by a
  link, while no Browser was running.
- Restore starts one process per application with its launch command. It
  does not restore a second copy launched with a different command.
- There is no autostart list besides session restore.
- Closing sends SIGTERM to the application's process only, not to the
  processes it started.
