# solactl

`solactl` is the operator command line. Most of its commands are calls on
the call plane, `sola-call` ([The call plane](call.md)); `emit` writes to
the bus; `logs`, `open`, and `media` work locally. It is installed as
`/opt/sola/bin/solactl`.

## Synopsis

```text
solactl compositor screenshot [-o PATH] [-a APP] [-w TITLE] [--format png|rgba] [-t SECS]
solactl compositor sample [-s N] [-t SECS]
solactl compositor windows
solactl compositor input click X Y [-b left|right|middle] [-t SECS]
solactl compositor input move X Y [-t SECS]
solactl compositor input scroll [-x DX] [-y DY] [-t SECS]
solactl compositor input key CHORD [-t SECS]
solactl compositor input press [X Y] [-b left|right|middle] [-t SECS]
solactl compositor input release [X Y] [-b left|right|middle] [-t SECS]
solactl compositor input drag X1 Y1 X2 Y2 [-b …] [-s STEPS] [-t SECS]
solactl session launch APP_ID [-c COMMAND] [-t SECS]
solactl session close APP_ID [-t SECS]
solactl projects [METHOD [ARGS…]]
solactl browser [METHOD [ARGS…]]
solactl bots [METHOD [ARGS…]]
solactl design [METHOD [ARGS…]]
solactl OWNER [METHOD [ARGS…]]       # any other live owner, by owner or app id
solactl mcp [--owner OWNER]…
solactl logs [APP] [-f]
solactl emit KIND PAYLOAD
solactl open TARGET
solactl media play-pause|next|prev|mute|vol-up|vol-down
solactl help [COMMAND…]
```

| Command | Goes through | Needs |
|---|---|---|
| `compositor`, `session` | Call plane; flags compiled into `solactl` | `sola-call` and the owner; `--help` needs nothing |
| `projects`, `browser`, `bots`, `design`, any other name | Call plane; flags read from the owner's advertised methods | `sola-call` and the owner, also for help |
| `mcp` | Call plane | `sola-call`; tools appear as owners do |
| `emit` | Bus | `sola-bus` |
| `logs` | Files in `/opt/sola/log` | nothing |
| `open` | Hands off to, or starts, the Browser or Paint | their binaries |
| `media` | Session D-Bus (MPRIS) and `wpctl` | a player, PipeWire |

`solactl` never starts an application to answer a call. There is no
`--version` and no `eval` command.

## Output and exit status

A successful call prints the reply's `data` to stdout as indented JSON,
and prints nothing when the reply has no data. Method lists and help are
plain text. Errors go to stderr, prefixed `solactl: `. Writing into a
closed pipe (`solactl browser tabs | head`) is not an error.

| Status | Meaning |
|---|---|
| 0 | Success; also `media` with no player to act on |
| 1 | The owner or the host answered with an error. This includes `timeout`, and `<owner> is not running` for `compositor` and `session` |
| 2 | Usage error in a compiled command (`compositor`, `session`, `mcp`, `logs`, `emit`, `open`, `media`): a missing or bad argument or an unknown subcommand; usage is printed |
| 3 | Local failure: `sola-call` unreachable, a live owner not in the catalog, an unknown method, a flag error, a `SOLA_BOT=1` refusal, a bad chord, or a failed `logs`, `emit`, `open`, or `media` |

### When the call host or an owner is down

| Situation | stderr | Status |
|---|---|---|
| `sola-call` not running | `solactl: call host is not running (<socket path>)` | 3 |
| `compositor` or `session` owner not connected | `solactl: compositor is not running` | 1 |
| Live owner not in the catalog (also for its method list and `--help`) | `solactl: <name> is not running` | 3 |
| Method not advertised | `solactl: <owner> has no method <method>` | 3 |
| Owner did not answer by the deadline, or exited during the call | `solactl: timeout` | 1 |
| `sola-call` restarted during the call | `solactl: call host closed the connection` | 3 |

Any command name `solactl` does not know is taken as an owner name, so a
misspelt command reports `<name> is not running`.

## Environment

| Variable | Read by | Effect |
|---|---|---|
| `SOLA_CALL_PATH` | call commands, `mcp` | Call socket. Default `$XDG_RUNTIME_DIR/sola-call` |
| `SOLA_BUS_PATH` | `emit` | Bus socket. Default `$XDG_RUNTIME_DIR/sola-bus` |
| `XDG_RUNTIME_DIR` | call commands, `mcp`, `emit` | Directory of the default sockets; `/tmp` when unset |
| `SOLA_BOT` | `compositor`, `browser`, `mcp` | `1`, `true`, `TRUE`, or `yes` turns on the [bot rules](#bot-rules) |
| `SOLA_PANE_ID` | `projects whoami`, `projects tab.spawn` | Default `--pane` and `--parent`. Projects sets it in each pane |
| `SOLA_WS_PATH` | `projects whoami` | Default `--path`. Projects sets it in each pane to the tab's checkout path |
| `HOME` | path arguments, `open` | `~` expansion; default location of `chrome.sock` |
| `XDG_DATA_HOME` | `open` | The Browser's handoff socket is `$XDG_DATA_HOME/sola/browser/chrome.sock` (default `~/.local/share/…`) |
| `SOLA_BROWSER` | `open` | Browser binary to start. Default `/opt/sola/bin/sola-browser` |
| `SOLA_PAINT` | `open` | Paint binary to start. Default `/opt/sola/bin/sola-paint` |
| `DBUS_SESSION_BUS_ADDRESS` | `media` | Session D-Bus used to find MPRIS players |
| `PATH` | `media` | Finds `wpctl` |

The working directory matters too: relative path arguments resolve
against it, and `design canvas.new` makes its canvas in it by default.

## Live-owner commands

`solactl projects`, `browser`, `bots`, and `design`, and `solactl NAME`
for any other owner, take their methods and flags from what the running
owner advertises ([The call plane](call.md#methods-and-arguments)). The
sections below list each owner's methods as they are advertised today.

### Listing and help

```text
solactl OWNER                  # the owner's methods, one line each
solactl OWNER --help           # the same (-h too)
solactl OWNER METHOD --help    # the method's flags
```

`OWNER` may be the owner or its app id (`solactl sola-design status`). A
method's help lists each flag as `-s, --long <value>`, then its help text
and `(required)` or `(optional)`; bool flags have no `<value>`. `-h` or
`--help` anywhere after the method prints that help instead of calling.

### Arguments

- `--name VALUE` or `-s VALUE` sets an argument. When a flag is given
  twice, the last one wins.
- A bool flag alone is `true`. `--name true`, `false`, `1`, or `0` sets it
  explicitly. Bool flags may come before other flags.
- A value that starts with `-` is refused (`--name needs a value`), except
  a negative number.
- A bare word fills the next required argument not yet given, in the
  advertised order: `solactl design open design/home`,
  `solactl bots new Suno`. A bare word with no required argument left is
  an error (`unexpected argument`).
- A single JSON object as the only argument is sent as the params
  unchanged, without conversion or path resolution:
  `solactl browser tab.open '{"url":"https://example.com"}'`.
- Values are converted by the argument's type: `int` and `float` are
  parsed (a value that does not parse is sent as a string and the owner
  answers with its error), `json` is parsed (else sent as a string), and
  `path` is made absolute (below).
- An unknown `--flag` is sent as a string argument. An unknown `-x` is an
  error (`unknown flag -x`).
- `solactl` does not check required arguments; the owner answers with its
  error.

### Paths

Arguments of type `path` are resolved by `solactl` before the call: `~`
and `~/…` expand, and a relative path is taken from **your** working
directory, not the owner's. They are `projects` `--path` and
`--prompt-file`; `browser` `--file` and screenshot `--path`; and `design`
`--path`, `--canvas`, and `--out`. Other arguments are sent as typed.

### Deadline

- When the method has a `--timeout` argument (seconds) and you pass it,
  the deadline is that plus 2 s, and at least 8 s.
- Otherwise it is the method's advertised deadline, rounded up to whole
  seconds (listed per owner below).
- Otherwise it is 8 s.

### Context

- `projects whoami` without `--pane` or `--path` sends `$SOLA_PANE_ID`
  and `$SOLA_WS_PATH`.
- `projects tab.spawn` without `--parent` sends `$SOLA_PANE_ID`.
- `design canvas.new` without `--path` sends your working directory.

### Bot rules

With `SOLA_BOT=1` (named informational bots), `solactl` refuses to take
the human's seat. A refusal exits 3 with a message naming the rule.

- `browser tab.focus` is refused, and `--select` on any `browser` method.
- `browser` page methods need `--tab`; they never default to the focused
  tab: `snapshot`, `click`, `hover`, `type`, `fill`, `get`, `links`,
  `select`, `wait`, `screenshot`, `goto`, `back`, `forward`, `reload`,
  `stop`, `find.page`, `find`, `key`, `scroll`, `tab.close`.
- `compositor screenshot`, `sample`, and `input` are refused;
  `compositor windows` is allowed.

The web path for a bot is `solactl browser` only.

## Compositor (`solactl compositor`)

Owner `compositor`, process `sola-river`, present for the whole session
([Compositor](compositor.md)). The flags are compiled into `solactl`.

### screenshot

| Flag | Default | Meaning |
|---|---|---|
| `-o`, `--output PATH` | `/tmp/sola/screenshots/<ms>.png` | Where to write. Sent as given: a relative path is taken from `sola-river`'s working directory, so pass an absolute one. Parent directories are created |
| `-a`, `--app APP` | the whole output | Capture this app's window (exact app id) |
| `-w`, `--window TITLE` | the app's first window | With `--app`, the window whose title is exactly `TITLE`; ignored without `--app` |
| `--format png\|rgba` | `png` | `rgba` (or `rgba8`) writes packed RGBA8 with no PNG encode; its default path is `/dev/shm/sola-freeze-<ms>.rgba` |
| `-t`, `--timeout SECS` | 10 | Deadline |

`--app` copies that window's own buffer (`ext-image-copy-capture`). The
window does not need to be on top and is not raised. The default PNG uses
fast compression. The reply is `{"path": …}`; with `rgba` it adds `width`,
`height`, and `"format": "rgba8"`. Errors: `screenshot already in
progress`, `window not found for capture: app_id=… title=…`, and `unknown
screenshot format …`.

The Shell's screenshot chords copy to the clipboard instead of writing
this file (see [Shell § Screenshots](sola-shell.md#screenshots)). The
method also takes a region (`x`, `y`, `width`, `height`, all four) that
has no flags here; over MCP, `compositor_screenshot` accepts it.

### sample

`-s`, `--size N` (default 15) is the side of the square patch around the
pointer: odd, at most 65 (an even size rounds up; 0 or less means 15).
`-t`, `--timeout SECS` defaults to 2. The reply has `x`, `y` (the
pointer), `left`, `top` (the patch's origin), `width`, `height`, `hot_x`,
`hot_y` (the pointer within the patch), and `pixels` (base64 RGBA8).
Errors: `no pointer position yet`, `sample already in progress`. Scope
uses this method.

### windows

No flags; the deadline is 5 s. The reply is an object keyed by app id;
each value is an array of `{"title", "window_id", "pid"}`.

### input

| Command | Arguments | Defaults |
|---|---|---|
| `input click X Y` | `-b`, `--button left\|right\|middle` (also `l`, `r`, `m`) | `left` |
| `input move X Y` | none | |
| `input scroll` | `-x`, `--dx DX`; `-y`, `--dy DY` (decimal; down is positive) | `0`, `5` |
| `input key CHORD` | none | |
| `input press [X Y]` | `-b`, `--button` as for `click` | `left` |
| `input release [X Y]` | `-b`, `--button` as for `click` | `left` |
| `input drag X1 Y1 X2 Y2` | `-b`, `--button`; `-s`, `--steps N` (moves between press and release) | `left`, `12` |

Each takes `-t`, `--timeout SECS` (default 5) and replies `{"ok": true}`
(`drag` prints nothing). `X` and `Y` are integer output coordinates. The
events go through the seat like real input, to whatever is under the
pointer or focused. `press` holds the button until a `release`; with
`X Y` either moves there first. `drag` is a press at `X1 Y1`, `STEPS`
moves about 16 ms apart, and a release at `X2 Y2`, so it is seen as a drag
(a tab bar row, a sidebar reorder) rather than a click.

`CHORD` is `MODIFIER+…+KEY`, in any case. Modifiers: `Meta` (or `Super`,
`Win`, `Cmd`), `Alt` (or `Option`), `Ctrl` (or `Control`), `Shift`. Keys:
`Tab`, `Esc` or `Escape`, `Enter` or `Return`, `Backspace` or `BS`,
`Space`, `Left`, `Right`, `A`–`Z`, `0`–`9`, `` ` `` (or `Grave`,
`Backtick`), and the keypad: `KP0`, `KP2`, `KP4`, `KP5`, `KP6`, `KP8`,
`KPEnter`, `KPMultiply` (or `KP*`), `KPAdd`, `KPEqual` (or `KP=`),
`KPDecimal` (or `KP.`). Any other key is refused (`unknown key '…'`,
status 3).

### Methods without a command

`cursor` (`--visible true|false`, default `true`; deadline 2 s) shows or
hides the compositor's pointer and replies `{"visible": …}`. It has no
`solactl compositor` command; over MCP it is `compositor_cursor`.

Over MCP, `compositor_input_key` expects `chord` as an object (`keycode`,
an XKB key code, and `meta`, `alt`, `ctrl`, `shift` booleans), which
`solactl compositor input key` builds from the text form. A text chord
sent to it is refused with `chord: invalid type …`.

## Session (`solactl session`)

Owner `session`, process `sola-session`, present for the whole session
([Session manager](session.md)). The flags are compiled into `solactl`.

| Command | Flags | Reply |
|---|---|---|
| `launch APP_ID` | `-c`, `--command COMMAND`; `-t`, `--timeout SECS` (5) | `{"app_id", "command", "pid"}` |
| `close APP_ID` | `-t`, `--timeout SECS` (5) | `{"app_id", "stopped": N}` |

`launch` runs `COMMAND`, split on whitespace with no shell quoting. The
default is `/opt/sola/bin/<APP_ID>` when that file exists, else `APP_ID`
itself. Errors: `empty command`, or the spawn error. `close` stops the
processes the session launched for `APP_ID`; with none it fails with `no
live children for <APP_ID>`.

## Bots (`solactl bots`)

A bot is a named informational agent session with its own home directory.
`solactl bots` needs the bots daemon, `sola-botsd` (owner `bots`), which
the process manager keeps running. It fails if the daemon or `sola-call`
is down; it does not launch a window.

```text
solactl bots                         # list methods
solactl bots list
solactl bots poll [-b BOT]
solactl bots new -n NAME             # or: solactl bots new NAME
solactl bots send -b BOT -t TEXT
solactl bots transcript -b BOT
solactl bots rm -b BOT
solactl bots cancel -b BOT
```

`-b`, `--bot` is a bot's id, slug, or name; `-n`, `--name` is a display
name; `-t`, `--text` is the message. Every method has the default 8 s
deadline.

| Method | Does |
|---|---|
| `list` | Bots and their status |
| `poll` | `{"bots", "transcript"}` in one call; `transcript` is `null` without `--bot` |
| `new` | Creates a bot and its home |
| `send` | Sends a message and returns while the turn runs; read the answer with `transcript` or `poll` |
| `transcript` | The dialog turns |
| `rm` | Deletes the catalog row, the home directory, and the Grok session |
| `cancel` | Cancels the current turn |

### Homes and the write fence

Each bot's home is `~/Bots/<slug>/`. `new` seeds the home and starts a
first turn, an introduction (the orientation prompt is not shown as a user
message).

The write fence is enforced by the prompt only. By default a bot writes
only inside its home. To open the fence for one task, say “drop the write
fence” or “work on the whole computer”, or name a path outside the home.
Seat steal is refused even then. A bot is not a coding agent, and it does
not use Projects unless you ask for that rail.

### Phone API

The daemon also serves HTTP on `0.0.0.0:27419`, in every session (see
[Process manager](process-manager.md#managed-components)). For a phone,
put a TLS proxy in front of it and open **27419/tcp** on the Sola machine
to that proxy only.

```text
GET  /health
GET  /bots
GET  /events                 # SSE: snapshot, bots, transcript, delta, removed
GET  /poll?bot={id}          # one-shot snapshot (debug); the phone uses /events
GET  /bots/{id}/transcript
POST /bots                 { "name": "Suno" }
DELETE /bots/{id}
POST /bots/{id}/send     { "text": "…" }
POST /bots/{id}/cancel
Authorization: Bearer <shared secret compiled into sola-botsd + SolaBot>
```

`GET /events` is `text/event-stream` (chunked), with keepalive comments
every 15 s. The **Bots** window uses the same HTTP and SSE surface on
`127.0.0.1:27419` (list, send, create, delete, and live events).
`solactl bots` uses `sola-call`, not HTTP.

The TLS proxy must not buffer the response body (`proxy_buffering off`,
and honor `X-Accel-Buffering: no`). It should speak HTTP/1.1 or later to
the daemon, with a long `proxy_read_timeout`.

## Projects (`solactl projects`)

Needs **Projects** running (owner `projects`, app id `sola-projects`).
Fails if the app or `sola-call` is down; it does not launch a window. A
tab is one checkout in a project: the project root, a `.worktrees/`
worktree, or a folder. Each project is its own window (a tab in the
shell's tab bar, named for the project), with its tabs along the top.
That row in the shell's bar shows a project's activity in place of its
icon: an amber diamond when any of its tabs is waiting on a decision, a
spinning ring when any is working (the diamond wins), the icon otherwise.
The app was called Workspaces, and a tab a workspace, until 2026-10-21;
`solactl workspaces` and the `workspace.*` methods are gone, with no
alias.

```text
solactl projects                           # list methods
solactl projects ps
solactl projects project.list
solactl projects project.add --path ~/Workspace/Sola
solactl projects project.startup [--project Illuno]
solactl projects project.startup --project Illuno --script 'cp -a "$PROJECT/.grok" "$WORKTREE/"'
solactl projects project.rm --project Sola
solactl projects project.reorder --project Sola [--before Illuno]
solactl projects tab.list [--project Sola]
solactl projects tab.spawn --project Sola --name ticket-123 \
    [--branch joshua/sc-1234/fix] [--base-branch origin/dev] [--title 'fix login'] \
    [--agent grok|codex|claude] [--prompt '…' | --prompt-file FILE] [--parent …] [--select]
solactl projects tab.spawn --project Sola --name hotfix --root   # no worktree
solactl projects tab.set --tab ticket-123 --title 'fix login'
solactl projects tab.set --tab adhoc --name sc-1234 \
    [--title 'fix login'] [--branch joshua/sc-1234/fix] [--before root]
solactl projects tab.exec --tab ticket-123 [--agent grok|codex|claude] \
    [--prompt '…' | --prompt-file FILE]
solactl projects tab.select --tab ticket-123
solactl projects tab.rm --tab ticket-123 [--worktree] [--force]
solactl projects pane.list [--tab ticket-123]
solactl projects pane.send --text 'follow up' --enter [--pane ticket-123]
solactl projects pane.read [--pane ticket-123] [--lines 40]
solactl projects pane.wait [--pane ticket-123] [--status done] [--timeout 300] [--fresh]
solactl projects whoami                    # from a Projects pane; or --pane / --path
```

| Short | Long | Methods |
|---|---|---|
| `-p` | `--project` | `project.rm`, `project.startup`, `project.reorder`, `tab.list`, `tab.spawn` |
| `-p` | `--path` | `project.add` |
| `-n` | `--name` | `tab.spawn`, `tab.set` |
| `-b` | `--branch` | `tab.spawn`, `tab.set` |
| `-a` | `--agent` | `tab.spawn`, `tab.exec` |
| `-s` | `--select` | `tab.spawn` |
| `-t` | `--tab` | `tab.rm`, `tab.select`, `tab.set`, `tab.exec`, `pane.list` |
| `-f` | `--force` | `tab.rm` |
| `-t` | `--text` | `pane.send` |
| `-e` | `--enter` | `pane.send` |
| `-l` | `--lines` | `pane.read` |
| `-s` | `--status` | `pane.wait` |
| `-f` | `--fresh` | `pane.wait` |

Required arguments: `project.add --path`; `project.rm`, `project.reorder`
`--project`; `tab.spawn --project --name`; `--tab` on `tab.rm`,
`tab.select`, `tab.set`, `tab.exec`; `pane.send --text`. `--project` on
`project.startup` and `--tab` on `pane.list` default to the focused
project window's selected tab; `--pane` on `pane.send`, `pane.read`, and `pane.wait`
defaults to the focused pane, else the selected tab.

Deadlines: `project.add` 15 s; `tab.spawn` and `tab.exec`
60 s; `pane.wait` 302 s, or its `--timeout` plus 2 s; the rest 8 s.

### Spawn

`--name` is the `.worktrees/<name>` folder and the default tab label.
`--branch` defaults to that name; `--base-branch` defaults to `HEAD`.
`--title` is the tab label. An empty `--title`, or a title equal to
`--name`, falls back to the folder slug; the slug is not prefixed onto a
custom title.

Spawn runs in the background: the new tab appears in its project
window's top bar, and the windows stay on what they show. `--select`
shows the project's window and selects the new tab (the same as **+** or
⌘T in the UI). `tab.exec` does not select. When you spawn from a Projects
pane, the parent defaults to `$SOLA_PANE_ID`. `--parent` takes a parent
tab, pane, or path.

`--agent` is `grok`, `codex`, or `claude`. Other names are an error
(OpenCode is presence-only). `--prompt` without `--agent` implies Grok.
`--prompt` and `--prompt-file` are exclusive. The first Codex session may
need `/hooks` in its TUI to trust the Sola status hook. Claude hooks are
merged into `~/.claude/settings.json` and apply to the next Claude
session. On start, Projects finds its existing Codex and Claude hook
entries by script file name and rewrites them in place (status message
"Sola Projects status"). Its catalog, last status, and hook scripts live
in `~/.config/sola/projects/`, moved there once from
`~/.config/sola/workspaces/`.

### Root tabs

`--root` opens the tab in the project root checkout instead of a new
worktree (the **No worktree** box on the New tab card). Its `kind`
is `root`, its `path` is the project root, and work lands on whatever
branch the root has checked out (usually master). `--branch` and
`--base-branch` are refused with `--root`, and the startup script does not
run.

A root tab closes with plain `tab.rm` (`--worktree` is refused).
`tab.set` takes `--title` and `--before` on a root tab; `--name` and
`--branch` need a worktree. Hover × and the pencil work as on a worktree
tab.

### Rename and reorder

`tab.set --name` runs `git worktree move` to `.worktrees/<name>`.
The id stays, so tmux sessions keep working; live or dirty checkouts are
moved with force. The project root cannot be renamed. `--branch` runs
`git branch -m` in that checkout and does not move the folder. To promote
an ad hoc tab, pass both:
`--name sc-1234 --branch joshua/sc-1234/fix --title '…'`.

Target a tab by id, slug, path, or label. `--before` reorders that tab
within its project's top bar (`end` = last). On a worktree tab, a
double-click on its chip sets `--title` (the label, not the folder).
`project.reorder` moves a project in the project order (the order windows
open in): `--before` another project, or omit it (or pass `end`) for
last. `tab.select` shows the tab's project window (opening it if it was
closed) and selects the tab.

### Lists, startup scripts, and sessions

`tab.list` replies `{"tabs": […]}`; `ps` nests each project's tabs under
`projects[].tabs[]`; `whoami` names the tab as `tab` (id) and `tab_name`;
`project.add` and `tab.exec` replies carry `tab`. Lists include `path`,
`kind`, and `parent`. `kind` is one of:

- `main` — the project's own tab
- `worktree`
- `root` — an extra tab in the project root
- `folder` — a non-git project

`project.startup` is the per-project script that runs in a new worktree
after spawn (also **Project → Startup Script…**). Without `--script` it
reads the script; `--script ''` clears it. The script sees `$PROJECT` (the
project folder on disk), `$WORKTREE` (this tab, `.worktrees/<name>`), and
`$NAME` (the tab name).

`pane.list` and `whoami` include `session_id` when that pane’s Grok or
Claude owner session is known. Use it with `grok -r` or `claude --resume`
after a reboot loses tmux.

### Panes

A tab name prefers the Grok leaf when sending, reading, waiting, or
exec-ing (the Codex leaf with `--agent codex`, the Claude leaf with
`--agent claude`). `pane.send`, `pane.read`, and `tab.exec --prompt`
address that tab’s tmux session exactly (`=session:` current pane),
so a split leaf `…-p` is not confused with its parent.

`pane.send` and `tab.exec --prompt` **paste** into the live agent
(tmux bracketed paste, then Enter), so a multiline brief does not submit
on the first newline and a long prompt is not truncated. A failed paste
returns the tmux error, not a bare `send failed`.

`pane.read` replies `{"text", "pane"}` with the whole scrollback, or the
last `--lines N` lines.

`pane.wait` holds until the pane's status is `--status` (`working`,
`waiting`, `done`, or `idle`; default `done`), for up to `--timeout`
seconds (default 300). `--fresh` waits for a transition onto that status.

### Remove

`tab.rm` unregisters the tab. It does not run `git worktree remove`
unless you pass `--worktree`; add `--force` to discard a dirty checkout.

`tab.rm` replies first, then drops the tab on the next tick, so a
call from inside that pane can finish instead of hanging.
`tmux kill-session` and `git worktree remove` then run off the UI thread,
and the rest of Projects stays live.

Do **not** run `git worktree remove` yourself from inside that pane: the
working directory vanishes and the next tool cannot run. Use `--worktree`
instead. If the checkout is already gone, Projects reaps the tab, with no
working spinner left behind.

## Browser (`solactl browser`)

Needs the **Browser** (`sola-browser`) running (owner `browser`). Fails if
the Browser or `sola-call` is down; it does not launch a window. Each tab
is a page with its own window, a row in the shell's tab bar, the same
pages the human uses.

```text
solactl browser                         # list methods
solactl browser tabs
solactl browser tab.open --url https://example.com [--select]
solactl browser tab.focus --tab 3
solactl browser tab.close [--tab 3]
solactl browser goto --url https://example.com/path [--tab 3] [--select]
solactl browser back|forward|stop [--tab 3]
solactl browser reload [--tab 3] [--hard]
solactl browser find.page --text needle [--tab 3] [--back]
solactl browser snapshot [--tab 3] [--interactive] [--ref e8] [--json]
solactl browser find --text Submit [--tab 3]
solactl browser click --ref e12
solactl browser click --tab 3 --text "Pay now"
solactl browser click --tab 3 --x 120 --y 80
solactl browser hover --ref e12 | --text NAME | --x 120 --y 80 [--tab 3]
solactl browser type --ref e5 --text more [--submit] [--tab 3]
solactl browser fill --ref e5 --text user@example.com
solactl browser fill --tab 3 --control lyrics --file /path/lyrics.txt
solactl browser get --tab 3 --control lyrics
solactl browser links [--tab 3]
solactl browser key --tab 3 --chord Control+Enter
solactl browser scroll --tab 3 --dy 400 [--dx 0] [--x 100] [--y 100]
solactl browser select --ref e9 --values OptionA,OptionB
solactl browser wait [--tab 3] [--load] [--text done] [--control Create] [--timeout 30]
solactl browser screenshot [--tab 3] [-o PATH]
```

| Short | Long | Methods |
|---|---|---|
| `-u` | `--url` | `tab.open`, `goto` |
| `-s` | `--select` | `tab.open`, `goto` |
| `-t` | `--tab` | every method that takes a tab |
| `-c` | `--chord` | `key` |
| `-q` | `--text` | `find.page`, `find`, `wait` |
| `-x` | `--text` | `type`, `fill`, `get` |
| `-b` | `--back` | `find.page` |
| `-i` | `--interactive` | `snapshot` |
| `-j` | `--json` | `snapshot` |
| `-r` | `--ref` | `click`, `hover`, `type`, `fill`, `get`, `select` |
| `-e` | `--submit` | `type` |
| `-f` | `--file` | `fill` |
| `-v` | `--values` | `select` |
| `-l` | `--load` | `wait` |
| `-o` | `--path` | `screenshot` |

`reload --hard` has no usable short flag: `solactl` reads `-h` as help.

`--tab` is an id, or a unique URL or title substring. Without it, page
methods use the focused tab, except `click`, `hover`, `type`, `fill`,
`get`, and `select`, which use the tab of the last snapshot. `tab.open`
opens the page in its own window behind the shown one in the focused zone,
and does **not** focus it unless you pass `--select`; `goto` does not focus
either. `tab.focus` and `--select` show the page's window and focus it.
`tab.close` never closes the last tab.

Deadlines: `snapshot`, `goto`, `click`, `hover`, `key`, `scroll`, `type`,
`fill`, `get`, `links`, `select`, and `screenshot` 15 s; `wait` 32 s, or
its `--timeout` plus 2 s; the rest 8 s.

### Background tabs

Snapshot, click, fill, type, and `browser screenshot` target **that tab’s
CEF document**. They work when the tab is in the background and do not
raise it.

### Snapshots and refs

Page verbs use a pruned accessibility snapshot in YAML, with opaque refs
(`e12`, or `f1e3` inside a frame). The snapshot starts with an `actions:`
list (role, name, ref); click those. `--interactive` keeps controls only,
`--ref` returns one subtree of the last snapshot, and `--json` returns
debug JSON instead of YAML. `click --text "Pay now"` uses the last
snapshot and fails with the candidate refs when more than one control
matches. A stale ref fails (the node is gone); take a new snapshot.

`find` searches the last snapshot, not the page. `find.page` is ⌘F
(`--back` searches backwards).

### Input

`click` and `hover` also take CSS-pixel `--x` `--y` in **that tab’s**
viewport. That needs no ref, and it is still CEF `Input.dispatch*`, not the
compositor seat.

`fill` and `type` write a contenteditable or ProseMirror editor with one
`paste` of the whole string (newlines kept); they do not assign
`textContent`. `type` appends; `fill` replaces, and takes the control by
`--ref` or by its actions-list name with `--control`. `fill --file PATH`
reads the string from disk (exclusive with `--text`). The reply includes
`value` and `matched: true`, or an error if the field did not keep the
text. `type --submit` presses Enter after.

`key --chord` (for example `Return`, `Escape`, `Control+Enter`) inserts
once: one `char` event, not keyDown-with-text plus char. `scroll --dx --dy`
(CSS pixels) go to the same tab, from a wheel origin at `--x` `--y`
(default 100, 100).

`select --values` takes comma-separated option labels or values.

### Reading the page

`get` reads the current text, value, or href of a ref or a `--control`
name (from the DOM aria-label if the accessibility tree misses it);
`--text` is the same as `--control`. `links` lists `<a href>` targets and
audio/video `duration` from the DOM.

Screenshot is a fallback when the tree is empty (canvas, maps). It
captures **that tab’s** document even when the tab is in the background,
writes `--path` (default `~/.cache/sola/browser/shot-<ms>.png`, under
`$XDG_CACHE_HOME` when set), and replies with `path`.

### Waiting

`wait` and `wait --load` return when `document.readyState` is `complete`
on a committed URL (not when the loading spinner stops). `wait --text`
snapshots until that string appears. `wait --control Create` snapshots
until that named control is in the actions list, or times out naming the
missing control; it does not succeed just because CEF fired load.
`--timeout` defaults to 30 s.

### Troubleshooting

If `tabs` or `tab.open` time out while the Browser window is still up, the
call plane is wedged. Close the window (× or the flower menu) and launch it
again. Do not kill the process from a bot.

Vault fill and confirm gates are not on this plane. Which methods need a
human confirm is an open question
([D3](../open-questions.md#d3--which-call-plane-methods-need-a-human-confirm-p1)).

## Design (`solactl design`)

Needs **Design** (`sola-design`) running (owner `design`). Fails if Design
or `sola-call` is down; it does not launch a window.

```text
solactl design                               # list methods
solactl design project.list                  # projects + canvases
solactl design project.add --path ~/Workspace/Thoxa
solactl design project.remove --path ~/Workspace/Thoxa  # forget it: canvases leave the rail and close; files stay
solactl design canvas.new onboarding         # design/onboarding/ in your cwd's project
solactl design canvas.new "Settings v2" --path ~/Workspace/Thoxa --raise
solactl design open design/onboarding        # relative to your cwd
solactl design open design/onboarding/canvas.toml --raise
solactl design canvas.close --canvas design/onboarding  # done with it: its boards' memory comes back; files stay
solactl design status [--canvas design/home] # canvas (shown, share), artboards (src, url, error, discarded, exits, board_links), annotations (sections, links, notes, flows), linked files, zoom, selection, memory
solactl design share [--canvas design/home]  # the canvas's live link
solactl design share.rotate [--canvas …]     # a new link; the old one stops working
solactl design selection
solactl design view --fit
solactl design view --artboard phone [--zoom 1]
solactl design view --section sign-in        # frame a [[section]]
solactl design view --zoom 0.5
solactl design wait [--artboard phone] [--timeout 15]
solactl design reload [--artboard phone] [--timeout 15]
solactl design screenshot [--artboard phone] [-o shot.png]
solactl design pdf [--artboard landing] [-o landing.pdf]
solactl design inspect [--artboard phone] [--selector 'main .card']
solactl design inspect --artboard light --selector section --all   # every match's box
```

| Short | Long | Methods |
|---|---|---|
| `-p` | `--path` | `project.add`, `project.remove`, `open`, `canvas.new` |
| `-n` | `--name` | `canvas.new` |
| `-r` | `--raise` | `open`, `canvas.new` |
| `-c` | `--canvas` | `canvas.close` and every page call |
| `-a` | `--artboard` | `view`, `wait`, `reload`, `screenshot`, `pdf`, `inspect` |
| `-f` | `--fit` | `view` |
| `-s` | `--section`, `--selector` | `view`, `inspect` |
| `-z` | `--zoom` | `view` |
| `-o` | `--out` | `screenshot`, `pdf` |
| `-A` | `--all` | `inspect` |

Required arguments: `--path` on `project.add`, `project.remove`, and
`open`; `canvas.new --name`; `canvas.close --canvas`. Deadlines: `wait`
and `reload` 32 s, or their `--timeout` (default 15) plus 2 s;
`screenshot`, `pdf`, and `inspect` 15 s; the rest 8 s.

### Canvases are files

A canvas is files, not app state: `<project>/design/<canvas>/canvas.toml`
lists the artboards, and each artboard is an HTML page beside it. Edit
those files with ordinary tools. Design watches them (and any outside file
a page loaded; `status` lists those as `linked`) and reloads.

After an edit, run `wait` or `reload` before `screenshot` or `inspect`.
Both make the boards show the files on disk (`wait` reloads when the files
are newer than the boards) and return once pages have loaded, web fonts are
in, and `auto` heights are measured.

### Opening a canvas

`canvas.new NAME` makes `<project>/design/<slug>/canvas.toml` (project =
`--path`, default your current directory), registers the project if
needed, and opens the canvas. It appears under the project in the rail. An
existing canvas of that name is opened instead (`created: false`).

`open` takes the canvas directory, its `canvas.toml`, or any file inside
it, and registers the project if Design does not know it yet.

`open` and `canvas.new` make a canvas live **off the human's screen**; it
shows in their rail. `--raise` puts it on their screen.

### Targeting a canvas and an artboard

Every page call (`status`, `share`, `share.rotate`, `view`, `wait`,
`reload`, `screenshot`, `pdf`, `inspect`) takes `--canvas`: the canvas
folder, its `canvas.toml`, or a file in it, opened in the background if
need be. Without it, the call goes to the canvas on the human's screen.
Over MCP, a page call without `canvas` goes to the canvas that agent last
opened or made, so agents working at the same time stay on their own.

`--artboard` defaults to the artboard holding the human's selection, else
the only artboard. `inspect` without `--selector` inspects the selected
element. `selection` is what the human Super+clicked (`null` when nothing).

### Annotations

Annotations are files too: `[[section]]`, `[[note]]`, `[[link]]`, and
`[[flow]]` in `canvas.toml`, and `data-design-to="<artboard id>"` exits in
the pages (see [Design § Files](sola-design.md#files)). Design draws
them over the canvas, so they are not in screenshots.

`status` lists them under `annotations`; each artboard's `exits` (with an
`error` when one leads nowhere) and `board_links` (artboards its plain
links lead to); and a note's `placed` or `error` once its artboard has
settled. `view --section` frames a section.

### Memory

Design holds its boards within the machine's free memory; it keeps 15%
free for everything else. When memory runs short, it **discards** idle
boards: their page closes, their picture stays. It reopens them on the next
call that needs one (`screenshot`, `pdf`, and `inspect` wait for the
reload; `wait` and `reload` reopen the boards they cover).

`status` marks those boards `discarded` and reports `memory`: `free_mib`,
`total_mib`, `keep_free_mib`, `boards_mib` (Design's estimate for its open
boards), and board counts.

When the boards in use leave no room, `open`, a call on a canvas that is
not open, or a call that reopens boards **fails with the reason**: the
numbers, each open canvas (boards, memory, why it is kept), and the way
out. Close a canvas you are done with: `canvas.close --canvas <dir>`
(`--canvas` is required; the canvas on the human's screen stays open). The
next call on it opens it again.

### Screenshots, PDFs, and inspect

`screenshot` captures the whole page (CDP, at the artboard's raster scale,
normally 2×) and replies with `path`. The result is the same however the
human is zoomed or panned. `screenshot` reports the `scale` it used: each
artboard renders at 2×, else 1×, else under 1× for a page over 8192 px.

`pdf` writes a one-page PDF of the artboard as it looks on screen (screen
media, backgrounds, no margins). Without `--out`, both write to
`$XDG_RUNTIME_DIR/sola/design/shots/<canvas>-<artboard>-<ms>.<ext>`.

`wait` and `reload` also wait for a `height = "auto"` artboard to be
measured.

`inspect` replies with the viewport (the artboard size), the document size,
`cut_off_below` (content taller than the artboard), the element's box and
computed styles, and up to 25 `issues` (`overflows-viewport`, `clipped-x`,
`clipped-y`). `--all` instead lists every match (up to 300) as
`{selector, tag, rect, text}`, with no styles; its default selector is
`body > *`.

In `status`, each artboard has `src` (what `canvas.toml` asks for), `url`
(what its tab shows), `error` (missing `src`, HTTP ≥ 400), and `detail`
(the part re-rendered at screen scale while the human is zoomed in, else
null).

`view` moves the human's camera only until they move it themselves on that
canvas; after that it replies `moved: false` and leaves it. On a canvas
that is not on screen, it frames the canvas for when the human opens it.

Boards are served over http from the project root (`/` = the project
folder; a canvas `root` is looked in first), not `file://`. There is no raw
`eval` on this plane.

## MCP (`solactl mcp`)

An MCP server on stdio that exposes the call plane to agents that speak
MCP. How it maps methods to tools is in
[The call plane § MCP](call.md#mcp-solactl-mcp).

```text
solactl mcp                      # every live owner
solactl mcp --owner design       # only Design (repeat --owner for more; owner or app id)
claude mcp add sola -- /opt/sola/bin/solactl mcp
```

- Every method a running app advertises is a tool named
  `<owner>_<method>` (dots become `_`): `design_screenshot`,
  `browser_tab_open`, `projects_pane_send`, … The list follows the apps
  that are up: start Design and its tools appear (the server checks every
  2 s and sends `tools/list_changed`).
- Each tool takes the same arguments as the `solactl <owner> <method>`
  flags, by their names, with JSON types (`integer`, `number`, `boolean`,
  `string`, `object`); values are passed as sent. Path arguments may be
  relative to the agent's working directory.
- A reply with a `path` to a PNG or JPEG (screenshots) also returns the
  image inline, up to 3,700,000 bytes; larger ones return the path and a
  note.
- The deadline, the Projects and Design context, and the Browser's bot
  rules are the same as for `solactl`. A Design page call without `canvas`
  goes to the canvas this server last opened or made.
- The compositor's bot refusals are part of `solactl compositor`, not of
  the methods, so with `SOLA_BOT=1` the compositor tools still work.
- The server exits when its input closes.

It is an adapter over the same methods, not a second protocol: what a tool
may do is what `solactl` may do. Which methods need a human confirm is
still open
([D3](../open-questions.md#d3--which-call-plane-methods-need-a-human-confirm-p1)).

## Other commands

These do not go through `sola-call`. Screenshot and synthetic input are
calls (`solactl compositor`), not bus topics.

### logs

```text
solactl logs [APP] [-f]
```

Prints `/opt/sola/log/<APP>.log` and exits. `APP` defaults to `sola`, the
log every Sola process writes to. `river` is River's own output, and
`app-<app_id>` is the output of an application the session launched
(`app-sola-browser`). `-f`, `--follow` keeps printing what is appended,
checked every 200 ms, until interrupted; a file that shrinks is read again
from the start. A missing file fails with `cannot open <path>` (status 3).

### emit

```text
solactl emit KIND PAYLOAD
solactl emit OpenUrl '{"url":"https://example.com","activate":true}'
```

Emits one bus topic ([The bus](bus.md)). `KIND` is the topic's name as
Monitor shows it (`Theme`, `OpenUrl`, `LaunchApp`, …); `PAYLOAD` is its
JSON in the same shape, so a topic seen in Monitor can be emitted again by
copying it. On success it prints `emitted KIND` (status 0). An unknown
kind prints the list of kinds; invalid JSON, a payload that does not match
the topic, or no bus connection fails with status 3. This is a developer
escape hatch: any topic can be faked.

### open

```text
solactl open URL|PATH
```

Opens a URL or file in the Browser, or an image in Paint. It does not go
through MIME.

- A path or `file://` URL whose extension is a raster image (`png`, `jpg`,
  `jpeg`, `jpe`, `gif`, `webp`, `bmp`, `tif`, `tiff`, `tga`) starts
  `sola-paint` with it (`$SOLA_PAINT`, else `/opt/sola/bin/sola-paint`).
- Anything else goes to the Browser. An absolute path becomes a
  `file://` URL; a relative path does too when the file exists or it
  starts with `./` or `../`. A running Browser gets the URL through its
  `chrome.sock`; otherwise `sola-browser` is started with it
  (`$SOLA_BROWSER`, else `/opt/sola/bin/sola-browser`).

`open` returns once the handoff or start is done (status 0); a missing
binary or a failed start is status 3. There is no fallback to another
browser. In a Sola terminal, `open` is an alias for `xdg-open`; that path
uses `sola-browser.desktop` (http(s), HTML, XHTML, PDF, `about:`, unknown
schemes), so for those both paths land in the Browser.

### media

```text
solactl media play-pause|next|prev|mute|vol-up|vol-down
```

The Shell runs this for the media keys; it works from a terminal too.

| Action | Does |
|---|---|
| `play-pause`, `next`, `prev` | Acts on an MPRIS player on the session D-Bus: one that is playing, else the first that can be controlled. `next` and `prev` do nothing when the player says it cannot |
| `mute` | Toggles mute on the default audio sink (`wpctl`) |
| `vol-up` | Raises the default sink by 5%, capped at 100% |
| `vol-down` | Lowers the default sink by 5% |

With no player, a transport action prints `no MPRIS player available` and
exits 0. A D-Bus or `wpctl` failure is status 3.
