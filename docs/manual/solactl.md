# solactl

Operator CLI. Talks to **sola-call** for compositor/session verbs, and to
the **bus** only for `emit`.

## Call plane (needs `sola-call` + the owner process)

```text
solactl compositor screenshot [-o PATH] [--app APP] [--window TITLE]
solactl compositor windows
solactl compositor input click|move|scroll|key …
solactl session launch <app_id> [--command CMD]
solactl session close  <app_id>
```

If `sola-call` or the owner is down, the command fails. It does **not**
launch a window.

Other running apps that have advertised methods: `solactl <app-id>` lists
them; `solactl <app-id> <method> …` invokes.

## Workspaces (`solactl ws`)

Needs **Workspaces** running (owner `ws`). Fails if the app or `sola-call`
is down — it does not launch a window.

```text
solactl ws                         # list methods
solactl ws ps
solactl ws project.list
solactl ws project.add --path ~/Workspace/Sola
solactl ws project.rm --project Sola
solactl ws workspace.list [--project Sola]
solactl ws workspace.spawn --project Sola --name ticket-123 \
    [--agent grok] [--prompt '…' | --prompt-file FILE] [--parent …]
solactl ws workspace.exec --workspace ticket-123 [--prompt '…']
solactl ws workspace.select --workspace ticket-123
solactl ws workspace.rm --workspace ticket-123
solactl ws pane.list [--workspace ticket-123]
solactl ws pane.send --text 'follow up' --enter [--pane ticket-123]
solactl ws pane.read [--pane ticket-123] [--lines 40]
solactl ws pane.wait [--pane ticket-123] [--status done] [--timeout 300] [--fresh]
solactl ws whoami                  # from a Workspaces pane; or --pane / --path
```

Lists include `path`, `kind`, and `parent`. A workspace name prefers the
Grok leaf when sending, reading, waiting, or exec-ing. `--prompt` implies
Grok. `--prompt` and `--prompt-file` are exclusive. Spawn parent defaults
to `$SOLA_PANE_ID` when you run from a Workspaces pane. `--agent` is Grok
only. `pane.wait` holds until status matches (`--fresh` waits for a
transition). Drop unregisters; it does not `git worktree remove`.

Bool flags (`--enter`, `--fresh`) can sit before other flags. Spawn /
add / wait use a longer call deadline than the default 8s.

## Not calls

```text
solactl emit <Topic> '<json>'   # bus poke
solactl logs [app] [-f]         # /opt/sola/log
solactl open <url|path>         # sola-browser (http/https) or sola-paint (image path)
solactl media <action>          # MPRIS / wpctl (shell key handler)
```

`eval` is gone (WebView stack retired). Screenshot and synthetic input
are calls, not bus topics.
