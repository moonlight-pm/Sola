---
name: sola-workspaces-cli
description: >
  Drive sola-workspaces from solactl. Use when the user says review ticket,
  work ticket, implement ticket, fan out, create worktree, spawn sibling,
  new workspace, tell that worktree, tell grok in X, brief a workspace,
  send to a pane, or solactl workspaces. Default is background — never
  steal the rail. /sola-workspaces-cli
---

# sola-workspaces CLI

Workspaces is sola-call owner **`workspaces`**. Face is `solactl workspaces …`.
The app must be up — the command fails if it is not (it will not launch a
window).

Contract: `docs/specs/2026-08-18-workspaces-cli-design.md`.  
Operator list: `docs/manual/solactl.md`. Do not copy those here.

You are usually the **root** Grok. Fan-out means a **sibling worktree +
pane**, not you leaving this checkout, and not `git worktree add`.

## Rail (locked)

CLI spawn is **background**. The new row appears; this pane and the rail
stay put.

- Do **not** pass `--select` unless the user asks to jump / switch / open /
  show that workspace.
- `workspace.exec`, `pane.send`, `pane.read`, `pane.wait` never select.
- `workspace.select` is the only dedicated interrupt.
- Do not invert with `--background` / `--no-select`. Quiet is the default.

## Intent → verb

Resolve project/parent with `whoami` (or `ps`) when you are in a Workspaces
pane. `--name` is the rail slug and `.worktrees/<name>`.

| User says | Do |
|---|---|
| review / work / implement / look at **ticket** *N*; fan out this work | If that workspace already exists → `workspace.exec --prompt`. Else **spawn** grok + `--prompt`. **No `--select`.** Do not wait unless they want a report. |
| create worktree / spawn sibling / new workspace | `workspace.spawn`. Prompt only if they gave a brief. **No `--select`.** |
| tell *name* / tell that grok / send to *name* / brief *name* | `workspace.exec --prompt` (or `pane.send --enter` for a follow-up line). Do not spawn a second row. Do not select. |
| when it's done / wait / report back | `pane.wait --status done` (add `--fresh` if it may already be done) |
| what's on that pane / read *name* | `pane.read` |
| jump to / switch to / show me / open *name* | `workspace.select`, or spawn with `--select` if they are creating *and* want to land there |
| drop / close that workspace | `workspace.rm` — unregister + kill tmux, **not** `git worktree remove` |

## Fan-out a ticket (stay here)

```bash
solactl workspaces whoami
solactl workspaces workspace.list --project PROJECT

# exists:
solactl workspaces workspace.exec --workspace SLUG --prompt '…'

# new (no --select):
solactl workspaces workspace.spawn --project PROJECT --name SLUG --agent grok \
  --prompt '…'
# optional: --base-branch origin/dev --branch joshua/sc-1234/fix --title '…'
# long brief: --prompt-file /tmp/brief.md  (not both)
```

Then **keep talking in this pane**. Do not `pane.wait` unless they asked.
Do not `workspace.select`.

`--prompt` implies grok. Only `--agent grok` is allowed. Spawn parent
defaults from `$SOLA_PANE_ID`. Do not fetch/checkout after spawn to “fix”
the branch — pass `--branch` / `--base-branch` on spawn.

## Talk to an existing row

```bash
solactl workspaces workspace.exec --workspace SLUG --prompt 'also check X'
solactl workspaces pane.send --pane SLUG --text 'also check X' --enter
solactl workspaces pane.read --pane SLUG --lines 80
solactl workspaces pane.wait --pane SLUG --status done --timeout 300
```

A workspace name prefers the **Grok** leaf. Pass a pane id from `pane.list`
to pin a split.

## Jump (only if they asked)

```bash
solactl workspaces workspace.select --workspace SLUG
solactl workspaces workspace.spawn --project PROJECT --name SLUG --select
```

## Do not

- Steal the rail (`--select`, `workspace.select`) on a ticket/fan-out/create
- `git worktree add` / `git worktree remove` as the product verb
- `--agent claude` (rejected; presence-only)
- Build a mailbox / ask-reply / `worker_done`
- Call `sat` (there is no such binary)
- Wait out a sibling unless they want a report back
