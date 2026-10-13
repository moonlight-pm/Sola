# solactl

Operator CLI. Talks to **sola-call** for compositor/session verbs, and to
the **bus** only for `emit`.

## Call plane (needs `sola-call` + the owner process)

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
(`ext-image-copy-capture`). The window does not need to be on top and
is not raised. `--format rgba` writes packed RGBA8 (no PNG) for the
shell freeze picker. Default PNG uses Fast compression. Shell hotkeys
copy to the clipboard instead of writing this file.

`workspaces`, `browser`, `bots`, and `design` are first-class subcommands
(`solactl` / `solactl --help`). Other running apps that have advertised
methods: `solactl <app-id>` lists them; `solactl <app-id> <method> …`
invokes.

Path arguments (`--path`, `--file`, `--out`, `--prompt-file`, …) are
resolved by `solactl` before the call: `~` expands and a relative path is
taken from **your** current directory, not the owner app's.

## Bots (`solactl bots`)

Needs **sola-botsd** (owner `bots`). Fails if the daemon or `sola-call`
is down — it does not launch a window.

```text
solactl bots                         # list methods
solactl bots list
solactl bots new --name Suno
solactl bots send --bot suno --text 'hello'
solactl bots transcript --bot suno
solactl bots rm --bot suno
solactl bots cancel --bot suno
```

Phone API: daemon binds HTTP `0.0.0.0:27419`. Public name is
`https://bot.sola.computer` (TLS at the proxy). Open **27419/tcp** to the
desk for that proxy:

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

`GET /events` is `text/event-stream` (chunked). Keepalive comments every 15s.
The iced **Bots** window uses the same HTTP/SSE on `127.0.0.1:27419`
(list/send/create/delete + live events). `solactl bots` still uses
sola-call. The TLS proxy must not buffer the body (`proxy_buffering off`,
honor `X-Accel-Buffering: no`) and should use HTTP/1.1+ to the daemon with
a long `proxy_read_timeout`.

Homes are `~/Bots/<slug>/`. `new` seeds the home and starts a first turn
(introduction; the orientation prompt is not shown as a user message).
`rm` deletes the catalog row, the home directory, and the Grok session.
Write fence is prompt-only: **default that home**; say “drop the write
fence” / “work on the whole computer” / name a path outside the home to
open it for that task. Seat steal is still refused. Not a coding agent;
not Workspaces unless you ask for that rail.

## Workspaces (`solactl workspaces`)

Needs **Workspaces** running (owner `workspaces`). Fails if the app or `sola-call`
is down — it does not launch a window.

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

`--name` is the `.worktrees/<name>` folder (and the rail **default**).
`--branch` defaults to that name; `--base-branch` defaults to HEAD.
`--title` is the rail label. Empty `--title` (or a title equal to
`--name`) falls back to the folder slug — the slug is not prefixed onto
a custom title. Spawn is background: the new row appears, the rail/grid
stay on the caller. `--select` jumps (same as the UI + / ⌘T).
`workspace.exec` does not select.
`--root` opens the tab in the project root checkout instead of a new
worktree (the **No worktree** box on the New workspace card): `kind` is
`root`, `path` is the project root, and work lands on whatever branch the
root has checked out (usually master). `--branch` / `--base-branch` are
refused with `--root`, and the startup script does not run. A root tab
closes with plain `workspace.rm` (`--worktree` is refused);
`workspace.set` takes `--title` and `--before` (`--name` / `--branch` need
a worktree). Hover × and the pencil work as on a worktree tab.
`workspace.set --name` `git worktree move`s to `.worktrees/<name>` (id
stays so tmux sessions keep working; live or dirty checkouts are
forced). The project root cannot be renamed.
`--branch` is `git branch -m` in that checkout and does not move the
folder. Promote an ad hoc tab with both: `--name sc-1234 --branch
joshua/sc-1234/fix --title '…'`. Target by id, slug, path, or rail
label. `--before` reorders that workspace in its project (`end` = last).
The rail also does this with drag-drop; hover pencil / double-click on a
worktree tab is `--title` (the label, not the folder). `project.reorder`
moves a project group (`--before` another project, or omit / `end` for last).

Lists include `path`, `kind` (`main` = the project's own tab,
`worktree`, `root` = extra tab in the project root, `folder` = non-git
project), and `parent`. `project.startup` is the
per-project script that runs in a new worktree after spawn (also
**Project → Startup Script…**). Env: `$PROJECT` (folder on disk),
`$WORKTREE` (this tab, `.worktrees/<name>`), `$NAME` (tab name).
`pane.list` and `whoami` include `session_id` when that pane’s Grok
or Claude owner session is known (used to `grok -r` or `claude --resume`
after a reboot loses tmux).
A workspace name prefers the
Grok leaf when sending, reading, waiting, or exec-ing (Codex leaf when
`--agent codex`, Claude leaf when `--agent claude`). `pane.send`, `pane.read`, and
`workspace.exec --prompt` address that workspace’s tmux session exactly
(`=session:` current pane) so a split leaf `…-p` is not confused with
the parent. `pane.send` and
`workspace.exec --prompt` **paste** into the live agent (tmux
bracketed-paste, then Enter) so a multiline brief does not submit on
the first newline and a long prompt is not truncated. A failed paste
returns the tmux error (not a bare `send failed`). `--prompt` without
`--agent` implies Grok. `--prompt` and `--prompt-file` are exclusive. Spawn
parent defaults to `$SOLA_PANE_ID` when you run from a Workspaces pane.
`--agent` is `grok`, `codex`, or `claude`. Other names error (OpenCode
stays presence-only). First Codex session may need `/hooks` in
the TUI to trust the Sola status hook. Claude hooks are merged into
`~/.claude/settings.json` and apply to the next Claude session. `pane.wait` holds until status
matches (`--fresh` waits for a transition). Drop unregisters; it does not
`git worktree remove` unless
you pass `--worktree` (add `--force` to toss a dirty checkout).
`workspace.rm` replies, then drops the tab on the next tick, so a
call from inside that pane can finish instead of hanging. `tmux
kill-session` and `git worktree remove` run off the iced thread after
that (the rest of Workspaces stays live). Do **not**
`git worktree remove` first from inside that pane — the cwd vanishes and
the next tool cannot run. Use `--worktree` instead. If the checkout is
already gone, Workspaces reaps the tab (no leftover working spinner).

Bool flags (`--enter`, `--fresh`, `--select`) can sit before other flags. Spawn /
add / wait use a longer call deadline than the default 8s.

## Browser (`solactl browser`)

Needs **sola-browser** running (owner `browser`). Fails if chrome or
`sola-call` is down — it does not launch a window. Same tab strip as
the human; an **Agent** group is an ordinary group a skill may create.

`tab.open` appends and does **not** focus unless `--select`. Page verbs
use a pruned accessibility YAML snapshot and opaque refs (`e12`). The
snapshot starts with an `actions:` list (role, name, ref) — click those.
`click --text "Pay now"` uses the last snapshot and fails with the
candidate refs when more than one control matches. Stale refs fail
(node gone); snapshot again. `click` / `hover` also take CSS-pixel `--x`
`--y` in **that tab’s** viewport (no ref; still CEF `Input.dispatch*`,
not the compositor seat). `fill` / `type` write a contenteditable /
ProseMirror editor with one `paste` of the whole string (newlines kept);
they do not assign `textContent`. `fill --file PATH` reads the string from
disk. The reply includes `value` and `matched: true`, or an error if the
field did not keep the text. `get` reads the current value/href of a ref
or `--control` name (DOM aria-label if the AX tree misses it). `links`
lists `<a href>` and audio/video `duration` from the DOM. `wait --control
Create` succeeds only when that named control exists in a snapshot (not
when CEF fires load). `key --chord` (e.g. `Return`, `Control+Enter`)
inserts once (one `char` event; not keyDown-with-text plus char).
`scroll --dx --dy` go to the same tab. If `tabs` / `tab.open` time out
while the sola-browser window is still up, the call plane is wedged:
close the window (X or Flower) and launch it again — do not kill the
process from a bot. Screenshot is a fallback when
the tree is empty (canvas / maps) and captures **that tab’s** document
even when backgrounded. Vault fill / confirm gates are not on this plane
(**D3**).

With `SOLA_BOT=1` (named informational bots): `--select` and `tab.focus`
are refused; page verbs require `--tab` (never default to the focused
tab); `solactl compositor input` / `screenshot` / `sample` are refused.
The web path is `solactl browser` only.

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

`--tab` is an id, or a unique URL/title substring. `--group` is an id or
name. `find` searches the last snapshot, not the page. `find.page` is ⌘F.
`wait` / `wait --load` returns when `document.readyState` is `complete` on
a committed URL (not the tab-strip spinner). `wait --text` snapshots until
that string appears. `wait --control Create` snapshots until that named
control is in the actions list (or times out naming the missing control).

Snapshot, click, fill, type, and `browser screenshot` target **that tab’s
CEF document** — they work when the tab is in the background and do not
raise it. `tab.open` does not focus. Bots (`SOLA_BOT=1`) **must** pass
`--tab`, and are refused `tab.focus`, `tab.open --select`, and compositor
screenshot/input.

## Design (`solactl design`)

Needs **sola-design** running (owner `design`). Fails if it or `sola-call`
is down — it does not launch a window. Verbs act on the canvas the human
sees; `open` switches it.

A canvas is files, not app state: `<project>/design/<canvas>/canvas.toml`
lists the artboards and each artboard is an HTML page beside it. Edit those
files with ordinary tools. Design watches them (and any outside file a page
loaded — `status` lists those as `linked`) and reloads. After an edit, `wait`
or `reload` before `screenshot` / `inspect`: both make the boards show the
files on disk (`wait` reloads when files are newer than the boards) and
return once pages have loaded, web fonts are in, and `auto` heights are
measured.

`canvas.new NAME` makes `<project>/design/<slug>/canvas.toml` (project =
`--path`, default your current directory), registers the project if
needed, and shows the canvas — it appears under the project in the rail.
An existing canvas of that name is opened instead (`created: false`).
`open` takes the canvas dir, its `canvas.toml`, or any file inside it, and
registers the project if Design does not know it yet. It does not raise the
window unless `--raise`. `--artboard` defaults to the artboard holding the
human's selection, else the only artboard. `inspect` without `--selector`
inspects the selected element. `selection` is what the human Super+clicked
(`null` when nothing).

```text
solactl design                               # list methods
solactl design project.list                  # projects + canvases
solactl design project.add --path ~/Workspace/Thoxa
solactl design project.remove --path ~/Workspace/Thoxa  # forget it: canvases leave the rail and close; files stay
solactl design canvas.new onboarding         # design/onboarding/ in your cwd's project
solactl design canvas.new "Settings v2" --path ~/Workspace/Thoxa --raise
solactl design open design/onboarding        # relative to your cwd
solactl design open design/onboarding/canvas.toml --raise
solactl design status [--canvas design/home] # canvas (shown, share), artboards (src, url, error), linked files, zoom, selection
solactl design share [--canvas design/home]  # the canvas's live link
solactl design share.rotate [--canvas …]     # a new link; the old one stops working
solactl design selection
solactl design view --fit
solactl design view --artboard phone [--zoom 1]
solactl design view --zoom 0.5
solactl design wait [--artboard phone] [--timeout 15]
solactl design reload [--artboard phone] [--timeout 15]
solactl design screenshot [--artboard phone] [-o shot.png]
solactl design pdf [--artboard landing] [-o landing.pdf]
solactl design inspect [--artboard phone] [--selector 'main .card']
solactl design inspect --artboard light --selector section --all   # every match's box
```

`open` and `canvas.new` make a canvas live **off the human's screen** (it
shows in their rail); `--raise` puts it on their screen. Every page call
(`status`, `share`, `view`, `wait`, `reload`, `screenshot`, `pdf`, `inspect`)
takes `--canvas` (its folder, `canvas.toml`, or a file in it — opened in the
background if need be); without it the call goes to the canvas on the human's
screen. Over MCP, a page call without `canvas` goes to the canvas that agent
last opened or made, so agents working at the same time stay on their own.

`screenshot` captures the whole page (CDP, at the artboard's raster scale,
normally 2×) and replies with `path` — the same however the human is zoomed
or panned; `pdf` writes a one-page PDF of
the artboard as it looks on screen (screen media, backgrounds, no margins).
Default paths are under `$XDG_RUNTIME_DIR/sola/design/shots/`. `wait` and
`reload` also wait for a `height = "auto"` artboard to be measured. `inspect` replies with the viewport
(the artboard size), the document size, `cut_off_below` (content taller than
the artboard), the element's box and computed styles, and up to 25
`issues` (`overflows-viewport`, `clipped-x`, `clipped-y`). `--all` instead
lists every match (up to 300) as `{selector, tag, rect, text}`. In
`status`, each artboard has `src` (what `canvas.toml` asks for), `url` (what
its tab shows), `error` (missing `src`, HTTP ≥ 400), and `detail` (the part
re-rendered at screen scale while the human is zoomed in, else null).
`screenshot` reports the `scale` it used: each artboard renders at 2×, else
1×, else under 1× for a page over 8192 px. `view` moves the human's camera
only until they move it themselves on that canvas; after that it replies
`moved: false` and leaves it. Boards are served over http from the project
root (`/` = project folder; a canvas `root` is looked in first), not
`file://`. There is no raw `eval` on this plane.

## MCP (`solactl mcp`)

An MCP server on stdio that exposes the call plane to agents that speak
MCP. Every method a running app advertises is a tool named
`<owner>_<method>` (dots become `_`): `design_screenshot`,
`browser_tab_open`, `workspaces_pane_send`, … The list follows the apps
that are up — start Design and its tools appear (the server sends
`tools/list_changed`). Each tool takes the same arguments as the
`solactl <owner> <method>` flags; path arguments may be relative to the
agent's working directory. A reply with a `path` to a PNG / JPEG
(screenshots) also returns the image inline (up to ~3.7 MB; larger ones
return the path only).

```text
solactl mcp                      # every live owner
solactl mcp --owner design       # only Design (repeat --owner for more)
claude mcp add sola -- /opt/sola/bin/solactl mcp
```

It is an adapter over the same methods, not a second protocol: what a tool
may do is what `solactl` may do (confirm policy is still **D3**).
`SOLA_BOT=1` rules apply the same way.

## Not calls

```text
solactl emit <Topic> '<json>'   # bus poke
solactl logs [app] [-f]         # /opt/sola/log
solactl open <url|path>         # sola-browser (URL / HTML / PDF) or sola-paint (image path)
solactl media <action>          # MPRIS / wpctl (shell key handler)
```

`solactl open` calls `sola_core::open_url` (or `open_image` for a raster
path). It does not go through MIME. In a Sola terminal, `open` is an alias
for `xdg-open`; that path uses `sola-browser.desktop` (http(s), HTML,
XHTML, PDF, `about:`, unknown schemes). Both land in sola-browser. A local
HTML or PDF path is resolved to an absolute `file://` URL before handoff.
There is no Helium fallback.

`eval` is gone (WebView stack retired). Screenshot and synthetic input
are calls, not bus topics.
