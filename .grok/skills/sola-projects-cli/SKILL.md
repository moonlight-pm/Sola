---
name: sola-projects-cli
description: >
  Drive sola-projects (Projects) from solactl. Use when the user says review
  ticket, work ticket, implement ticket, fan out, create worktree, spawn
  sibling, new worktree tab (or the old "new workspace"), new tab on master,
  no worktree, root tab, tell that worktree, tell grok in X, brief a tab,
  send to a pane, clean up this worktree, merge and clean up, remove this
  worktree, or solactl projects. Default is background — never steal
  the rail. /sola-projects-cli
---

# sola-projects CLI

Projects is sola-call owner **`projects`**. Face is `solactl projects …`.
The app must be up — the command fails if it is not (it will not launch a
window).

Renamed 2026-10-21: the app was Workspaces (`solactl workspaces`, no
alias now) and a tab was a "workspace". Methods are `tab.*`, the flag is
`--tab` / `-t`, JSON says `tabs` / `tab` / `tab_name`.

Contract: `docs/specs/2026-08-18-workspaces-cli-design.md` (old names in
its body).  
Operator list: `docs/manual/solactl.md`. Do not copy those here.

You are usually in a **root tab**. Do the work here, on master. Fan-out
(a sibling worktree + pane) happens only when the user asks for it — and
never as `git worktree add`.

## Root tabs work on master (`kind: main` / `kind: root`)

A root tab sits in the project root checkout: the project's own tab
(`kind: main`, label `root`) or an extra tab opened with **No worktree**
on the New tab card / `tab.spawn --root` (`kind: root`).
`whoami` / `ps` show the kind; `path` is the project root.

In a root tab, do the work right there on the root's branch (usually
master). Do **not** open a worktree or spawn a sibling unless the user
asks (create a worktree / fan out / spawn a sibling / new worktree tab).
There is no merge step: approval (LGTM / ship it / nailed it…) means
commit on that branch in the same turn. All root tabs share one checkout
— commit only your own changes; never stash, reset, or discard edits you
did not make.

The main tab cannot close (drop the project instead). An extra root tab
closes with plain `tab.rm` (the app refuses `--worktree` — its
path is the project root). On a `kind: root` tab, `tab.set` takes
`--title` and `--before`; `--name` and `--branch` need a worktree. The
project startup script does not run for a root tab.

## Rail (locked)

CLI spawn is **background**. The new row appears; this pane and the rail
stay put.

- Do **not** pass `--select` unless the user asks to jump / switch / open /
  show that tab.
- `tab.exec`, `pane.send`, `pane.read`, `pane.wait` never select.
- `tab.select` is the only dedicated interrupt.
- Do not invert with `--background` / `--no-select`. Quiet is the default.

## Intent → verb

Resolve project/parent with `whoami` (or `ps`) when you are in a Projects
pane. `--name` is the `.worktrees/<name>` folder (rail default). `--title` is the rail label.

| User says | Do |
|---|---|
| review / work / implement / look at **ticket** *N* (no fan-out asked) | Do it here, in this checkout — in a root tab that is master. Do not spawn. If a tab for that ticket already exists, brief it with `tab.exec --prompt` instead. |
| fan out / in a worktree / in a sibling: ticket *N* or this work | If that tab already exists → `tab.exec --prompt`. Else **spawn** grok + `--prompt`. **No `--select`.** Do not wait unless they want a report. |
| create worktree / spawn sibling / new worktree tab | `tab.spawn`. Prompt only if they gave a brief. **No `--select`.** |
| new tab on master / in root / no worktree | `tab.spawn --root` (no `--branch` / `--base-branch`). Prompt only if they gave a brief. **No `--select`.** |
| tell *name* / tell that grok / send to *name* / brief *name* | `tab.exec --prompt` (or `pane.send --enter` for a follow-up line). Do not spawn a second row. Do not select. |
| when it's done / wait / report back | `pane.wait --status done` (add `--fresh` if it may already be done) |
| what's on that pane / read *name* | `pane.read` |
| jump to / switch to / show me / open *name* | `tab.select`, or spawn with `--select` if they are creating *and* want to land there |
| clean up this worktree / merge and clean up this worktree | Merge to master, then remove the git worktree **and** close the tab (below). |
| remove this worktree, don't merge / toss this worktree | Do **not** merge. Remove the git worktree **and** close the tab. |
| drop / close that tab | `tab.rm` only — leave the git worktree |
| rename this worktree / promote to ticket *N* | `tab.set --name SLUG` (moves `.worktrees/<name>`). Add `--title` for the rail label and `--branch` for the git branch. Do not `git worktree move` by hand. |
| rename this tab / change the label | `tab.set --title '…'` (empty clears; rail falls back to the slug). Does not move the folder. |
| reorder this tab / move it before *name* | `tab.set --tab SLUG --before OTHER` (`end` = last in the project). Same project only. |
| close this project | `project.close --project NAME` — its sessions end and Projects forgets it (the folder stays) |

## Fan-out (only when asked)

```bash
solactl projects whoami
solactl projects tab.list --project PROJECT

# exists:
solactl projects tab.exec --tab SLUG --prompt '…'

# new (no --select):
solactl projects tab.spawn --project PROJECT --name SLUG --agent grok \
  --prompt '…'
# Codex or Claude sibling:
# solactl projects tab.spawn --project PROJECT --name SLUG --agent codex \
#   --prompt '…'
# solactl projects tab.spawn --project PROJECT --name SLUG --agent claude \
#   --prompt '…'
# optional: --base-branch origin/dev --branch joshua/sc-1234/fix --title '…'
# long brief: --prompt-file /tmp/brief.md  (not both)
# on master, no worktree (kind: root):
# solactl projects tab.spawn --project PROJECT --name SLUG --root \
#   --agent claude --prompt '…'
```

Then **keep talking in this pane**. Do not `pane.wait` unless they asked.
Do not `tab.select`.

`--prompt` without `--agent` implies grok. `--agent` is `grok`, `codex`, or `claude`.
Spawn parent defaults from `$SOLA_PANE_ID`. Do not fetch/checkout after
spawn to “fix” the branch — pass `--branch` / `--base-branch` on spawn.

## Talk to an existing row

```bash
solactl projects tab.exec --tab SLUG --prompt 'also check X'
solactl projects pane.send --pane SLUG --text 'also check X' --enter
solactl projects pane.read --pane SLUG --lines 80
solactl projects pane.wait --pane SLUG --status done --timeout 300
```

Long or multiline briefs: `--prompt-file`, not a giant `--prompt`. Send is
a paste into the live agent (then Enter). Always pass `--enter` on
`pane.send` when you mean submit — without it the text sits in the composer.

A tab name prefers the **Grok** leaf (or that agent's leaf when
`--agent codex` or `--agent claude`). Pass a pane id from `pane.list` to pin a split.

## Promote an ad hoc tab

`--name` is the `.worktrees/<name>` folder. The catalog id does not
change. `--title` is the rail label (not prefixed with the slug).
`--branch` is optional (`git branch -m`); omit it to keep the current
branch.

```bash
solactl projects tab.set --tab adhoc --name sc-1234 \
    --title 'fix login' --branch joshua/sc-1234/fix
```

## Jump (only if they asked)

```bash
solactl projects tab.select --tab SLUG
solactl projects tab.spawn --project PROJECT --name SLUG --select
```

## Merge / cleanup

Never remove a git worktree or a Projects tab unless they asked. Merge /
LGTM / ship it is **not** a request to remove anything. There is no default
cleanup.

`git worktree remove` does not close the tab by itself; plain `tab.rm`
does not delete the checkout. When they asked to **remove the worktree**,
close the tab **and** pass `--worktree` (the app removes the git checkout
after tmux dies). Do **not** run `git worktree remove` from inside that
pane first — the cwd vanishes, later tools cannot start, and the rail
used to keep a working spinner. If the folder is already gone, Projects
reaps the tab on its own.

| They said | Merge | Git worktree + branch | Close tab |
|---|---|---|---|
| merge / merge that / merge to master / ship it / LGTM / nailed it / looks good / perfect | yes | no | no |
| clean up this worktree / merge and clean up this worktree / ship and cleanup | yes | yes | yes |
| remove this worktree, don't merge / toss this worktree | no | yes | yes |
| close the tab / drop the tab | no | no | yes |
| clean up / close a `kind: root` tab | no | — (none) | yes |
| keep the worktree / keep the tab / hold off / don't drop it | — | no | no |

If merge is in the request and merge fails, stop — do not remove.

### When removing a worktree

Resolve names (`whoami`, then `project.list` / `ps`):

| Var | From |
|---|---|
| `$NAME` | worktree slug / `.worktrees/<name>` |
| `$BRANCH` | git (may differ from `$NAME`) |
| `$PROJECT` | project's `root` — not `whoami.path` |

Never `git worktree remove` from this pane (cwd dies; the next tool
cannot start). Merge and delete the branch **while the checkout still
exists**, then one `tab.rm --worktree`. Same for tab-only drop:
one command, then stop.

```bash
# if merge was requested: commit in the worktree if needed, merge $BRANCH
# into master from the project root, then:
git -C "$PROJECT" branch -d "$BRANCH"
solactl projects tab.rm --tab "$NAME" --worktree
# toss / discard a dirty checkout:
# solactl projects tab.rm --tab "$NAME" --worktree --force
```

`--force` only if they said toss / discard. `git branch -d` refuses
unmerged; do not `-D`. Skip the branch line if you are not deleting it.

Tab-only drop is just `solactl projects tab.rm --tab "$NAME"`
(no `--worktree`). A `kind: root` tab is always a tab-only drop. `tab.rm` replies **before** it kills tmux, so a
foreground call from inside the pane works. After it returns, **stop** —
no more tools in that tab.

Prefer the parent/root pane when it is already there.

## Do not

- Steal the rail (`--select`, `tab.select`) on a ticket/fan-out/create
- Remove a worktree or tab without being asked (merge/LGTM is not asking)
- Remove a worktree and leave the tab (unless they said keep the tab)
- `git worktree remove` from inside the pane being cleaned up (use
  `tab.rm --worktree` instead)
- `git worktree add` as the spawn verb (`tab.spawn` instead)
- Open a worktree or spawn a sibling from a root tab (`kind: main` or
  `kind: root`) unless the user asked for one; pass `--worktree` when
  closing a `kind: root` tab
- `--agent` other than `grok`, `codex`, or `claude` (OpenCode stays presence-only).
- Build a mailbox / ask-reply / `worker_done`
- Call `sat` (there is no such binary)
- Wait out a sibling unless they want a report back
- Raw `tmux paste-buffer` / `send-keys` / `capture-pane` when
  `tab.exec` / `pane.send` / `pane.read` fail — retry or report
  the error. Do not address the pane by `%id` yourself.
