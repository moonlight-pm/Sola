# solactl

`solactl` is the operator command line. Most of its commands are calls on
the call plane, `sola-call` ([The call plane](call.md)); `emit` and
`background` write to the bus; `logs`, `open`, and `media` work locally.
On Oath it comes in the `sola` package as `/bin/solactl`; elsewhere it is
installed as `/opt/sola/bin/solactl`. Oath is recognised by
`/oath/INDEX.md`; the same test picks Oath's directories for logs and
programs below.

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
solactl OWNER [METHOD [ARGS…]]       # any live owner, by owner or app id
solactl mcp [--owner OWNER]…
solactl logs [APP] [-f]
solactl emit KIND PAYLOAD
solactl open TARGET
solactl background set IMAGE
solactl background clear
solactl media play-pause|next|prev|mute|vol-up|vol-down
solactl help [COMMAND…]
```

| Command | Goes through | Needs |
|---|---|---|
| `compositor`, `session` | Call plane; flags compiled into `solactl` | `sola-call` and the owner; `--help` needs nothing |
| `projects`, `browser`, `bots`, `design`, any other name | Call plane; flags read from the owner's advertised methods | `sola-call` and the owner, also for help |
| `mcp` | Call plane | `sola-call`; tools appear as owners do |
| `emit`, `background` | Bus | `sola-bus` (the picture is drawn by `sola-river`) |
| `logs` | Files in Sola's log directory | nothing |
| `open` | Hands off to, or starts, the Browser, Paint, or Calendar | their binaries |
| `media` | Session D-Bus (MPRIS) and `wpctl` | a player, PipeWire |

`solactl` never starts an application to answer a call. There is no
`--version`, and there are no `eval`, `watch`, or generic `call` commands:
a method is called by its owner's name, and [Monitor](sola-monitor.md)
shows bus and call traffic.

## Output and exit status

A successful call prints the reply's `data` to stdout as indented JSON,
and prints nothing when the reply has no data. Method lists and help are
plain text. Errors go to stderr, prefixed `solactl: ` (`solactl media: `
for `media`); usage errors in a compiled command are clap's `error: …`.
Call output written into a closed pipe (`solactl browser tabs | head`) is
not an error.

| Status | Meaning |
|---|---|
| 0 | Success; also `media` with no player to act on |
| 1 | The owner or the host answered with an error. This includes `timeout`, and `<owner> is not running` for `compositor` and `session`. Also any failure of `compositor input drag` |
| 2 | Usage error in a compiled command (`compositor`, `session`, `mcp`, `logs`, `emit`, `open`, `background`, `media`): a missing or bad argument or an unknown subcommand; usage is printed |
| 3 | Local failure: `sola-call` unreachable, a live owner not in the catalog, an unknown method, a flag error, a `SOLA_BOT=1` refusal, a bad chord, or a failed `logs`, `emit`, `open`, `background`, or `media` |

In a compiled command a negative value must be joined to its flag:
`solactl compositor input scroll --dy=-5`. Written apart (`--dy -5`), the
`-5` is read as a flag and the command fails with status 2. Live-owner
flags are the other way round: they take a negative number written apart
(`--dy -5`), and have no `--name=value` form.

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
| `SOLA_BUS_PATH` | `emit`, `background` | Bus socket. Default `$XDG_RUNTIME_DIR/sola-bus` |
| `XDG_RUNTIME_DIR` | call commands, `mcp`, `emit`, `background` | Directory of the default sockets. When it is unset or empty (an SSH login on Oath sets none): `/run/user/<uid>` if that directory exists, else `/tmp` |
| `SOLA_BOT` | `compositor`, `browser`, `mcp` | `1`, `true`, `TRUE`, or `yes` turns on the [bot rules](#bot-rules) |
| `SOLA_PANE_ID` | `projects whoami`, `projects tab.spawn` | Default `--pane` and `--parent`. Projects sets it in each session |
| `SOLA_WS_PATH` | `projects whoami` | Default `--path`. Projects sets it in each pane to the tab's checkout path |
| `HOME` | path arguments, `open`, `logs` | `~` expansion; default location of `chrome.sock` and, on Oath, of the logs |
| `SOLA_LOG_DIR` | `logs` | Log directory. Default `$XDG_STATE_HOME/sola` on Oath (`~/.local/state/sola` when that is unset or not absolute), else `/opt/sola/log` |
| `SOLA_BIN_DIR` | `open` | Where Sola's programs are. Default `/bin` on Oath, else `/opt/sola/bin` |
| `XDG_DATA_HOME` | `open` | The Browser's handoff socket is `$XDG_DATA_HOME/sola/browser/chrome.sock` (default `~/.local/share/…`) |
| `SOLA_BROWSER` | `open` | Browser binary to start. Default `sola-browser` in Sola's bin directory. A path that is not a file is ignored |
| `SOLA_PAINT` | `open` | Paint binary to start. Default `sola-paint` in Sola's bin directory. A path that is not a file is ignored |
| `DBUS_SESSION_BUS_ADDRESS` | `media` | Session D-Bus used to find MPRIS players |
| `PATH` | `media` | Finds `wpctl` |

Over SSH, `XDG_RUNTIME_DIR` is the one that matters: with the fallback to
`/run/user/<uid>`, `solactl` run as the desk user from an SSH login reaches
the session's sockets without setting anything.

The working directory matters too: relative path arguments resolve
against it, and `design canvas.new` makes its canvas in it by default.

## Live-owner commands

`solactl projects`, `browser`, `bots`, and `design`, and `solactl NAME`
for any other owner, take their methods and flags from what the running
owner advertises ([The call plane](call.md#methods-and-arguments)). The
sections below list each owner's methods as they are advertised today.

### Owners

| Owner | App id | Process | Present | Section |
|---|---|---|---|---|
| `compositor` | `sola-river` | `sola-river` | Whole session | [Compositor](#compositor-solactl-compositor) |
| `session` | `sola-session` | `sola-session` | Whole session | [Session](#session-solactl-session) |
| `bots` | `sola-botsd` | `sola-botsd` | While the daemon runs | [Bots](#bots-solactl-bots) |
| `projects` | `sola-projects` | `sola-projects` | While **Projects** is open | [Projects](#projects-solactl-projects) |
| `browser` | `sola-browser` | `sola-browser` | While the **Browser** is open | [Browser](#browser-solactl-browser) |
| `design` | `sola-design` | `sola-design` | While **Design** is open | [Design](#design-solactl-design) |

No other application offers methods: Calendar, Mail, Settings, Paint, and
the Shell have none. The Shell's desktop workspaces have no `solactl`
command; `solactl workspaces` is read as an owner name and fails with
`workspaces is not running` (status 3).

`compositor` and `session` have compiled commands, but they are live
owners too, so their app ids reach them through the generic parser:
`solactl sola-river cursor --visible false` (see
[Methods with no compiled flags](#methods-with-no-compiled-flags)).

### Listing and help

```text
solactl OWNER                  # the owner's methods, one line each
solactl OWNER --help           # the same (-h too)
solactl OWNER METHOD --help    # the method's flags
```

`OWNER` may be the owner or its app id (`solactl sola-design status`). The
list starts with `solactl <name> — <app id>`, then each method's name and
summary. A method's help lists each flag as `-s, --long <value>`, then
its help text and `(required)` or `(optional)`; bool flags have no
`<value>`. `-h` or
`--help` anywhere after the method prints that help instead of calling.

### Arguments

- `--name VALUE` or `-s VALUE` sets an argument; `--name=VALUE` is not
  split. When a flag is given twice, the last one wins.
- A bool flag alone is `true`. `--name true`, `false`, `1`, or `0` sets it
  explicitly. Bool flags may come before other flags.
- A value that starts with `-` is refused (`--name needs a value`), except
  a negative number.
- A bare word fills the next required argument not yet given, in the
  advertised order: `solactl design open design/home`,
  `solactl bots new Suno`. A bare word with no required argument left is
  an error (`unexpected argument`).
- A single JSON object as the only argument is sent as the params,
  without conversion or path resolution:
  `solactl browser tab.open '{"url":"https://example.com"}'`. The
  [context](#context) is still added and the [bot rules](#bot-rules)
  still apply. JSON that does not parse is an error (status 3).
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
`--prompt-file`; `browser` `--file` and screenshot `--path`; `design`
`--path`, `--canvas`, and `--out`; and the compositor's screenshot
`--output` when you reach it by app id (`solactl sola-river screenshot`).
Other arguments are sent as typed.

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

The web path for a bot is `solactl browser` only. The Browser rules go
with the owner, so they hold however the Browser is named (`browser` or
`sola-browser`) and over MCP. Limit: the compositor refusals belong to the
`solactl compositor` command, not to its methods, so they do not apply
over MCP or through the app id (`solactl sola-river …`).

## Compositor (`solactl compositor`)

Owner `compositor`, process `sola-river`, present for the whole session
([Compositor](compositor.md)). The flags are compiled into `solactl`.

### screenshot

| Flag | Default | Meaning |
|---|---|---|
| `-o`, `--output PATH` | `/tmp/sola/screenshots/<ms>.png` | Where to write. Sent as given: a relative path is taken from `sola-river`'s working directory, so pass an absolute one. Parent directories are created |
| `-a`, `--app APP` | the whole output | Capture this app's window (exact app id) |
| `-w`, `--window TITLE` | the app's first window | With `--app`, the window whose title is exactly `TITLE`; ignored without `--app` |
| `--format png\|rgba` | `png` | `rgba` (or `rgba8`) writes packed RGBA8 with no PNG encode; its default path is `/dev/shm/sola-freeze-<ms>.rgba` (`/tmp/sola/screenshots/<ms>.rgba` without `/dev/shm`) |
| `-t`, `--timeout SECS` | 10 | Deadline |

`--app` copies that window's own buffer (`ext-image-copy-capture`). The
window does not need to be on top and is not raised. The default PNG uses
fast compression. The reply is `{"path": …}`; with `rgba` it adds `width`,
`height`, and `"format": "rgba8"`. Errors:
`screenshot already in progress`,
`window not found for capture: app_id=… title=…`, and
`unknown screenshot format … (png|rgba)`.

The Shell's screenshot chords copy to the clipboard instead of writing
this file (see [Shell § Screenshots](sola-shell.md#screenshots)). The
method also takes a region, which has no flags here (see
[Methods with no compiled flags](#methods-with-no-compiled-flags)).

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
| `input click X Y` | `-b`, `--button left\|right\|middle` (also `l`, `r`, `m`; lower case only) | `left` |
| `input move X Y` | none | |
| `input scroll` | `-x`, `--dx DX`; `-y`, `--dy DY` (decimal; down is positive; a negative value as `--dy=-5`) | `0`, `5` |
| `input key CHORD` | none | |
| `input press [X Y]` | `-b`, `--button` as for `click` | `left` |
| `input release [X Y]` | `-b`, `--button` as for `click` | `left` |
| `input drag X1 Y1 X2 Y2` | `-b`, `--button`; `-s`, `--steps N` (moves between press and release) | `left`, `12` |

Each takes `-t`, `--timeout SECS` (default 5) and replies `{"ok": true}`
(`drag` prints nothing; on failure it prints `solactl: <method>: <error>`
for the step that failed, and exits 1). Another button name fails with
`unknown button …`. `X` and `Y` are integer output coordinates. The
events go through the seat like real input, to whatever is under the
pointer or focused. `press` holds the button until a `release`; with
`X Y` either moves there first. `drag` is a press at `X1 Y1`, `STEPS`
moves about 16 ms apart, and a release at `X2 Y2`, so it is seen as a drag
(a row in the bar, a sidebar reorder) rather than a click.

`CHORD` is `MODIFIER+…+KEY`, in any case. Modifiers: `Meta` (or `Super`,
`Win`, `Cmd`), `Alt` (or `Option`), `Ctrl` (or `Control`), `Shift`. Keys:
`Tab`, `Esc` or `Escape`, `Enter` or `Return`, `Backspace` or `BS`,
`Space`, `Left`, `Right`, `A`–`Z`, `0`–`9`, `` ` `` (or `Grave`,
`Backtick`), and the keypad: `KP0`, `KP2`, `KP4`, `KP5`, `KP6`, `KP8`,
`KPEnter`, `KPMultiply` (or `KP*`), `KPAdd`, `KPEqual` (or `KP=`),
`KPDecimal` (or `KP.`). Any other key is refused (`unknown key '…'`,
status 3).

### Methods with no compiled flags

- `cursor` (`--visible true|false`, default `true`; deadline 2 s) shows or
  hides the compositor's pointer and replies `{"visible": …}`.
- `screenshot` also takes a region: `--x`, `--y`, `--width`, `--height`,
  all four, in output coordinates. With `--app` the region is ignored.

`solactl compositor` has no command or flag for either. Reach them by the
compositor's app id, which goes through the generic parser, or over MCP
(`compositor_cursor`, `compositor_screenshot`):

```bash
solactl sola-river cursor --visible false
solactl sola-river screenshot --x 0 --y 0 --width 640 --height 480 -o shot.png
```

That way the deadline is the method's own (2 s for `cursor`, 8 s for
`screenshot`), `-o`/`--output` is a path argument resolved against your
working directory, and the bot refusals do not apply.

`input.key` expects `chord` as an object (`keycode`, an XKB key code, and
`meta`, `alt`, `ctrl`, `shift` booleans, all five given), which
`solactl compositor input key` builds from the text form. A text chord
sent to it by app id or over MCP (`compositor_input_key`) is refused with
`chord: invalid type …`; the JSON form works:

```bash
solactl sola-river input.key '{"chord":{"keycode":23,"meta":true,"alt":false,"ctrl":false,"shift":false}}'
```

## Session (`solactl session`)

Owner `session`, process `sola-session`, present for the whole session
([Session manager](session.md)). The flags are compiled into `solactl`.

| Command | Flags | Reply |
|---|---|---|
| `launch APP_ID` | `-c`, `--command COMMAND`; `-t`, `--timeout SECS` (5) | `{"app_id", "command", "pid"}` |
| `close APP_ID` | `-t`, `--timeout SECS` (5) | `{"app_id", "stopped": N}` |

`launch` runs `COMMAND`, split on whitespace with no shell quoting. The
default is `<APP_ID>` in Sola's bin directory (`/bin/<APP_ID>` on Oath,
`/opt/sola/bin/<APP_ID>` elsewhere) when that file exists, else `APP_ID`
itself. Errors: `empty command`, or the spawn error. `close` stops the
processes the session launched for `APP_ID`; with none it fails with
`no live children for <APP_ID>`.

## Bots (`solactl bots`)

A bot is a named informational agent session. Each bot is a directory in
`~/Bots`, as each project is a directory in `~/Projects`, and runs on the
AI it was made with: **Grok**, **Claude**, or **Gab AI**. The AI is chosen
when the bot is made and does not change after. `solactl bots` needs the
bots daemon, `sola-botsd` (owner `bots`). It fails if the daemon or
`sola-call` is down; it does not launch a window. The process manager
`sola` keeps the daemon running; on Oath no service starts it, and until
it is started by hand `solactl bots` fails with `bots is not running`.

```text
solactl bots                         # list methods
solactl bots list
solactl bots poll [-b BOT]
solactl bots new -n NAME -a AI       # AI: grok, claude, or gab
solactl bots setup -b BOT -a AI      # a directory made by hand
solactl bots send -b BOT -t TEXT
solactl bots transcript -b BOT
solactl bots rm -b BOT
solactl bots cancel -b BOT
```

`-b`, `--bot` is a bot's id, slug, or name; `-n`, `--name` is a display
name; `-a`, `--ai` is `grok`, `claude`, or `gab`; `-t`, `--text` is the
message. All are required where they appear, except `--bot` on `poll`.
Every method has the default 8 s deadline.

| Method | Does |
|---|---|
| `list` | Bots, their AI, and their status |
| `poll` | `{"bots", "transcript"}` in one call; `transcript` is `null` without `--bot`, or for a bot it does not find |
| `new` | Creates a bot on an AI, and its home |
| `setup` | Gives a bot directory without an AI its AI; fails if it has one |
| `send` | Sends a message and returns while the turn runs; read the answer with `transcript` or `poll` |
| `transcript` | The dialog turns |
| `rm` | Deletes the bot's directory (and a Grok bot's session) |
| `cancel` | Cancels the current turn |

### Homes and the write fence

Each bot's home is `~/Bots/<slug>/`. Its `bot.json` holds its name, its
AI, an optional `model` (empty for the AI's default), and its session; its
dialog is `dialog.json`. A directory made in `~/Bots` by hand shows up as
a bot with no AI; it talks once `setup` (or its header in the **Bots**
window) gives it one. `new` and `setup` seed the home and start a first
turn, an introduction (the orientation prompt is not shown as a user
message). A catalog from before bots were directories,
`~/.config/sola/bots/catalog.json`, is moved once into each bot's
`bot.json` (all on Grok); the old file is kept as `catalog.json.moved`.

The write fence is enforced by the prompt only. By default a bot writes
only inside its home. To open the fence for one task, say “drop the write
fence” or “work on the whole computer”, or name a path outside the home.
Seat steal is refused even then. A bot is not a coding agent, and it does
not use Projects unless you ask for that rail.

### The AIs

| AI | Runs as | Signs in with |
|---|---|---|
| Grok | `grok agent stdio` (ACP), one child per bot, all tools approved | the Grok CLI's own login; run `grok` in a terminal once to sign in |
| Claude | `claude -p` per message on the bot's session (`--resume`), all tools approved, no MCP servers | Claude Code's own login, the subscription; an `ANTHROPIC_API_KEY` is never passed on |
| Gab AI | Gab's chat API (`https://gab.ai/v1`, model `arya`), the dialog sent each turn | an API key in `GAB_API_KEY` or `~/.config/sola/bots/gab.key` (Gab AI → Settings → API; Plus plan) |

Grok and Claude bots are agents in their home: shell, files, and the
browser through `solactl browser`, under `AGENTS.md` (Claude reads it
through the home's `CLAUDE.md`). An `AGENTS.md` that was already in a
directory made by hand is kept as `AGENTS.md.orig`. A Gab AI bot only
chats: no tools, its `CURRENT.md` in its system prompt. The daemon runs
`$SOLA_GROK` / `$SOLA_CLAUDE` when set, else `~/.local/bin/grok` /
`~/.local/bin/claude` when that file exists, else `grok` / `claude` on
`PATH`.

### The socket API

The daemon serves HTTP on a Unix socket, `$XDG_RUNTIME_DIR/sola-bots`
(mode 0600; `SOLA_BOTS_PATH` overrides it), with no token: the socket is
the user's alone. There is no TCP port and no access from another
computer.

```text
GET  /health
GET  /bots
GET  /events                 # SSE: snapshot, bots, transcript, delta, removed
GET  /poll?bot={id}          # one-shot snapshot (debug)
GET  /bots/{id}/transcript
POST /bots                 { "name": "Suno", "ai": "claude" }
POST /bots/{id}/setup      { "ai": "gab" }
DELETE /bots/{id}
POST /bots/{id}/send     { "text": "…" }
POST /bots/{id}/cancel
```

```bash
curl --unix-socket "$XDG_RUNTIME_DIR/sola-bots" http://localhost/bots
```

`GET /events` is `text/event-stream` (chunked), with keepalive comments
every 15 s. The **Bots** window uses this surface (list, send, create,
delete, and live events). `solactl bots` uses `sola-call`, not HTTP. The
iPhone app (SolaBot) spoke the old TCP API and cannot reach this one.

## Projects (`solactl projects`)

Needs **Projects** running (owner `projects`, app id `sola-projects`).
Fails if the app or `sola-call` is down; it does not launch a window.

A **session** is a terminal (a shell, or an agent: Grok, Codex, Claude)
in a project. It is a tmux session on the computer it runs on, so it
runs whether or not a window shows it. Every session starts in its
project's folder; a project can have any number. A project is a folder in
`~/Projects`, here or on another computer.

Each session open here is its own window, a row in the shell's bar. The
row names three things, drawn apart: the project, the session (when it is
not the project's main one), and the computer it runs on — always, this
one too — as a chip in that computer's prompt color. In place of its icon
the row shows the session's activity: an amber diamond while it waits on
a decision, a spinning ring while it works.

Any computer's Projects can open any computer's sessions: two computers
can watch and type in the same session. Its size follows the computer
that typed last. **Closing a window detaches**: that computer stops
watching and the session keeps running, as it does when Projects quits.
**Project → End Session…** (or `tab.rm`) ends a session — its tmux is
killed where it runs, for every computer watching; it asks first while
its agent is working or waiting. A session whose shell exits shows
**Start new shell**.

Projects makes no worktrees. A session makes its own when its work needs
one (`git worktree add .worktrees/<slug>` from the project root); ending
a session never touches them.

New Project… (⌘N) makes `~/Projects/<name>` and opens it. New Session…
(⌘T) starts one in the focused session's project, on its computer, in a
window of its own. Closing the last window quits Projects; started with
nothing to open, Projects waits 20 seconds for a `project.open` (the
launcher's), then quits. Projects was called Workspaces; `solactl
workspaces` and the `workspace.*` methods are gone. The verbs still say
`tab`: a tab is a session.

```text
solactl projects                           # list methods
solactl projects ps
solactl projects project.list
solactl projects project.open --path ~/Projects/Sola    # its sessions here
solactl projects project.open --host <computer> --path /home/<user>/Projects/Sola [--session ws-main]
solactl projects project.close --project Sola   # its windows close; sessions run on
solactl projects tab.list [--project Sola]
solactl projects tab.spawn --project Sola [--name review] [--title 'fix login'] \
    [--agent grok|codex|claude] [--prompt '…' | --prompt-file FILE] [--parent …] [--select]
solactl projects tab.set --tab review [--name …] [--title 'fix login']
solactl projects tab.exec --tab review [--agent grok|codex|claude] \
    [--prompt '…' | --prompt-file FILE]
solactl projects tab.select --tab review
solactl projects tab.rm --tab review        # end it, for every computer
solactl projects pane.list [--tab review]
solactl projects pane.send --text 'follow up' --enter [--pane review]
solactl projects pane.read [--pane review] [--lines 40]
solactl projects pane.wait [--pane review] [--status done] [--timeout 300] [--fresh]
solactl projects whoami                    # from a Projects pane; or --pane / --path
```

| Short | Long | Methods |
|---|---|---|
| `-p` | `--project` | `project.close`, `tab.list`, `tab.spawn` |
| `-p` | `--path` | `project.open` |
| `-n` | `--name` | `tab.spawn`, `tab.set` |
| `-a` | `--agent` | `tab.spawn`, `tab.exec` |
| `-s` | `--select` | `tab.spawn` |
| `-t` | `--tab` | `tab.rm`, `tab.select`, `tab.set`, `tab.exec`, `pane.list` |
| `-t` | `--text` | `pane.send` |
| `-e` | `--enter` | `pane.send` |
| `-l` | `--lines` | `pane.read` |
| `-s` | `--status` | `pane.wait` |
| `-f` | `--fresh` | `pane.wait` |

Required arguments: `--path` on `project.open`; `project.close
--project`; `tab.spawn --project`; `--tab` on `tab.rm`, `tab.select`,
`tab.set`, `tab.exec`; `pane.send --text`. `--tab` on `pane.list` and
`--pane` on `pane.send`, `pane.read`, and `pane.wait` default to the
focused session. `tab.list` without `--project` lists every session open
here.

Deadlines: `project.open` 15 s; `tab.spawn` and `tab.exec` 60 s;
`pane.wait` 302 s, or its `--timeout` plus 2 s; the rest 8 s.

Errors name what went wrong: `<path> is not a folder of ~/Projects` for
`project.open`, `a session '<name>' is already open in this project`, and
`no tab selected` when a default has nothing to fall back on.

### Open

`project.open` opens a project's sessions here, each in its own window:
every session running on its computer in that folder, whichever computer
started it (one already open here comes forward). With none running
anywhere, it starts the project's main session. `--session <id>` (its id
there, as `ws-main`) opens that session alone: the launcher's chooser does
this from a session's row. A folder that is not a
project here yet is added first. Its reply is the project with `opened`
(none of its sessions was open here). The launcher's Projects chooser
uses it, and Project → Open lists the folders of `~/Projects` with no
session open here.

A session says what it is to any computer that lists it: tmux options on
it (`@sola-project`, `@sola-name`, `@sola-title`, `@sola-kind`). One made
before those existed is placed by the folder its pane is in.

`project.close` closes the project's windows here and replies
`{"project", "closed": true}`; its sessions keep running.

### Start

`tab.spawn` starts a session in the project's folder, on the project's
computer, in a window of its own. `--name` is its name, unique among the
project's sessions open here (default: the next number, `2`, `3`, …);
`--title` is its label (default: the name). The reply is the new session:
`id`, `name`, `title`, `label`, `path`, `kind`, `parent`, `project`, and
`selected`.

It runs in the background: its window lands behind, and the shown windows
stay. `--select` brings it up (as ⌘T does). When you spawn from a Projects
pane, the parent defaults to `$SOLA_PANE_ID`; `--parent` takes a session,
pane, or path.

`--agent` is `grok`, `codex`, or `claude`. Other names are an error
(OpenCode is presence-only). `--prompt` without `--agent` implies Grok.
`--prompt` and `--prompt-file` are exclusive. The first Codex session may
need `/hooks` in its TUI to trust the Sola status hook. Claude hooks are
merged into `~/.claude/settings.json` and apply to the next Claude
session. On start, Projects finds its existing Codex and Claude hook
entries by script file name and rewrites them in place (status message
"Sola Projects status"). Its catalog, last status, and hook scripts live
in `~/.config/sola/projects/`.

`tab.set` sets a session's `--name` and `--title`; its id and its tmux
stay. Target a session by id, name, path, or label. `tab.select` brings
its window up and replies `{"id", "selected": true}`.

### Sessions on another computer

`project.open --host <computer> --path <folder>` opens a folder on another
computer (an ssh host: a name, `user@name`, or a `~/.ssh/config` alias).
`--path` is the folder's full path there. `solactl` resolves `--path` on
this computer, so `~` becomes this computer's home and a relative path is
taken from your working directory here; write the full path. The
launcher's Projects chooser does this from **On another computer…**.

Its sessions run there, in that computer's own Projects tmux (socket
`sola-ws`, the one its own Projects uses), reached over ssh: a Projects
restart, or losing the network, leaves them running, and the windows
attach them again. Here such a session's id is `<id>@<computer>`.

Status lives on the computer a session runs on: its agents' hooks append
their events to that computer's hook log
(`~/.local/state/sola/projects/hooks.log`), and every Projects watching any
of its sessions follows that log — this computer's by reading it, another
computer's over a connection of its own (the link, `ssh … tail -F`). So
marks, islands, desk cards and `pane.wait` work on every computer that has
the session open, whichever one started it; looking at or answering an
island on one lets it go on all. The first link installs the same agent
hooks there (Claude's `settings.json`, Codex's `hooks.json`, Grok's
`sola-status.json`; scripts in `~/.config/sola/remote/`). `pane.send`, `pane.read` and `tab.exec` reach
its panes; a lost session whose Grok or Claude transcript is still there
resumes.

Sessions opened from another computer before 2026-10-30 lived on a socket
of their own there (`sola-ws-remote`); they are still found and attached
until they end, and nothing new is made there. A session's ssh may ask
for a password in its window. The link and the listing never ask: they
need a key or an agent that logs you in, and tmux and base64 there.

- The connection drops: the window says **Can't reach** the computer,
  with ssh's reason, and **Reconnect**. The session there is left alone,
  and the link is tried again every 20 seconds; windows come back with it.
- Agents running there cannot drive this Projects (`solactl projects`
  there talks to that computer's own Sola).

### Lists and sessions

`tab.list` replies `{"tabs": […]}`; `ps` replies `{"projects",
"selected"}` with each project's sessions under `projects[].tabs[]`. A
session has `id`, `name`, `title`, `label`, `path`, `kind`, `parent`,
`status`, `agent`, and `project`, plus `host` on another computer; in
`ps` and the `tab.set` reply it also says `selected`. A project has `id`,
`name`, `root`, and `host` on another computer. `project.list` replies
`{"projects": […]}`, those with a session open here, each with `open`.

`whoami` replies `pane`, `tab` (id), `tab_name`, `project`,
`project_name`, `path`, `kind`, `status`, `agent`, and `session_id`.
`tab.exec` replies `{"tab", "pane", "started", "sent"}`: `started` when it
launched the agent, `sent` when it pasted the prompt into one already
running. `kind` is `main` (the project's main session), `folder` (the main
session of a non-git project), or `root` (any other).

`pane.list` and `whoami` include `session_id` when that pane's Grok or
Claude owner session is known. Use it with `grok -r` or `claude --resume`
after a reboot loses tmux.

### Panes

`pane.send` and `tab.exec --prompt` **paste** into the live agent (tmux
bracketed paste), so a multiline brief does not submit on the first
newline and a long prompt is not truncated. `tab.exec` then presses
Enter; `pane.send` does only with `--enter`. A failed paste returns the
tmux error, not a bare `send failed`.

`pane.send` replies `{"ok": true, "pane"}`. `pane.read` replies
`{"text", "pane"}` with the whole scrollback, or the last `--lines N`
lines.

`pane.wait` holds until the session's status is `--status` (`working`,
`waiting`, `done`, or `idle`; default `done`), for up to `--timeout`
seconds (default 300, at most 3600). `--fresh` waits for a transition
onto that status. It replies `{"pane", "status"}`, or fails with
`timeout waiting for pane <pane> to be <status>`.

### End

`tab.rm` ends a session: its tmux is killed where it runs, for every
computer watching it, and its window here closes. It replies
`{"ok": true}` first, then ends it on the next tick, so a call from
inside that session can finish instead of hanging; the kill runs off the
UI thread. Its folder and any worktree stay.

## Browser (`solactl browser`)

Needs the **Browser** (`sola-browser`) running (owner `browser`). Fails if
the Browser or `sola-call` is down; it does not launch a window. Each tab
is a page with its own window, a row in the shell's bar, the same
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

Required arguments: `--url` on `tab.open` and `goto`; `tab.focus --tab`;
`--text` on `find.page` and `find`; `key --chord`; `type --ref --text`;
`select --ref --values`. `click` and `hover` need `--ref`, `--text`, or
both `--x` and `--y` (`--text` with `--x`/`--y` is refused); `fill` needs
`--ref` or `--control`, and `--text` or `--file`; `get` needs `--ref` or
`--control`; `scroll` needs a non-zero `--dx` or `--dy`.

`--tab` is an id, or a substring of one tab's URL or title (any case).
More than one match fails with `<n> tabs match …; pass an id`. Without
it, page methods use the focused tab, except `click`, `hover`, `type`,
`fill`, `get`, and `select`, which use the tab of the last page call.
`tab.open` opens the page in its own window behind the shown one in the
focused zone, and does **not** focus it unless you pass `--select`;
`goto` does not focus either. `tab.focus` and `--select` show the page's
window and focus it. Closing the last tab opens a blank page
(`about:blank`) in its place, so one page is always open.

`tabs` replies `{"tabs", "active"}`; each tab has `id`, `url`, `title`,
`active`, `loading`, and `profile`. `tab.open` replies
`{"id", "selected"}`, `tab.focus` `{"id"}`, and `tab.close`
`{"ok": true}`.

Deadlines: `snapshot`, `goto`, `click`, `hover`, `key`, `scroll`, `type`,
`fill`, `get`, `links`, `select`, and `screenshot` 15 s; `wait` 32 s, or
its `--timeout` plus 2 s; the rest 8 s.

### Background tabs

Snapshot, click, fill, type, and `browser screenshot` target **that tab’s
CEF document**. They work when the tab is in the background and do not
raise it.

### Snapshots and refs

Page verbs use a pruned accessibility snapshot in YAML, with opaque refs
(`e12`, or `f1e3` inside a frame; a leading `@` is ignored). `snapshot`
replies `{"tab", "snapshot"}`: the text opens with `url:` and `title:`
lines (and `focused:` or `dialog: open` when they apply), then an
`actions:` list (role, name, ref); click those. `--interactive` keeps
controls only, `--ref` returns one subtree of the last snapshot, and
`--json` returns debug JSON instead of YAML. `click --text "Pay now"`
uses the last snapshot and fails with the candidate refs when more than
one control matches. A stale ref fails (the node is gone); take a new
snapshot.

`find` searches the last snapshot, not the page, and replies
`{"tab", "hits"}`; with no snapshot of that tab it fails and says to take
one.
`find.page` is ⌘F (`--back` searches backwards).

### Input

`click` and `hover` also take CSS-pixel `--x` `--y` in **that tab’s**
viewport. That needs no ref, and it is still CEF `Input.dispatch*`, not the
compositor seat.

`fill` and `type` write a contenteditable or ProseMirror editor with one
`paste` of the whole string (newlines kept); they do not assign
`textContent`. `type` appends; `fill` replaces, and takes the control by
`--ref` or by its actions-list name with `--control` (from the DOM
aria-label when the last snapshot has no such name). `fill --file PATH`
reads the string from disk (exclusive with `--text`; at most 100,000
bytes). The reply includes `value` and `matched: true`, or an error if the
field did not keep the text. `type --submit` presses Enter after.

`key --chord` (for example `Return`, `Escape`, `Control+Enter`) goes to
that tab, not the seat, and inserts once: one `char` event, not
keyDown-with-text plus char. A chord is `MODIFIER+…+KEY` in any case.
Modifiers: `Meta` (or `Super`, `Win`, `Cmd`, `Command`), `Alt` (or
`Option`), `Ctrl` (or `Control`), `Shift`. Keys: `Enter` or `Return`,
`Escape` or `Esc`, `Tab`, `Backspace` or `BS`, `Space`, `Delete` or
`Del`, `ArrowUp`/`Up`, `ArrowDown`/`Down`, `ArrowLeft`/`Left`,
`ArrowRight`/`Right`, `Home`, `End`, `PageUp` or `PgUp`, `PageDown` or
`PgDn`, and a single letter or digit. With `Ctrl`, `Meta`, or `Alt` held,
no character is typed. `scroll --dx --dy` (whole CSS pixels) go to the
same tab, from a wheel origin at `--x` `--y` (default 100, 100).

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

If `tabs` or `tab.open` time out while the Browser's windows are still
up, the Browser is not answering its calls. Quit it (**Browser → Quit
Browser**, ⌘Q) and launch it again. Do not kill the process from a bot.

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

`share` and `share.rotate` reply `{"url", "tunnel"}`: the link, and
whether a tunnel token is set ([Design § Share](sola-design.md#share)).
With sharing off they fail with `sharing is off (see the Design log)`.

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
claude mcp add sola -- solactl mcp
```

- Every method a running app advertises is a tool named
  `<owner>_<method>` (dots become `_`): `design_screenshot`,
  `browser_tab_open`, `projects_pane_send`, … The list follows the apps
  that are up: start Design and its tools appear (the server checks every
  2 s and sends `tools/list_changed`).
- Each tool takes the same arguments as the `solactl <owner> <method>`
  flags, by their names, with JSON types (`integer`, `number`, `boolean`,
  `string`, `object`); values are passed as sent. Path arguments may be
  relative to the agent's working directory; that includes the
  compositor's screenshot `output`, which
  `solactl compositor screenshot -o` sends as given.
- A reply with a `path` to a PNG or JPEG (screenshots) also returns the
  image inline, up to 3,700,000 bytes; larger ones return the path and a
  note.
- The deadline, the Projects and Design context, and the Browser's bot
  rules are the same as for `solactl`. A Design page call without `canvas`
  goes to the canvas this server last opened or made.
- The compositor's bot refusals are part of `solactl compositor`, not of
  the methods, so with `SOLA_BOT=1` the compositor tools still work.
  `compositor_input_key` wants `chord` as an object (see
  [Methods with no compiled flags](#methods-with-no-compiled-flags)).
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

Prints `<APP>.log` from Sola's log directory and exits: on Oath
`~/.local/state/sola/` (`$XDG_STATE_HOME/sola/` when that is set and
absolute), elsewhere `/opt/sola/log/`; `SOLA_LOG_DIR` overrides both.
`APP` defaults to `sola`, the log every Sola process writes to.
`app-<app_id>` is the output of an application the session launched
(`app-sola-browser`). `river` is River's own output where the process
manager `sola` starts River; on Oath, PID 1 runs River and Sola writes no
`river.log`. `-f`, `--follow` keeps printing what is appended,
checked every 200 ms, until interrupted; a file that shrinks is read again
from the start. A missing file fails with `cannot open <path>` (status 3).

### emit

```text
solactl emit KIND PAYLOAD
solactl emit OpenUrl '{"url":"https://example.com","activate":true}'
```

Emits one bus topic ([The bus](bus.md)) as `solactl`. `KIND` is the
topic's name as Monitor shows it (`Theme`, `OpenUrl`, `LaunchApp`, …),
spelt exactly; `PAYLOAD` is its JSON in the same shape, so a topic seen in
Monitor can be emitted again by copying it. A topic that carries no
payload still needs valid JSON (`{}`); its content is ignored. On success
it prints `emitted KIND` (status 0). An unknown kind prints the list of
kinds; invalid JSON, a payload that does not match the topic, or no bus
connection fails with status 3. This is a developer escape hatch: any
topic can be faked.

### open

```text
solactl open URL|PATH
```

Opens a URL or file in the Browser, an image in Paint, or an iCalendar
file in Calendar. It does not go through MIME.

- A path or `file://` URL whose extension is a raster image (`png`, `jpg`,
  `jpeg`, `jpe`, `gif`, `webp`, `bmp`, `tif`, `tiff`, `tga`; any case)
  starts `sola-paint` with it (`$SOLA_PAINT`, else `sola-paint` in Sola's
  bin directory: `/bin` on Oath, `/opt/sola/bin` elsewhere).
- A path or `file://` URL ending in `.ics`, `.ical`, or `.icalendar` (any
  case) starts `sola-calendar` from Sola's bin directory with it. A
  running Calendar keeps its window: the new process hands the path over
  (`$XDG_RUNTIME_DIR/sola/calendar.sock`) and exits. Calendar opens the
  event as a new draft.
- Anything else goes to the Browser, web URLs to images or `.ics` files
  included. An absolute path becomes a `file://` URL; a relative path
  does too when the file exists or it starts with `./` or `../`. A running
  Browser gets the URL through its `chrome.sock`; otherwise
  `sola-browser` is started with it (`$SOLA_BROWSER`, else `sola-browser`
  in Sola's bin directory).

`open` returns once the handoff or start is done (status 0); a missing
binary or a failed start is status 3, with
`open failed: <program> not found at <path>` or the spawn error. There is
no fallback to another browser or image viewer.

### background

```text
solactl background set IMAGE
solactl background clear
```

`set` copies `IMAGE` into `~/.config/sola/background/` (with the pictures
set before it, which Settings → Appearance lists; a different file of the
same name is kept as `name-2.ext`; the same file again uses its copy), makes it the picture behind
the windows (`Topic::Background`, which survives a restart), and prints the
copy's path. The original can then move or go. `sola-river` draws it
filling the output, centre-cropped. `clear` goes back to the plain black
ground. A file that cannot be read, or no bus connection, is status 3; a
picture that will not decode is logged by `sola-river` and the ground stays
as it was. Paint's **File → Set as Background** does the same for the
image in front.

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
