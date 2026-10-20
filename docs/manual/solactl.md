# solactl

`solactl` is the operator command line. It talks to the call host,
`sola-call`, for compositor, session, and app methods, and to the bus only
for `emit`.

## Call plane

These commands need `sola-call` and the process that owns the method.

```text
solactl compositor screenshot [-o PATH] [--app APP] [--window TITLE] [--format png|rgba]
solactl compositor sample [--size N]
solactl compositor windows
solactl compositor input click|move|scroll|key …   # rejected when SOLA_BOT=1
solactl session launch <app_id> [--command CMD]
solactl session close  <app_id>
```

If `sola-call` or the owner is down, the command fails. It does **not**
launch a window.

`compositor screenshot --app` copies that window’s own buffer
(`ext-image-copy-capture`). The window does not need to be on top and is
not raised. `--format rgba` writes packed RGBA8 (no PNG) for the Shell's
freeze picker. The default PNG uses fast compression. The Shell's
screenshot chords copy to the clipboard instead of writing this file (see
[Shell § Screenshots](sola-shell.md#screenshots)).

`workspaces`, `browser`, `bots`, and `design` are first-class subcommands
(see `solactl` or `solactl --help`). Any other running app that advertises
methods works the same way: `solactl <app-id>` lists its methods, and
`solactl <app-id> <method> …` invokes one.

Path arguments (`--path`, `--file`, `--out`, `--prompt-file`, …) are
resolved by `solactl` before the call: `~` expands, and a relative path is
taken from **your** current directory, not the owner app's.

## Bots (`solactl bots`)

A bot is a named informational agent session with its own home directory.
`solactl bots` needs the bots daemon, `sola-botsd` (owner `bots`). It
fails if the daemon or `sola-call` is down; it does not launch a window.

```text
solactl bots                         # list methods
solactl bots list
solactl bots new --name Suno
solactl bots send --bot suno --text 'hello'
solactl bots transcript --bot suno
solactl bots rm --bot suno
solactl bots cancel --bot suno
```

### Homes and the write fence

Each bot's home is `~/Bots/<slug>/`. `new` seeds the home and starts a
first turn, an introduction (the orientation prompt is not shown as a user
message). `rm` deletes the catalog row, the home directory, and the Grok
session.

The write fence is enforced by the prompt only. By default a bot writes
only inside its home. To open the fence for one task, say “drop the write
fence” or “work on the whole computer”, or name a path outside the home.
Seat steal is refused even then. A bot is not a coding agent, and it does
not use Workspaces unless you ask for that rail.

### Phone API

The daemon also serves HTTP on `0.0.0.0:27419`. Its public name is
`https://bot.sola.computer`, with TLS at a proxy. Open **27419/tcp** on the
Sola machine to that proxy.

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

## Workspaces (`solactl workspaces`)

Needs **Workspaces** running (owner `workspaces`). Fails if the app or
`sola-call` is down; it does not launch a window.

```text
solactl workspaces                         # list methods
solactl workspaces ps
solactl workspaces project.list
solactl workspaces project.add --path ~/Workspace/Sola
solactl workspaces project.startup --project Illuno
solactl workspaces project.startup --project Illuno --script 'cp -a "$PROJECT/.grok" "$WORKTREE/"'
solactl workspaces project.rm --project Sola
solactl workspaces project.reorder --project Sola [--before Illuno]
solactl workspaces workspace.list [--project Sola]
solactl workspaces workspace.spawn --project Sola --name ticket-123 \
    [--branch joshua/sc-1234/fix] [--base-branch origin/dev] [--title 'fix login'] \
    [--agent grok|codex|claude] [--prompt '…' | --prompt-file FILE] [--parent …] [--select]
solactl workspaces workspace.spawn --project Sola --name hotfix --root   # no worktree
solactl workspaces workspace.set --workspace ticket-123 --title 'fix login'
solactl workspaces workspace.set --workspace adhoc --name sc-1234 \
    [--title 'fix login'] [--branch joshua/sc-1234/fix] [--before root]
solactl workspaces workspace.exec --workspace ticket-123 [--agent grok|codex|claude] [--prompt '…']
solactl workspaces workspace.select --workspace ticket-123
solactl workspaces workspace.rm --workspace ticket-123 [--worktree] [--force]
solactl workspaces pane.list [--workspace ticket-123]
solactl workspaces pane.send --text 'follow up' --enter [--pane ticket-123]
solactl workspaces pane.read [--pane ticket-123] [--lines 40]
solactl workspaces pane.wait [--pane ticket-123] [--status done] [--timeout 300] [--fresh]
solactl workspaces whoami                  # from a Workspaces pane; or --pane / --path
```

Bool flags (`--enter`, `--fresh`, `--select`) can sit before other flags.
Spawn, add, and wait use a longer call deadline than the default 8 s.

### Spawn

`--name` is the `.worktrees/<name>` folder and the default rail label.
`--branch` defaults to that name; `--base-branch` defaults to `HEAD`.
`--title` is the rail label. An empty `--title`, or a title equal to
`--name`, falls back to the folder slug; the slug is not prefixed onto a
custom title.

Spawn runs in the background: the new row appears, and the rail and grid
stay on the caller. `--select` jumps to the new workspace (the same as
**+** or ⌘T in the UI). `workspace.exec` does not select. When you spawn
from a Workspaces pane, the parent defaults to `$SOLA_PANE_ID`.

`--agent` is `grok`, `codex`, or `claude`. Other names are an error
(OpenCode is presence-only). `--prompt` without `--agent` implies Grok.
`--prompt` and `--prompt-file` are exclusive. The first Codex session may
need `/hooks` in its TUI to trust the Sola status hook. Claude hooks are
merged into `~/.claude/settings.json` and apply to the next Claude
session.

### Root tabs

`--root` opens the tab in the project root checkout instead of a new
worktree (the **No worktree** box on the New workspace card). Its `kind`
is `root`, its `path` is the project root, and work lands on whatever
branch the root has checked out (usually master). `--branch` and
`--base-branch` are refused with `--root`, and the startup script does not
run.

A root tab closes with plain `workspace.rm` (`--worktree` is refused).
`workspace.set` takes `--title` and `--before` on a root tab; `--name` and
`--branch` need a worktree. Hover × and the pencil work as on a worktree
tab.

### Rename and reorder

`workspace.set --name` runs `git worktree move` to `.worktrees/<name>`.
The id stays, so tmux sessions keep working; live or dirty checkouts are
moved with force. The project root cannot be renamed. `--branch` runs
`git branch -m` in that checkout and does not move the folder. To promote
an ad hoc tab, pass both:
`--name sc-1234 --branch joshua/sc-1234/fix --title '…'`.

Target a workspace by id, slug, path, or rail label. `--before` reorders
that workspace within its project (`end` = last); the rail does the same
with drag and drop. On a worktree tab, the hover pencil or a double-click
sets `--title` (the label, not the folder). `project.reorder` moves a
project group: `--before` another project, or omit it (or pass `end`) for
last.

### Lists, startup scripts, and sessions

Lists include `path`, `kind`, and `parent`. `kind` is one of:

- `main` — the project's own tab
- `worktree`
- `root` — an extra tab in the project root
- `folder` — a non-git project

`project.startup` is the per-project script that runs in a new worktree
after spawn (also **Project → Startup Script…**). It sees `$PROJECT` (the
project folder on disk), `$WORKTREE` (this tab, `.worktrees/<name>`), and
`$NAME` (the tab name).

`pane.list` and `whoami` include `session_id` when that pane’s Grok or
Claude owner session is known. Use it with `grok -r` or `claude --resume`
after a reboot loses tmux.

### Panes

A workspace name prefers the Grok leaf when sending, reading, waiting, or
exec-ing (the Codex leaf with `--agent codex`, the Claude leaf with
`--agent claude`). `pane.send`, `pane.read`, and `workspace.exec --prompt`
address that workspace’s tmux session exactly (`=session:` current pane),
so a split leaf `…-p` is not confused with its parent.

`pane.send` and `workspace.exec --prompt` **paste** into the live agent
(tmux bracketed paste, then Enter), so a multiline brief does not submit
on the first newline and a long prompt is not truncated. A failed paste
returns the tmux error, not a bare `send failed`.

`pane.wait` holds until the status matches; `--fresh` waits for a
transition.

### Remove

`workspace.rm` unregisters the tab. It does not run `git worktree remove`
unless you pass `--worktree`; add `--force` to discard a dirty checkout.

`workspace.rm` replies first, then drops the tab on the next tick, so a
call from inside that pane can finish instead of hanging.
`tmux kill-session` and `git worktree remove` then run off the UI thread,
and the rest of Workspaces stays live.

Do **not** run `git worktree remove` yourself from inside that pane: the
working directory vanishes and the next tool cannot run. Use `--worktree`
instead. If the checkout is already gone, Workspaces reaps the tab, with no
working spinner left behind.

## Browser (`solactl browser`)

Needs the **Browser** (`sola-browser`) running (owner `browser`). Fails if
the Browser or `sola-call` is down; it does not launch a window. The tab strip is the same
one the human uses; an **Agent** group is an ordinary group that a skill
may create.

```text
solactl browser                         # list methods
solactl browser tabs
solactl browser tab.open --url https://example.com
solactl browser tab.open --url https://example.com --select
solactl browser tab.focus --tab 3
solactl browser tab.close --tab 3
solactl browser tab.move --tab 3 --group Agent
solactl browser group.create --tab 3 --name Agent
solactl browser group.list
solactl browser goto --url https://example.com/path --tab 3
solactl browser snapshot [--tab 3] [--interactive] [--ref e8]
solactl browser find --text Submit
solactl browser click --ref e12
solactl browser click --tab 3 --text "Pay now"
solactl browser click --tab 3 --x 120 --y 80
solactl browser fill --ref e5 --text user@example.com
solactl browser fill --tab 3 --control lyrics --file /path/lyrics.txt
solactl browser get --tab 3 --control lyrics
solactl browser links --tab 3
solactl browser type --ref e5 --text more --submit
solactl browser hover --ref e12
solactl browser hover --tab 3 --x 120 --y 80
solactl browser key --tab 3 --chord Return
solactl browser key --tab 3 --chord Control+Enter
solactl browser scroll --tab 3 --dy 400
solactl browser select --ref e9 --values OptionA,OptionB
solactl browser wait [--load] [--text done] [--control Create] [--timeout 30]
solactl browser screenshot --tab 3 [-o PATH]
solactl browser back|forward|reload|stop [--tab 3]
solactl browser find.page --text needle
```

`--tab` is an id, or a unique URL or title substring. `--group` is an id or
a name. `tab.open` appends a tab and does **not** focus it unless you pass
`--select`.

### Background tabs

Snapshot, click, fill, type, and `browser screenshot` target **that tab’s
CEF document**. They work when the tab is in the background and do not
raise it.

### Snapshots and refs

Page verbs use a pruned accessibility snapshot in YAML, with opaque refs
(`e12`). The snapshot starts with an `actions:` list (role, name, ref);
click those. `click --text "Pay now"` uses the last snapshot and fails
with the candidate refs when more than one control matches. A stale ref
fails (the node is gone); take a new snapshot.

`find` searches the last snapshot, not the page. `find.page` is ⌘F.

### Input

`click` and `hover` also take CSS-pixel `--x` `--y` in **that tab’s**
viewport. That needs no ref, and it is still CEF `Input.dispatch*`, not the
compositor seat.

`fill` and `type` write a contenteditable or ProseMirror editor with one
`paste` of the whole string (newlines kept); they do not assign
`textContent`. `fill --file PATH` reads the string from disk. The reply
includes `value` and `matched: true`, or an error if the field did not
keep the text.

`key --chord` (for example `Return`, `Control+Enter`) inserts once: one
`char` event, not keyDown-with-text plus char. `scroll --dx --dy` go to the
same tab.

### Reading the page

`get` reads the current value or href of a ref or a `--control` name (from
the DOM aria-label if the accessibility tree misses it). `links` lists
`<a href>` targets and audio/video `duration` from the DOM.

Screenshot is a fallback when the tree is empty (canvas, maps). It
captures **that tab’s** document even when the tab is in the background.

### Waiting

`wait` and `wait --load` return when `document.readyState` is `complete`
on a committed URL (not when the tab-strip spinner stops). `wait --text`
snapshots until that string appears. `wait --control Create` snapshots
until that named control is in the actions list, or times out naming the
missing control; it does not succeed just because CEF fired load.

### Bots

With `SOLA_BOT=1` (named informational bots):

- `--select` and `tab.focus` are refused.
- Page verbs require `--tab`; they never default to the focused tab.
- `solactl compositor input`, `screenshot`, and `sample` are refused.

The web path for a bot is `solactl browser` only.

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

Every page call (`status`, `share`, `view`, `wait`, `reload`, `screenshot`,
`pdf`, `inspect`) takes `--canvas`: the canvas folder, its `canvas.toml`, or
a file in it, opened in the background if need be. Without it, the call
goes to the canvas on the human's screen. Over MCP, a page call without
`canvas` goes to the canvas that agent last opened or made, so agents
working at the same time stay on their own.

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
media, backgrounds, no margins). Default paths are under
`$XDG_RUNTIME_DIR/sola/design/shots/`.

`wait` and `reload` also wait for a `height = "auto"` artboard to be
measured.

`inspect` replies with the viewport (the artboard size), the document size,
`cut_off_below` (content taller than the artboard), the element's box and
computed styles, and up to 25 `issues` (`overflows-viewport`, `clipped-x`,
`clipped-y`). `--all` instead lists every match (up to 300) as
`{selector, tag, rect, text}`.

In `status`, each artboard has `src` (what `canvas.toml` asks for), `url`
(what its tab shows), `error` (missing `src`, HTTP ≥ 400), and `detail`
(the part re-rendered at screen scale while the human is zoomed in, else
null).

`view` moves the human's camera only until they move it themselves on that
canvas; after that it replies `moved: false` and leaves it.

Boards are served over http from the project root (`/` = the project
folder; a canvas `root` is looked in first), not `file://`. There is no raw
`eval` on this plane.

## MCP (`solactl mcp`)

An MCP server on stdio that exposes the call plane to agents that speak
MCP. Every method a running app advertises is a tool named
`<owner>_<method>` (dots become `_`): `design_screenshot`,
`browser_tab_open`, `workspaces_pane_send`, … The list follows the apps
that are up: start Design and its tools appear (the server sends
`tools/list_changed`). Each tool takes the same arguments as the
`solactl <owner> <method>` flags; path arguments may be relative to the
agent's working directory. A reply with a `path` to a PNG or JPEG
(screenshots) also returns the image inline, up to about 3.7 MB; larger
ones return the path only.

```text
solactl mcp                      # every live owner
solactl mcp --owner design       # only Design (repeat --owner for more)
claude mcp add sola -- /opt/sola/bin/solactl mcp
```

It is an adapter over the same methods, not a second protocol: what a tool
may do is what `solactl` may do. Which methods need a human confirm is
still open
([D3](../open-questions.md#d3--which-call-plane-methods-need-a-human-confirm-p1)).
`SOLA_BOT=1` rules apply the same way.

## Other commands

These do not go through `sola-call`.

```text
solactl emit <Topic> '<json>'   # bus poke
solactl logs [app] [-f]         # /opt/sola/log
solactl open <url|path>         # sola-browser (URL / HTML / PDF) or sola-paint (image path)
solactl media <action>          # MPRIS / wpctl (shell key handler)
```

`solactl open` calls `sola_core::open_url` (or `open_image` for a raster
path). It does not go through MIME. In a Sola terminal, `open` is an alias
for `xdg-open`; that path uses `sola-browser.desktop` (http(s), HTML,
XHTML, PDF, `about:`, unknown schemes). For those, both paths land in the
Browser. A local HTML or PDF path is resolved to an absolute
`file://` URL before handoff. There is no fallback to another browser.

Screenshot and synthetic input are calls, not bus topics. There is no
`eval` command.
