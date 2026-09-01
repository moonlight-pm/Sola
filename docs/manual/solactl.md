# solactl

Operator CLI. Talks to **sola-call** for compositor/session verbs, and to
the **bus** only for `emit`.

## Call plane (needs `sola-call` + the owner process)

```text
solactl compositor screenshot [-o PATH] [--app APP] [--window TITLE] [--format png|rgba]
solactl compositor sample [--size N]
solactl compositor windows
solactl compositor input click|move|scroll|key …
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

`workspaces` is a first-class subcommand (`solactl` / `solactl --help`).
Other running apps that have advertised methods: `solactl <app-id>` lists
them; `solactl <app-id> <method> …` invokes.

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
    [--agent grok] [--prompt '…' | --prompt-file FILE] [--parent …] [--select]
solactl workspaces workspace.set --workspace ticket-123 --title 'fix login'
solactl workspaces workspace.exec --workspace ticket-123 [--prompt '…']
solactl workspaces workspace.select --workspace ticket-123
solactl workspaces workspace.rm --workspace ticket-123
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

Lists include `path`, `kind`, and `parent`. `project.startup` is the
per-project script that runs in a new worktree after spawn (also
**Project → Startup Script…**). Env: `$PROJECT` (folder on disk),
`$WORKTREE` (this tab, `.worktrees/<name>`), `$NAME` (tab name).
A workspace name prefers the
Grok leaf when sending, reading, waiting, or exec-ing. `--prompt` implies
Grok. `--prompt` and `--prompt-file` are exclusive. Spawn parent defaults
to `$SOLA_PANE_ID` when you run from a Workspaces pane. `--agent` is Grok
only. `pane.wait` holds until status matches (`--fresh` waits for a
transition). Drop unregisters; it does not `git worktree remove`.
`workspace.rm` replies, then closes the tab on the next tick, so a
call from inside that pane can finish instead of hanging.

Bool flags (`--enter`, `--fresh`, `--select`) can sit before other flags. Spawn /
add / wait use a longer call deadline than the default 8s.

## Not calls

```text
solactl emit <Topic> '<json>'   # bus poke
solactl logs [app] [-f]         # /opt/sola/log
solactl open <url|path>         # sola-browser (URL / HTML) or sola-paint (image path)
solactl media <action>          # MPRIS / wpctl (shell key handler)
```

`solactl open` calls `sola_core::open_url` (or `open_image` for a raster
path). It does not go through MIME. In a Sola terminal, `open` is an alias
for `xdg-open`; that path uses `sola-browser.desktop` (http(s), HTML,
XHTML, `about:`, unknown schemes). Both land in sola-browser. There is no
Helium fallback.

`eval` is gone (WebView stack retired). Screenshot and synthetic input
are calls, not bus topics.
