---
name: sola-workspaces-cli
description: >
  Drive sola-workspaces from solactl (list/spawn/exec/send/read/wait).
  Use when the user is in Workspaces, wants a sibling worktree, or says
  solactl ws, spawn workspace, brief grok, or fan out a ticket.
---

# sola-workspaces CLI

Workspaces is running as sola-call owner **`ws`**. The face is
`solactl ws …`. The app must be up — the command fails if it is not
(it will not launch a window).

Full contract: `docs/specs/2026-08-18-workspaces-cli-design.md`.
Operator list: `docs/manual/solactl.md`.

## Discover

```bash
solactl ws              # methods
solactl ws whoami       # this pane (needs $SOLA_PANE_ID or --pane/--path)
solactl ws ps
```

## Fan out a ticket (usual loop)

From the project **root** session:

```bash
solactl ws workspace.list --project Sola
solactl ws workspace.spawn --project Sola --name ticket-123 --agent grok \
  --prompt 'Look at ticket 123 and implement …'
# or --prompt-file /tmp/brief.md

solactl ws pane.wait --pane ticket-123 --status done --timeout 300
solactl ws pane.read --pane ticket-123 --lines 80
solactl ws pane.send --pane ticket-123 --text 'also check X' --enter
solactl ws pane.wait --pane ticket-123 --status done --fresh
```

Spawn parent defaults to `$SOLA_PANE_ID` when you are already in a
Workspaces pane, so a sibling nests under the caller.

`--prompt` and `--prompt-file` are exclusive. `--prompt` implies grok.
Only `--agent grok` is allowed.

## Existing checkout

```bash
solactl ws workspace.exec --workspace ticket-123 --prompt 'continue'
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
