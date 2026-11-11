# Session manager

The session manager, `sola-session`, starts, tracks, and stops your
applications: everything opened from the launcher, and the applications
restored when a session starts. It has no window. It is a bus client and a
call-plane provider with the owner name `desk` (`solactl desk`). On Oath, PID 1 runs it
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
| `LaunchApp {app_id, command}` on the bus | The shell (the launcher — Projects included, when it is not running — and the clock's panel, which opens Calendar); a web app's `--app` process when no Browser is running | The one in the request |
| The `launch` call: `solactl desk launch <app_id> [--command CMD]` | You, scripts, agents | `--command`; else `/bin/<app_id>` if that file exists; else the app id itself |

The call does not read the catalog: without `--command` it runs the
program the app id names (`solactl desk launch sola-mail` runs
`/bin/sola-mail`). A program that needs arguments needs `--command`.

```bash
solactl desk launch sola-terminal
solactl desk launch foot --command "foot -e htop"
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
| `SOLA_APP` | The app id, for an application that isn't Sola's ([below](#applications-that-arent-solas)) |

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
Projects, Browser, Preview, Paint, Mail, Calendar, Bots, Scope, and
Design. Oath is listed only on Oath (where `/oath/INDEX.md` exists). Each
app id is `sola-` and the name in lower case (`sola-settings`,
`sola-browser`), and each runs `/bin/<app_id>`.
They are not stored, they do not appear in Settings, and they cannot be
edited there.

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
remove the app. Settings lists them under Applications → Web apps, where
one can be renamed or removed. A running Browser opens the app's window.
With none running, the `--app` process sends `LaunchApp` for
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
`solactl desk close` does not find it. Its windows are ordinary
windows, and Super+Q still closes them through `sola-river`.

## Closing

`CloseApp` with an app id on the bus, or the `close` call, sends SIGTERM to
every tracked process of that app id that is not already stopping. Only
the process itself gets the signal, not the processes it started.

```bash
solactl desk close sola-mail
```

A process that is stopping stays tracked until it exits, and then
`UserAppExited` follows. Closing it again in the meantime does nothing.
The call replies `{app_id, stopped}` with the number of processes stopped,
or fails with `no live children for <app_id>` when nothing tracked matches.

Super+Q sends `CloseApp` for the focused application, which reaches three
receivers at once: `sola-session` stops the process, `sola-river` asks
every window with that app id to close ([Compositor](compositor.md)), and
a kit application quits by itself. `solactl desk close` does only the
first.

An application that isn't Sola's quits whole instead, as the next section
says; the call replies `{app_id, quitting: true}`.

## Applications that aren't Sola's

An application whose command runs a program whose name does not start
with `sola` (Steam, any other X11 or Wayland program in the catalog) is
not Sola's. Kit applications and web apps (`sola-browser --app …`) are.

Such an application starts with `SOLA_APP=<app_id>` in its environment,
and every process it starts inherits it, including ones that leave its
process tree: Steam's pressure-vessel containers and webhelpers start
sessions of their own, and its logger ends up under PID 1. That variable
is how `sola-session` finds all of the application.

It runs only while it has a window on the desk. Sola has no tray, so an
application that hides in one when its window closes, as Steam does, would
otherwise keep running with nothing to show for it. A window counts as
the application's when its app id is the application's, or when its
process, or the nearest process above it, carries the application's
`SOLA_APP`. A game Steam started (app id `steam_app_<id>`) is Steam's, so
Steam stays while the game is open. Hidden windows and windows on other
workspaces count.

Closing the last window from Sola quits at once, without the 5 s: Steam
answers a close from the compositor by starting its webhelper again, and
its window comes back within a second, empty.

| When | `sola-session` quits it |
|---|---|
| Its last window is closed from Sola: the bar's ×, Super+W (`CloseWindow`) | Yes, at once |
| It had a window and has had none for 5 s (30 s in its first 2 minutes, while it starts): its own close button, its last window crashing | Yes |
| It has shown no window 2 minutes after it started | Yes |
| `CloseApp`: Super+Q, the `close` call | Yes, at once |
| **Exit Sola** | Yes, within the 6 s every application gets |

Quitting goes in steps until no process carries its `SOLA_APP`:

1. While it runs, its own quit command, when it has one. Steam's is
   `/bin/steam -shutdown`, which hands the request to the running Steam as
   **Steam → Exit** does, so Steam saves its state and ends its games.
   Steam's command starts Steam when none is running, so it is sent only
   while one is, and only once.
2. After 15 s (at once, with no quit command), SIGTERM to every process
   that carries its `SOLA_APP`.
3. After 5 s more, SIGKILL to the rest.

It leaves the saved set when it starts quitting, except at **Exit Sola**,
which keeps the set as it was. Picking it in the launcher while it quits
starts it again once it is gone.

```bash
solactl desk close steam
```

## Ending the session

On `Shutdown` (**Exit Sola** in the flower menu), `sola-session` sends
SIGTERM to every application it is tracking, waits up to 6 s for them to
exit, sends SIGKILL to any that remain, then exits. Applications that
aren't Sola's get their own quit command first (Steam's `-shutdown`), and
SIGKILL goes to every process left with their `SOLA_APP`. It does not change the
saved set, so the applications open at **Exit Sola** are the ones
restored.

## Restoring the last session

Restore has three parts, each kept by a different process:

| Part | Kept by | Saved in |
|---|---|---|
| Which applications run | `sola-session` | `~/.local/state/sola/session/apps.json` |
| What each application has open: its tabs, a window each | The application | Its own files (below) |
| Where each window goes: its workspace, zone, and place in the bar, which tab each zone shows, the workspaces themselves, and which one shows | The shell | `~/.local/state/sola/shell/tabs.json` |

### Applications

`sola-session` keeps the applications it is running as a sorted list of `{app_id, command}`
with one entry per app id, in `~/.local/state/sola/session/apps.json`.
It saves the list again after every launch, close, and exit. An
application you close, or one that quits or crashes, leaves the list.

An application that exits because its compositor went (rivulet restarted
under the running desk, say by an `oath apply` that moved `pkg:rivulet`)
did not
close: it stays on the list, and once a new compositor has answered for
2 s it is started again. `sola-session` tells by the compositor's Wayland
socket: the one the application was launched on no longer answers, or a
new one has taken its place. The log says `app went with its display` and
`the display is back: starting it again`.

When it starts, `sola-session` reads the stored list from its file, then
waits for `sola-river`'s first window list (`Windows`), which says what
is already running: there is no timer, so a late `sola-river` delays the
restore instead of spoiling it. Meanwhile a launch or a close works as
usual, and the stored list stays in the file. Then it launches every listed
application that has no window with that app id and is not already
tracked, once, with its stored command. A stored entry for an application
that has since been renamed launches under the new name (`sola-workspaces`
runs as `sola-projects`). After a fresh start everything on the list
opens. At boot no compositor may answer yet: then the listed applications
wait, as those that went with their display do, and start once one has
answered for 2 s (`session restore: no compositor yet`). Started with no
display, an application such as Steam would exit and leave the list.
After a restart of `sola-session` alone, the applications are still open
and nothing is launched: the listed ones that have windows
stay on the list (`keeping the saved apps already running`) until their
windows have been gone for 3 s.

### Tabs

Each application reopens what it had open by itself, a window per tab:

| Application | Reopens | From |
|---|---|---|
| Terminal | Every saved tab, its panes attached to their tmux sessions again; sessions the tmux server lost (after a reboot) are made again with new shells | `~/.local/state/sola/terminal/sessions.json` |
| Browser | Every page of the active profile and of each workspace's profile; a web app's pages open as that app | `session.json` in each profile's folder under `~/.local/share/sola/browser/profiles/` |
| Paint | Every saved image that still exists | `~/.local/state/sola/paint/session.json` |
| Projects | Every open project, with its selected tab attached | `~/.config/sola/projects/catalog.json` |
| Design | A window for each project in its list | `~/.config/sola/design/projects.json` |

The other applications open one window. What a tab holds is described in
each application's notes ([Applications](applications.md)).

### Places

A window an application opens as a tab is announced first, with a key
that stays the same across restarts: `<app_id>/<tab id>`. The window names
its tab too: a kit app gives it the Wayland app id `<app_id>#<token>` (the
key's token, `sola_bus::topics::tab_token`); `sola-river` publishes the app
id without it and the token in the window's `WindowFacts`, and the shell pairs each window
with the tab it names — whatever came first, and in whatever order the
windows were made. A window with no token (an app from before) pairs by
the order its process announced its tabs. Keys:
(`sola-terminal/<tab id>`, `sola-browser/<page id>`, `<web app id>/<page id>`,
`sola-paint/<path>`, `sola-projects/<project id>`,
`sola-design/<project id>`). A window that is not an announced tab goes by
its app id.

The shell saves the bar whenever it changes. When the shell
starts, it takes the saved layout once, and it places the windows already
up (applications kept through its restart) only after the bus has
replayed everything it holds (`Replayed`): the zone grid and the saved
layout are in first, whatever order they came in. For the next 2 minutes, a window
that arrives takes its saved workspace, zone, and place, matched by its key
and then by its app id, and a zone shows the tab it showed. An app with
windows on several workspaces gets one saved place per window. A window
with no key of its own that finds no place left, while a window of its app
that took the app's place is still open (while the layout is restored), joins that
one: same workspace and zone, right after it in the bar. Steam maps a
short-lived window first, which takes Steam's place, and its main window
after; the main window goes where Steam was. Any other window with no saved
place joins the showing workspace, after the focused tab. A floating window
also gets back its saved rectangle ([Compositor](compositor.md)).

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
and opens it as a new page through `open_url` ([Opening links and files](#opening-links-and-files)); when no
Browser is running, `sola-session` starts it. Other ways in
(`solactl desk launch`, opening a file) still reach the application's
own single-instance handling.

## Opening links and files

Every link or file Sola opens goes to the app Sola keeps for its scheme
(`mailto:`) or its type (the extension: `.png`): a link clicked in
Terminal, Projects, Mail or Calendar, a link the Browser does not load
itself, the Browser's Open on a download, the Oath app's home link,
`solactl open`, and on Oath `xdg-open` from any program (`gh auth login`,
`claude /login`, Steam). The library is `sola-open`
([spec](../specs/2026-10-31-sola-open-design.md)).

### Handlers

Apps **claim** the schemes and types they open. Sola's own apps ship
theirs:

| App | Schemes | Types |
|---|---|---|
| Browser | `http`, `https` | `html`, `htm`, `xhtml`, `pdf`, `svg`, `txt`, `json`, `xml` |
| Paint | — | `png`, `jpg`, `jpeg`, `jpe`, `gif`, `webp`, `bmp`, `tif`, `tiff`, `tga` |
| Calendar | — | `ics`, `ical`, `icalendar` |
| Mail | `mailto` | — |

A kit app claims when it starts (`BusSetup::opens`); the claim is kept in
`~/.config/sola/handlers/claims/<app id>.json` with the time it first
claimed. The handler for a scheme or a type is the first that is still
installed of:

1. The one you chose, in **Settings → Default Apps** (kept in
   `~/.config/sola/handlers/defaults.json`:
   `{"defaults": {"mailto:": "sola-mail", ".png": "sola-paint"}}`, an app
   id or a `.desktop` file's name). A claim never writes it.
2. Sola's own claimant (the table).
3. Other claimants, the earliest claim first. A later claim never moves
   ahead of an earlier one, so a new app does not take a scheme or a type
   from the app that has it.
4. For a scheme only: `~/.config/mimeapps.list` (`[Default Applications]`,
   `x-scheme-handler/<scheme>`), then a `.desktop` file whose `MimeType`
   lists `x-scheme-handler/<scheme>`, in `~/.local/share/applications` and
   each `$XDG_DATA_DIRS` entry's `applications`. That is how
   `claude-cli://` reaches Claude Code.

A type nothing claims has no handler; nothing looks inside a file.

**Settings → Default Apps** lists every scheme (**Links**) and type
(**Files**) something opens, with the app that opens it. Click one for
everything that could: its claimants and, for a scheme, its `.desktop`
handlers; the one that opens it is marked. Choosing another writes your
choice; the next open uses it.

An app may ask to be the handler (`sola_kit::opener::request_default`,
the bus event `DefaultHandlerRequest {app_id, kind}`, `kind` being
`mailto:` or `.png`). The shell asks you with a card
([Shell](sola-shell.md#notifications)); only **Use** writes your choice.
An app that already opens it, or does not claim it, gets no card.

### Delivery

A claiming app answers the call-plane method `open` (`{target}`: the link,
or the file's absolute path) on its call owner: `browser`, `sola-paint`,
`sola-calendar`, `sola-mail`. When it is running, the opener calls it.
When it is not, the opener sends `LaunchApp` with the app's plain command,
so `sola-session` starts it, tracks it, and brings it back after a desk
restart; it waits up to 20 s for the app's `open`, then calls it. The
Browser opens the link as a page in front; Paint opens the image in a
window of its own; Calendar drafts the event; Mail opens a new letter with
the link's To, Cc, Subject and body (Mail has no Bcc, so a link's Bcc is
left out).

With no call plane, or an app from before `open`, the opener starts
`<app> --open <target>`: that process hands the target to the running app
(the Browser over `chrome.sock`, Calendar over its socket, Paint by
`OpenImage`), or has the session start the app and then hands it over;
with no session either, it becomes the app itself. A `.desktop` handler
runs its `Exec`, with `%u` `%U` `%f` `%F` the target and `%i` `%c` `%k`
left out.

### The Browser

A link to a scheme the Browser does not load (anything but `http`,
`https`, `file`, `about`, `data`, `blob`, `javascript`, `view-source`,
`chrome`, `devtools`, `filesystem`, `ws`, `wss`, `sola`) goes to its handler
when you clicked it: `mailto:` to Mail. A page that goes there by itself
(no click) gets nothing; the log says `without a click: not opened`.

Applications that want the shell involved emit `OpenUrl {url, activate}`
on the bus instead. A running Browser opens the page itself, as the active
page with `activate`; when the Browser is not running, the shell starts it
through `sola_core::open_url`.

### xdg-open and solactl open

On Oath, `pkg:sola` puts Sola's opener at `/bin/xdg-open`, so every
program that calls xdg-open reaches the same handlers. `xdg-open
<file|URL>` exits 0 when opened, 1 on a usage error, 2 when the file is
not there, 3 when nothing opens it, and 4 when its handler failed;
`--help` and `--version` print and exit 0. `solactl open <target>` does the
same and exits 3 on any failure. `/bin/x-www-browser` is the same opener: the
last name Rust's `webbrowser` crate tries, and it never calls `xdg-open`, so
that is how a program built on it (`grok`'s MCP sign-in) reaches a browser.

```bash
xdg-open https://example.com
xdg-open ~/Pictures/shot.png
solactl open "mailto:someone@example.com?subject=Hello"
```

A relative path is taken from the caller's working directory. `$BROWSER`
is not set on the desk; programs that read it fall back to `xdg-open` or
`x-www-browser`.

## Bus topics and call methods

[The bus](bus.md) documents the encoding and the persistent rules.

| Topic | Direction | Payload |
|---|---|---|
| `LaunchApp` | Read | `{app_id, command}` |
| `CloseApp` | Read (also by `sola-river` and kit applications) | An app id |
| `Shutdown` | Read | None |
| `Windows` | Read at start | See [Compositor](compositor.md) |
| `LaunchResult` | Published | `{app_id, command, ok, error}` |
| `UserAppExited` | Published | `{app_id, command, code, signal}` |

Call methods, owner `desk` (also the MCP tools `desk_launch` and
`desk_close`):

| Method | Parameters | Reply |
|---|---|---|
| `launch` | `app_id`; `command` (optional) | `{app_id, command, pid}` |
| `close` | `app_id` | `{app_id, stopped}` |

## Restarts

| What restarts | What happens |
|---|---|
| `sola-session` | Open applications keep running, but the new `sola-session` did not start them: no `UserAppExited`, and `solactl desk close` does not find them. The listed ones with windows stay in the saved list while they have windows (3 s grace). Super+Q still closes their windows. Restore launches only listed applications that have no window. A listed one that isn't Sola's is watched as before: once its windows are gone for 5 s, it quits whole. An apply that moves `pkg:sola` restarts `sola-session` the same way: `svc:sola-session` keeps its children (`keep_children`), so the applications, and a game under Steam, keep running on the program they started with. Without it, PID 1 stops them with the service and the new one opens them again. An application of Sola's whose own package moved is stopped and started again on the new program, alone (its running program against the one its command names: checked as the session manager starts, and when an apply says it moved packages, `PackagesMoved`); one that isn't Sola's never is. |
| `sola-bus` | `sola-session` reconnects and subscribes again. A `LaunchApp` sent while the bus was down is lost. |
| The compositor | The applications lose their display and exit; they stay in the saved list and start again on the new compositor. See [Architecture](architecture.md#when-a-part-stops). |

## Logs

| File | Contents |
|---|---|
| `~/.local/state/sola/sola.log` | `sola-session`'s log, lines tagged `[session]`: `launch`, `user app launched` with the pid, `CloseApp: stopping`, `user app exited`, `quitting` with the reason and `quit: …` for each step, and what restore decided |
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
- Applications started from a terminal or by another program (not through
  `sola-session` or `open_url`, `open_image`, `open_calendar`) are not
  tracked and are not restored.
- Restore starts one process per application with its launch command. It
  does not restore a second copy launched with a different command.
- There is no autostart list besides session restore.
- Closing one of Sola's applications sends SIGTERM to its process only,
  not to the processes it started.
- An application that isn't Sola's and was started some other way (a
  terminal, or a `sola-session` older than 2026-10-31) has no `SOLA_APP`,
  so nothing watches its windows and closing does not find all of it.
- An application that isn't Sola's and never shows a window (a
  command-line tool in the catalog) is quit after 2 minutes.
