# Session manager

The session manager, `sola-session`, starts, tracks, and stops your
applications: everything opened from the launcher, and the applications
restored when a session starts. It has no window. It is a bus client and a
call-plane provider with the owner name `session`. The process manager
starts it with the other components and restarts it when it exits (see
[Process manager](process-manager.md)).

The session manager does not decide which applications exist or what they
are called. The application catalog belongs to the shell's launcher and to
Settings; `sola-session` runs whatever command a request carries.

## Launching

### Requests

A launch arrives in one of two ways:

| Path | Sent by | Command run |
|---|---|---|
| `LaunchApp {app_id, command}` on the bus | The launcher; Arcade, for games | The one in the request |
| The `launch` call: `solactl session launch <app_id> [--command CMD]` | You, scripts, agents | `--command`; else `/opt/sola/bin/<app_id>` if that file exists; else the app id itself |

The call does not read the catalog: `solactl session launch sola-mail`
runs `/opt/sola/bin/sola-mail`, and anything else needs `--command`.

```bash
solactl session launch sola-terminal
solactl session launch firefox --command firefox
```

`LaunchApp` is not kept on the bus. If `sola-session` is not running when
the launcher sends it, nothing starts, and the launcher's **Opening …**
toast clears after 20 s.

### The command

The command is split at whitespace into a program and its arguments.
There is no shell: no quoting, no variables, no `~`, no pipes. A program
name without a slash is looked up on `PATH`. An empty command fails with
`empty command`.

### Scopes

When a systemd user manager is running, each application runs in its own
transient scope:

```text
systemd-run --user --scope --quiet --collect
    --unit=sola-app-<app_id>-<n>.scope --description="Sola app: <app_id>"
    --property=TimeoutStopSec=5s --property=KillSignal=SIGTERM
    -- <program> <arguments>
```

`<n>` counts launches since `sola-session` started, so two copies of one
application get two scopes. In the unit name, characters of the app id
other than letters, digits, `_`, and `-` become `_`. The scope holds every
process the application starts, so stopping it stops them all.

```bash
systemctl --user list-units 'sola-app-*'
```

`sola-session` counts a user manager as running when
`$XDG_RUNTIME_DIR/systemd/private` exists, or when
`systemctl --user is-system-running` reports running or degraded. Without
one, for example on a seat with no login session, it starts the program
directly as its own child.

### Environment and working directory

Applications inherit `sola-session`'s environment: the environment you
started `sola` in, plus the cursor variables the process manager sets
(`XCURSOR_THEME`, `XCURSOR_PATH`). `sola-session` adds:

| Variable | Value |
|---|---|
| `WAYLAND_DISPLAY` | River's socket, from `$XDG_RUNTIME_DIR/sola-wayland` |
| `XDG_SESSION_TYPE` | `wayland`. A console login leaves `tty`, and Chromium-based applications and desktop portals then treat the session as not Wayland. |
| `DISPLAY` | Xwayland's display, from `$XDG_RUNTIME_DIR/sola-display`, or else the first live socket in `/tmp/.X11-unix` |
| `XDG_RUNTIME_DIR` | Passed through |

It also removes `SOLA_NO_SELF_WATCH`. The process manager sets that
variable for its own components; without it, a kit application restarts
itself when its binary in `/opt/sola/bin` changes, which is how
`cargo make install <app>` reaches an application that is already open.

Applications start in `sola-session`'s working directory: the directory
you ran `sola` from, usually your home directory after a console login.

Standard input is `/dev/null`. Standard output and standard error go to
`/opt/sola/log/app-<app_id>.log`, appended, and emptied at the next launch
once the file passes 8 MiB. If that file cannot be opened, the output goes
to `sola-session`'s own standard error.

### The result

After starting the process, `sola-session` emits
`LaunchResult {app_id, command, ok, error}`. `ok: true` means the process
started, not that it stayed up: with a scope, `systemd-run` itself starts
even when the program does not exist, and that failure arrives as an exit
(see [Tracking](#tracking)). The `launch` call replies `{app_id, command, pid}`,
or fails with the error.

For `ok: false` the shell shows the toast
`Failed to launch <app_id>: <error>`.

## Application ids

An app id names an application in four places:

- the catalog entry, built in or added in Settings → Applications;
- the launch request, which `sola-session` tracks the process under;
- the Wayland app id the program gives its windows, which `sola-river`
  reports in `Windows`;
- `CloseApp`, which the shell sends with the focused window's app id.

The shell matches windows to catalog entries exactly, then ignoring case,
for icons, names, and the **Opening …** toast. `sola-session` and
`sola-river` match exactly. If a catalog id is `Orca` and the window says
`orca`, Super+Q still closes the window, but `sola-session` does not stop
the scope it started, and a restart of `sola-session` starts the
application again. Use the window's own app id as the catalog id: the
**Running, not configured** list in Settings shows it.

## The application catalog

The launcher lists the catalog: Sola's built-in applications first, then
the entries you add, in the order they arrive. It searches the names,
ignoring case. Each entry has:

| Field | Meaning |
|---|---|
| `app_id` | The identifier; see [Application ids](#application-ids) |
| `label` | The name in the launcher and the tab bar |
| `command` | What `LaunchApp` carries |
| `icon` | An icon pack name under `/opt/sola/share/icons` (`lucide/terminal`, `simpleicons/firefox`), or a path to an image |
| `kind` | `command`, or `web` for a [web app](web-apps.md) |
| `url` | A web app's start page |
| `profile` | The Browser profile a web app runs on |

### Built-in entries

The shell defines these in code: Settings, Monitor, Terminal, Projects,
Browser, Kit, Preview, Paint, Mail, Calendar, Bots, Arcade, Scope, Design,
and Spotify. Each app id is `sola-` and the name in lower case
(`sola-settings`, `sola-browser`), and each runs `/opt/sola/bin/<app_id>`.
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

**Running, not configured** lists open windows whose app id has no entry
(Sola's own applications excepted), with a command guessed from the app id
or from the window's process.
**Configure** starts a new entry with both filled in.

Other applications publish entries too. Arcade adds an entry with no
command so the tab bar can name a running game. Entries with no command
never appear in the launcher.

### Web apps

An entry of kind `web` runs `/opt/sola/bin/sola-browser --app <app_id>`.
Browser writes these entries when you wrap a page and retracts them when
you remove the app; Settings does not list them. See [Web apps](web-apps.md).

### Desktop files

Sola does not read `.desktop` files. An application installed on the
system appears in the launcher only once you add it in Settings. Sola's own
`.desktop` files exist for default handlers (see [xdg-open](#xdg-open)).

## Tracking

`sola-session` keeps a record of every process it starts, filed under the
launch's app id; several copies of one application have several records.
At least every 500 ms it checks each one. When a process has exited, it
drops the record and emits `UserAppExited {app_id, command, code, signal}`.
Exactly one of `code` (a normal exit) and `signal` (killed by a signal) is
set. With a scope, this is the exit status of `systemd-run`, which passes
on the application's.

The shell shows `<app_id> exited (code N)` for a non-zero exit and
`<app_id> killed (signal N)` for a signal. A clean exit shows nothing.

A process started any other way is not tracked: from a terminal, by
`solactl open`, by another application, or by an earlier run of
`sola-session`. It has no scope, no exit fact, and no place in session
restore, and `solactl session close` does not find it. Its windows are
ordinary windows, and Super+Q still closes them through `sola-river`.

## Closing

`CloseApp` with an app id on the bus, or the `close` call, stops every
tracked process of that app id that is not already stopping:

- with a scope, `systemctl --user stop --no-block <unit>`: systemd sends
  SIGTERM to every process in the scope, and SIGKILL to any still running
  5 s later;
- without a scope, SIGTERM to the process.

```bash
solactl session close sola-mail
```

A process that is stopping stays tracked until it exits, and then
`UserAppExited` follows. Closing it again in the meantime does nothing.
The call replies `{app_id, stopped}` with the number of processes stopped,
or fails with `no live children for <app_id>` when nothing tracked matches.

Super+Q sends `CloseApp` for the focused application, which reaches three
receivers at once: `sola-session` stops the scope, `sola-river` asks every
window with that app id to close ([Compositor § Closing](compositor.md#closing)),
and a kit application quits by itself. `solactl session close` does only
the first.

## Ending the session

On `Shutdown` (**Quit Sola**, or the process manager ending the session),
`sola-session` asks systemd to stop every scope, waits up to 6 s for the
processes to exit, sends SIGKILL to any that remain and stops their scopes
again, then exits. The stop requests reach systemd before the wait begins,
so they complete even if `sola-session` is ended first.

## Restoring the last session

`sola-session` publishes the applications it is running as `SessionApps`,
a persistent, sorted list of `{app_id, command}` with one entry per app id,
stored in `~/.config/sola/state.yaml`. It publishes the list again after
every launch, close, and exit. An application you close, or one that quits
or crashes, leaves the list. Ending the session does not change it, so the
applications open at **Quit Sola** are the ones restored.

When it starts, `sola-session` spends 750 ms collecting the stored
`SessionApps` and the current `Windows`. Then it launches every listed
application that has no window with that app id and is not already
tracked, once, with its stored command. A stored entry for an application
that has since been renamed launches under the new name (`sola-workspaces`
runs as `sola-projects`). After a fresh start everything on the list
opens. After a restart of `sola-session` alone, the applications
are still open and nothing is launched.

Restore does not place windows: the shell puts each window back in its
saved zone or floating rectangle when it appears
([Compositor § Floating and zones](compositor.md#floating-and-zones)).
What an application reopens inside its window (tabs, documents) is up to
the application.

## One instance per application

`sola-session` has no notion of a single instance: every launch starts a
new process. Applications that keep one instance do it themselves:
Browser (with its web apps), Paint, Arcade, Design, Calendar, Terminal, and
Projects. A second process hands its request (a URL, an image, a file, a
new tab, or a request to come forward) to the running one and exits.
`sola-session` reaps it like any other exit. A second Projects shows the
running one's current project.

The launcher does not start a second copy of an application hidden with
Super+H: choosing it shows and raises the hidden one
([Shell § Hide](sola-shell.md#hide)). For a visible application without
its own single-instance handling, the launcher starts another copy.

## Opening URLs and files

Opening a URL or an image does not go through `sola-session`. Every Sola
component that opens one uses the same two functions in `sola-core`, and
they start the target application directly.

### URLs

`sola_core::open_url`:

1. Turns a local path into a `file://` URL: an absolute path always, a
   relative path when the file exists or the path starts with `./` or
   `../`.
2. If the Browser is running, writes the URL to its socket,
   `~/.local/share/sola/browser/chrome.sock` (under `$XDG_DATA_HOME` when
   that is set). The Browser opens it in a new tab.
3. Otherwise starts `/opt/sola/bin/sola-browser <url>` in the background,
   or the binary that `SOLA_BROWSER` names.

There is no fallback to another browser. If the binary is missing, the
open fails.

Applications that want the shell involved emit `OpenUrl {url, activate}`
on the bus instead. A running Browser opens the tab itself; with
`activate`, the shell also raises the Browser, showing it if it was
hidden. When the Browser is not running, the shell calls `open_url`.

### Images

`sola_core::open_image` treats a path, or a `file://` URL, ending in
`.png`, `.jpg`, `.jpeg`, `.jpe`, `.gif`, `.webp`, `.bmp`, `.tif`,
`.tiff`, or `.tga` (any case) as an image. It starts
`/opt/sola/bin/sola-paint <path>`, or the binary that `SOLA_PAINT` names. A
running Paint receives the path from the new process and opens it
([Paint](sola-paint.md)).

### solactl open

`solactl open <target>` sends an image to Paint and anything else to
`open_url`. It exits with status 3 when the open fails.

```bash
solactl open https://example.com
solactl open ~/Pictures/shot.png
```

### xdg-open

`cargo make install` copies each crate's `dist/` tree into
`~/.local/share` (`$XDG_DATA_HOME`), runs `update-desktop-database`, and
makes every installed `sola-*.desktop` file the default handler for the
MIME types it lists (`xdg-mime default`). Other applications' handlers are
left alone. The NixOS module installs `xdg-utils` and `desktop-file-utils`,
which this needs.

| Desktop file | Runs | Default for |
|---|---|---|
| `sola-browser.desktop` | `/opt/sola/bin/sola-browser %u` | `http`, `https`, `about:`, and unknown URL schemes; HTML, XHTML, PDF |
| `sola-paint.desktop` | `/opt/sola/bin/sola-paint %F` | PNG, JPEG, GIF, WebP, BMP, TIFF, TGA |
| `sola-spotify.desktop` | `/opt/sola/bin/sola-spotify` | Nothing; it names Spotify for media controls |

So `xdg-open` from a terminal, or a link clicked in another program,
reaches the Browser or Paint in the same way: a new process that hands the
URL or file to the running one.

## Bus topics and call methods

[The bus](bus.md) documents the encoding and the persistent rules.

| Topic | Direction | Payload |
|---|---|---|
| `LaunchApp` | Read | `{app_id, command}` |
| `CloseApp` | Read (also by `sola-river` and kit applications) | An app id |
| `Shutdown` | Read | None |
| `SessionApps` | Read at start; published | List of `{app_id, command}`, persistent |
| `Windows` | Read at start | See [Compositor](compositor.md#bus-topics) |
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
| sola-session | Open applications keep running, but the new `sola-session` does not know them: no `UserAppExited`, `solactl session close` does not find them, and they leave `SessionApps` at the next launch, close, or exit. Super+Q still closes their windows. Restore launches only listed applications that have no window. |
| sola-bus | `sola-session` reconnects and subscribes again. A `LaunchApp` sent while the bus was down is lost. |
| River | The session ends ([Compositor § Restarts](compositor.md#restarts)). |

## Logs

| File | Contents |
|---|---|
| `/opt/sola/log/sola.log` | `sola-session`'s log, lines tagged `[session]`: `launch`, `user app launched` with the unit and pid, `CloseApp: stopping`, `user app exited`, and what restore decided |
| `/opt/sola/log/app-<app_id>.log` | Each application's standard output and standard error |

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
- Applications opened through `open_url`, `open_image`, a terminal, or
  another program are not tracked and are not restored.
- Restore opens one copy of each application with its launch command. It
  does not restore a second window, or a copy launched with a different
  command.
- There is no autostart list besides session restore.
- Without a systemd user manager, closing sends SIGTERM to the main
  process only, not to the processes it started.
