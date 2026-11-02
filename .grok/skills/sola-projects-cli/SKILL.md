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
  Session** (menu) / `session.end` ends it — for every computer watching.

The verbs say `session` (`session.new`, `--session`, JSON `sessions`;
they said `tab` until 2026-10-31). A session from another computer is keyed
`<id>@<host>`.

Operator list: `docs/manual/solactl.md`. Do not copy it here.

## Intent → verb

Resolve the project with `whoami` (or `ps`) when you are in a Projects
pane.

| User says | Do |
|---|---|
| start / fan out a session in *project* (with a brief) | `session.new --project P [--name N] --agent claude --prompt '…'` — no `--select` |
| tell *name* / brief *name* / send to *name* | `session.exec --session N --prompt '…'` (or `pane.send --pane N --text '…' --enter`) |
| what's on that pane / read *name* | `pane.read --pane N` |
| when it's done / report back | `pane.wait --pane N --status done` (`--fresh` if it may be done already) |
| show me / switch to *name* | `session.select --session N` |
| open *project* (on *host*) | `project.open --path ~/Projects/P [--host H]` — opens its running sessions there, else a new main one |
| a window to pick a project in (what the launcher's Projects opens) | `window.navigate` — opens in front and takes focus: only when asked |
| rename that session | `session.set --session N --name '…'` and/or `--title '…'` |
| end that session | `session.end --session N` (kills it where it runs; worktrees stay) |
| close the project's windows here | `project.close --project P` (its sessions keep running) |

Worktree requests ("in a worktree", "on a branch") are the session's own
job: brief it to make one, or make it yourself in your session.

## Focus (locked)

CLI spawn is **background**: the new window lands behind, on the workspace
of the session that started it. Pass `--select`
only when the user asks to jump to it. `session.exec`, `pane.send`,
`pane.read`, `pane.wait` never select. `session.select` is the only interrupt.

## Fan-out

```bash
solactl projects whoami
solactl projects session.list --project PROJECT
# exists:
solactl projects session.exec --session NAME --prompt '…'
# new (no --select):
solactl projects session.new --project PROJECT --name NAME --agent claude \
  --prompt '…'
# long brief: --prompt-file /path/brief.md (not both)
```

Then **keep talking in this pane**. Do not `pane.wait` unless they asked.

`session.new --prompt` without `--agent` starts grok: name the agent you
mean. `session.exec` without `--agent` sends to whatever agent is running
in that session (grok starts only when none is) — leave `--agent` out to
talk to a running session; naming a different one is an error. `--agent`
is `grok`, `codex`, or `claude`. Spawn parent defaults from
`$SOLA_PANE_ID`.

## Talk to an existing session

```bash
solactl projects session.exec --session NAME --prompt 'also check X'
solactl projects pane.send --pane NAME --text 'also check X' --enter
solactl projects pane.read --pane NAME --lines 80
solactl projects pane.wait --pane NAME --status done --timeout 300
```

Long or multiline briefs: `--prompt-file`. `pane.send` pastes into the
live agent; pass `--enter` when you mean submit.

## Do not

- Steal focus (`--select`, `session.select`) on a fan-out
- End a session (`session.end`) unless asked — closing its window is not ending
  it, and ending it ends it for every computer
- Ask Projects for a worktree, or remove one unasked
- Build a mailbox / ask-reply / `worker_done`
- Raw `tmux paste-buffer` / `send-keys` / `capture-pane` when
  `session.exec` / `pane.send` / `pane.read` fail — retry or report the error
