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

`workspaces`, `browser`, and `bots` are first-class subcommands (`solactl` /
`solactl --help`). Other running apps that have advertised methods:
`solactl <app-id>` lists them; `solactl <app-id> <method> …` invokes.

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
The TLS proxy must not buffer the body (`proxy_buffering off`, honor
`X-Accel-Buffering: no`) and should use HTTP/1.1+ to the daemon with a
long `proxy_read_timeout`.

Homes are `~/Bots/<slug>/`. `new` seeds the home and starts a first turn
(introduction; the orientation prompt is not shown as a user message).
`rm` deletes the catalog row, the home directory, and the Grok session.
Not a coding agent; not Workspaces.

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
solactl workspaces workspace.list [--project Sola]
solactl workspaces workspace.spawn --project Sola --name ticket-123 \
    [--branch joshua/sc-1234/fix] [--base-branch origin/dev] [--title 'fix login'] \
    [--agent grok|codex] [--prompt '…' | --prompt-file FILE] [--parent …] [--select]
solactl workspaces workspace.set --workspace ticket-123 --title 'fix login'
solactl workspaces workspace.set --workspace adhoc --name sc-1234 \
    [--title 'fix login'] [--branch joshua/sc-1234/fix]
solactl workspaces workspace.exec --workspace ticket-123 [--agent grok|codex] [--prompt '…']
solactl workspaces workspace.select --workspace ticket-123
solactl workspaces workspace.rm --workspace ticket-123 [--worktree] [--force]
solactl workspaces pane.list [--workspace ticket-123]
solactl workspaces pane.send --text 'follow up' --enter [--pane ticket-123]
solactl workspaces pane.read [--pane ticket-123] [--lines 40]
solactl workspaces pane.wait [--pane ticket-123] [--status done] [--timeout 300] [--fresh]
solactl workspaces whoami                  # from a Workspaces pane; or --pane / --path
```

`--name` is the rail slug and `.worktrees/<name>` folder. `--branch`
defaults to that name; `--base-branch` defaults to HEAD. `--title` is a
rail subtitle (`sc-1234 · fix login`). Spawn is background: the new
row appears, the rail/grid stay on the caller. `--select` jumps
(same as the UI + / ⌘T). `workspace.exec` does not select.
`workspace.set --name` slugs the rail label and `git worktree move`s
to `.worktrees/<name>` (id stays so tmux sessions keep working; live
or dirty checkouts are forced). The project root cannot be renamed.
`--branch` is `git branch -m` in that checkout and does not move the
folder. Promote an ad hoc tab with both: `--name sc-1234 --branch
joshua/sc-1234/fix --title '…'`. Target by id if the old slug is gone.

Lists include `path`, `kind`, and `parent`. `project.startup` is the
per-project script that runs in a new worktree after spawn (also
**Project → Startup Script…**). Env: `$PROJECT` (folder on disk),
`$WORKTREE` (this tab, `.worktrees/<name>`), `$NAME` (tab name).
`pane.list` and `whoami` include `session_id` when that pane’s Grok
owner session is known (used to `grok -r` after a reboot loses tmux).
A workspace name prefers the
Grok leaf when sending, reading, waiting, or exec-ing (Codex leaf when
`--agent codex`). `pane.send`, `pane.read`, and
`workspace.exec --prompt` address that workspace’s tmux session exactly
(`=session:` current pane) so a split leaf `…-p` is not confused with
the parent. `pane.send` and
`workspace.exec --prompt` **paste** into the live agent (tmux
bracketed-paste, then Enter) so a multiline brief does not submit on
the first newline and a long prompt is not truncated. A failed paste
returns the tmux error (not a bare `send failed`). `--prompt` without
`--agent` implies Grok. `--prompt` and `--prompt-file` are exclusive. Spawn
parent defaults to `$SOLA_PANE_ID` when you run from a Workspaces pane.
`--agent` is `grok` or `codex`. First Codex session may need `/hooks` in
the TUI to trust the Sola status hook. `pane.wait` holds until status
matches (`--fresh` waits for a transition). Drop unregisters; it does not
`git worktree remove` unless
you pass `--worktree` (add `--force` to toss a dirty checkout).
`workspace.rm` replies, then closes the tab on the next tick, so a
call from inside that pane can finish instead of hanging. Do **not**
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
use a pruned accessibility YAML snapshot and opaque refs (`e12`). Stale
refs fail; snapshot again. `click` / `hover` also take CSS-pixel `--x`
`--y` in **that tab’s** viewport (no ref; still CEF `Input.dispatch*`,
not the compositor seat). `key --chord` (e.g. `Return`, `Control+Enter`)
and `scroll --dx --dy` go to the same tab. Screenshot is a fallback when
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
solactl browser click --tab 3 --x 120 --y 80
solactl browser fill --ref e5 --text user@example.com
solactl browser type --ref e5 --text more --submit
solactl browser hover --ref e12
solactl browser hover --tab 3 --x 120 --y 80
solactl browser key --tab 3 --chord Return
solactl browser key --tab 3 --chord Control+Enter
solactl browser scroll --tab 3 --dy 400
solactl browser select --ref e9 --values OptionA,OptionB
solactl browser wait [--load] [--text done] [--timeout 30]
solactl browser screenshot --tab 3 [-o PATH]
solactl browser back|forward|reload|stop [--tab 3]
solactl browser find.page --text needle
```

`--tab` is an id, or a unique URL/title substring. `--group` is an id or
name. `find` searches the last snapshot, not the page. `find.page` is ⌘F.
`wait` / `wait --load` returns when `document.readyState` is `complete` on
a committed URL (not the tab-strip spinner). `wait --text` snapshots until
that string appears.

Snapshot, click, fill, type, and `browser screenshot` target **that tab’s
CEF document** — they work when the tab is in the background and do not
raise it. `tab.open` does not focus. Bots (`SOLA_BOT=1`) **must** pass
`--tab`, and are refused `tab.focus`, `tab.open --select`, and compositor
screenshot/input.

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
