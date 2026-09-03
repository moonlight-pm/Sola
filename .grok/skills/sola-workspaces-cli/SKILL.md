---
name: sola-workspaces-cli
description: >
  Drive sola-workspaces from solactl. Use when the user says review ticket,
  work ticket, implement ticket, fan out, create worktree, spawn sibling,
  new workspace, tell that worktree, tell grok in X, brief a workspace,
  send to a pane, clean up this worktree, merge and clean up, remove this
  worktree, or solactl workspaces. Default is background — never steal
  the rail. /sola-workspaces-cli
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
| clean up this worktree / merge and clean up this worktree | Merge to master, then remove the git worktree **and** close the tab (below). |
| remove this worktree, don't merge / toss this worktree | Do **not** merge. Remove the git worktree **and** close the tab. |
| drop / close that workspace | `workspace.rm` only — leave the git worktree |

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

Long or multiline briefs: `--prompt-file`, not a giant `--prompt`. Send is
a paste into Grok (then Enter). Always pass `--enter` on `pane.send` when
you mean submit — without it the text sits in the composer.

A workspace name prefers the **Grok** leaf. Pass a pane id from `pane.list`
to pin a split.

## Jump (only if they asked)

```bash
solactl workspaces workspace.select --workspace SLUG
solactl workspaces workspace.spawn --project PROJECT --name SLUG --select
```

## Merge / cleanup

Never remove a git worktree or a Workspaces tab unless they asked. Merge /
LGTM / ship it is **not** a request to remove anything. There is no default
cleanup.

`git worktree remove` does not close the tab by itself; plain `workspace.rm`
does not delete the checkout. When they asked to **remove the worktree**,
close the tab **and** pass `--worktree` (the app removes the git checkout
after tmux dies). Do **not** run `git worktree remove` from inside that
pane first — the cwd vanishes, later tools cannot start, and the rail
used to keep a working spinner. If the folder is already gone, Workspaces
reaps the tab on its own.

| They said | Merge | Git worktree + branch | Close tab |
|---|---|---|---|
| merge / merge that / merge to master / ship it / LGTM / nailed it / looks good / perfect | yes | no | no |
| clean up this worktree / merge and clean up this worktree / ship and cleanup | yes | yes | yes |
| remove this worktree, don't merge / toss this worktree | no | yes | yes |
| close the tab / drop the workspace | no | no | yes |
| keep the worktree / keep the tab / hold off / don't drop it | — | no | no |

If merge is in the request and merge fails, stop — do not remove.

### When removing a worktree

Resolve names (`whoami`, then `project.list` / `ps`):

| Var | From |
|---|---|
| `$NAME` | rail slug and `.worktrees/<name>` |
| `$BRANCH` | git (may differ from `$NAME`) |
| `$PROJECT` | project's `root` — not `whoami.path` |

Never `git worktree remove` from this pane (cwd dies; the next tool
cannot start). Merge and delete the branch **while the checkout still
exists**, then one `workspace.rm --worktree`. Same for tab-only drop:
one command, then stop.

```bash
# if merge was requested: commit in the worktree if needed, merge $BRANCH
# into master from the project root, then:
git -C "$PROJECT" branch -d "$BRANCH"
solactl workspaces workspace.rm --workspace "$NAME" --worktree
# toss / discard a dirty checkout:
# solactl workspaces workspace.rm --workspace "$NAME" --worktree --force
```

`--force` only if they said toss / discard. `git branch -d` refuses
unmerged; do not `-D`. Skip the branch line if you are not deleting it.

Tab-only drop is just `solactl workspaces workspace.rm --workspace "$NAME"`
(no `--worktree`). `workspace.rm` replies **before** it kills tmux, so a
foreground call from inside the pane works. After it returns, **stop** —
no more tools in that tab.

Prefer the parent/root pane when it is already there.

## Do not

- Steal the rail (`--select`, `workspace.select`) on a ticket/fan-out/create
- Remove a worktree or tab without being asked (merge/LGTM is not asking)
- Remove a worktree and leave the tab (unless they said keep the tab)
- `git worktree remove` from inside the pane being cleaned up (use
  `workspace.rm --worktree` instead)
- `git worktree add` as the spawn verb (`workspace.spawn` instead)
- `--agent claude` (rejected; presence-only)
- Build a mailbox / ask-reply / `worker_done`
- Call `sat` (there is no such binary)
- Wait out a sibling unless they want a report back
