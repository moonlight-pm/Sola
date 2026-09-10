---
name: sync-project
description: >
  Bring this project's git worktrees back into sync with master so drift
  cannot deploy stale binaries. For each sibling worktree, one at a time:
  checkpoint if needed, then merge that branch into master. After every
  merge-up, tell each worktree to merge master back in. Use when the user
  says sync project, /sync-project, sync worktrees, merge all worktrees,
  or wants worktrees in sync before shared-package work.
---

# Sync project (worktrees ↔ master)

Run from the **project-root** Grok (master checkout). This project only.
Leave git worktrees and Workspaces tabs in place. Do not `--select`. Do not
install. Do not push `master` unless the user asked.

Load [`sola-workspaces-cli`](../sola-workspaces-cli/SKILL.md) for exec/wait
verbs. Sibling checkpoints use the user [`checkpoint`](file:///home/joshua/.grok/skills/checkpoint/SKILL.md)
skill — do not nest `grok --cwd`.

## Inventory

```bash
git worktree list
git status -sb
solactl workspaces whoami
solactl workspaces ps
```

Targets = every checkout under `.worktrees/` for **this** repo. Skip the
master checkout you are sitting in. Match rail slug to folder name
(`.worktrees/<name>`).

Per target, record: path, branch, porcelain, `git log --oneline master..BRANCH`,
whether it is already an ancestor of `master`.

## Pass 1 — checkpoint, then merge up (serial)

For **each** target, finish both steps before the next target.

### 1a. Checkpoint (only if needed)

Needed when the worktree is **dirty** (`git status --porcelain` nonempty) or
docs/progress would change under checkpoint (uncommitted product work).
Unique commits already on the branch do **not** need a checkpoint.

If not needed: skip 1a.

If needed: `workspace.exec --workspace SLUG --prompt-file` with a brief that
says, in this order:

1. Load and run `/checkpoint` **only if** there is something to checkpoint
   (dirty tree, uncommitted docs drift, or branch ahead of its upstream
   that should be pushed). If already clean and docs match code: reply
   `checkpoint clean — nothing to do` and stop.
2. Do not merge to master. Do not remove the worktree or tab. Do not install.

Then:

```bash
solactl workspaces pane.wait --pane SLUG --status done --timeout 600 --fresh
```

`--fresh` waits for a transition onto `done` after the brief. If exec fails
(`send failed` / missing tmux pane) and the tree is dirty: **stop that
target** (do not merge uncommitted work). If exec fails and the tree is
clean: skip 1a and continue to 1b.

### 1b. Merge that branch into master

From the project root (this checkout):

```bash
git merge --no-ff BRANCH
```

- Unique commits: merge. `CURRENT.md` / `docs/capabilities.md` conflicts:
  keep master's dashboard, fold the incoming unique facts.
- Already an ancestor of master: `Already up to date` is success.
- Merge failure: **stop the whole sync**. Do not start the next target.
  Do not begin Pass 2.

## Pass 2 — merge master back into each worktree (serial)

Only after Pass 1 finished for **every** target.

For each target, `workspace.exec --prompt-file`:

- `git merge master` (expect fast-forward; unique work is already on master).
- If `CURRENT.md` / `docs/capabilities.md` conflict, keep master's combined
  dashboard.
- Do not remove the worktree or tab. Do not install.

Then `pane.wait --pane SLUG --status done --timeout 300 --fresh`. If exec
fails on a dead pane, note it and continue; the git checkout can still be
fast-forwarded from here:

```bash
git -C .worktrees/SLUG merge master
```

## Report

| Target | Checkpoint | Merge-up | Sync-down |
|---|---|---|---|
| slug | skipped / clean / commit / failed | merged / already on master / failed | FF / failed / sent |

`master` ahead of origin: say so. Do not push unless asked.
