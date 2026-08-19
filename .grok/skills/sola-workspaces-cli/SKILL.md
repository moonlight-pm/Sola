---
name: sola-workspaces-cli
description: >
  Drive sola-workspaces from solactl (list/spawn/exec/send/read/wait).
  Use when the user is in Workspaces, wants a sibling worktree, or says
  solactl workspaces, spawn workspace, brief grok, or fan out a ticket.
---

# sola-workspaces CLI

Workspaces is running as sola-call owner **`workspaces`**. The face is
`solactl workspaces …`. The app must be up — the command fails if it is not
(it will not launch a window).

Full contract: `docs/specs/2026-08-18-workspaces-cli-design.md`.
Operator list: `docs/manual/solactl.md`.

## Discover

```bash
solactl workspaces              # methods
solactl workspaces whoami       # this pane (needs $SOLA_PANE_ID or --pane/--path)
solactl workspaces ps
```

## Fan out a ticket (usual loop)

From the project **root** session:

```bash
solactl workspaces workspace.list --project Sola
solactl workspaces workspace.spawn --project Sola --name ticket-123 --agent grok \
  --base-branch origin/dev --branch joshua/sc-1234/fix \
  --prompt 'Look at ticket 123 and implement …'
# or --prompt-file /tmp/brief.md

solactl workspaces pane.wait --pane ticket-123 --status done --timeout 300
solactl workspaces pane.read --pane ticket-123 --lines 80
solactl workspaces pane.send --pane ticket-123 --text 'also check X' --enter
solactl workspaces pane.wait --pane ticket-123 --status done --fresh
```

Spawn parent defaults to `$SOLA_PANE_ID` when you are already in a
Workspaces pane, so a sibling nests under the caller.

`--prompt` and `--prompt-file` are exclusive. `--prompt` implies grok.
Only `--agent grok` is allowed.

`--name` is the rail / `.worktrees/` slug. `--branch` is the git branch
(default: same as name). `--base-branch` is the start-point (default:
HEAD). Do **not** fetch/checkout after spawn to fix the branch.

```bash
solactl workspaces workspace.spawn --project Illuno --name sc-1234 \
  --base-branch origin/dev --branch joshua/sc-1234/fix-login
```

A project may have a **startup script** that runs in the new worktree
after spawn (copy `.grok`, etc.):

```bash
solactl workspaces project.startup --project Illuno
solactl workspaces project.startup --project Illuno \
  --script 'cp -a "$PROJECT/.grok" "$WORKTREE/"'
```

Script env (cwd is the new worktree):

| Var | Meaning |
|---|---|
| `PROJECT` | Project folder on disk (the root checkout) |
| `WORKTREE` | This tab — `<project>/.worktrees/<name>` |
| `NAME` | Tab name |

## Existing checkout

```bash
solactl workspaces workspace.exec --workspace ticket-123 --prompt 'continue'
```

Reuses a Grok leaf if one is there; otherwise starts `grok` in the
preferred pane.

## Targeting

A workspace name prefers the **Grok** leaf (then the active leaf).
Pass a pane id from `pane.list` to pin a split.

## Do not

- Build a mailbox / ask-reply / `worker_done` on top of this
- Pass `--agent claude` (rejected)
- Treat `workspace.rm` as `git worktree remove` (it is not)
- Call `sat` (there is no such binary)
