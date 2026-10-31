---
name: sola-projects-cli
description: >
  Drive sola-projects (Projects) from solactl. Use when the user says start
  a session, new session in <project>, fan out, brief a session, tell that
  session / tell grok in X, send to a pane, read a pane, wait for it, end a
  session, open a project (here or on another computer), or solactl
  projects. Default is background — never steal focus.
  /sola-projects-cli
---

# sola-projects CLI

Projects is sola-call owner **`projects`**. Face is `solactl projects …`.
The app must be up — the command fails if it is not.

Model (sessions anywhere, 2026-10-30; spec
`docs/specs/2026-10-30-projects-sessions-anywhere-design.md`):

- A **session** is a tmux session on the computer it runs on (socket
  `sola-ws` there). It starts in its **project's folder**. Any computer's
  Projects can open it; each open session is its own window (a row in the
  shell's tab bar, labelled project · session · host).
- **Projects makes no worktrees.** A session makes its own when its work
  needs one (AGENTS.md → Worktrees): `git worktree add .worktrees/<slug>
  -b <branch>` from the project root. Never ask Projects for one.
- **Closing a window detaches.** The session keeps running. Only **End
  Session** (menu) / `tab.rm` ends it — for every computer watching.

The verbs still say `tab` (`tab.spawn`, `--tab`, JSON `tabs`): a tab is a
session. A session from another computer is keyed `<id>@<host>`.

Operator list: `docs/manual/solactl.md`. Do not copy it here.

## Intent → verb

Resolve the project with `whoami` (or `ps`) when you are in a Projects
pane.

| User says | Do |
|---|---|
| start / fan out a session in *project* (with a brief) | `tab.spawn --project P [--name N] --agent claude --prompt '…'` — no `--select` |
| tell *name* / brief *name* / send to *name* | `tab.exec --tab N --prompt '…'` (or `pane.send --pane N --text '…' --enter`) |
| what's on that pane / read *name* | `pane.read --pane N` |
| when it's done / report back | `pane.wait --pane N --status done` (`--fresh` if it may be done already) |
| show me / switch to *name* | `tab.select --tab N` |
| open *project* (on *host*) | `project.open --path ~/Projects/P [--host H]` — opens its running sessions there, else a new main one |
| rename that session | `tab.set --tab N --name '…'` and/or `--title '…'` |
| end that session | `tab.rm --tab N` (kills it where it runs; worktrees stay) |
| close the project's windows here | `project.close --project P` (its sessions keep running) |

Worktree requests ("in a worktree", "on a branch") are the session's own
job: brief it to make one, or make it yourself in your session.

## Focus (locked)

CLI spawn is **background**: the new window lands behind. Pass `--select`
only when the user asks to jump to it. `tab.exec`, `pane.send`,
`pane.read`, `pane.wait` never select. `tab.select` is the only interrupt.

## Fan-out

```bash
solactl projects whoami
solactl projects tab.list --project PROJECT
# exists:
solactl projects tab.exec --tab NAME --prompt '…'
# new (no --select):
solactl projects tab.spawn --project PROJECT --name NAME --agent claude \
  --prompt '…'
# long brief: --prompt-file /path/brief.md (not both)
```

Then **keep talking in this pane**. Do not `pane.wait` unless they asked.

`--prompt` without `--agent` implies grok. `--agent` is `grok`, `codex`,
or `claude`. Spawn parent defaults from `$SOLA_PANE_ID`.

## Talk to an existing session

```bash
solactl projects tab.exec --tab NAME --prompt 'also check X'
solactl projects pane.send --pane NAME --text 'also check X' --enter
solactl projects pane.read --pane NAME --lines 80
solactl projects pane.wait --pane NAME --status done --timeout 300
```

Long or multiline briefs: `--prompt-file`. `pane.send` pastes into the
live agent; pass `--enter` when you mean submit.

## Do not

- Steal focus (`--select`, `tab.select`) on a fan-out
- End a session (`tab.rm`) unless asked — closing its window is not ending
  it, and ending it ends it for every computer
- Ask Projects for a worktree, or remove one unasked
- Build a mailbox / ask-reply / `worker_done`
- Raw `tmux paste-buffer` / `send-keys` / `capture-pane` when
  `tab.exec` / `pane.send` / `pane.read` fail — retry or report the error
