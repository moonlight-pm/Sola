# Product

<!-- impeccable:product-schema 1 -->

## Platform

linux

Native Sola Wayland app (iced / sola-kit). Not web, iOS, or Android.

## Stack

delegated: iced 0.14 + sola-kit + sola-terminal lib + tmux, because that is the
Sola app chassis and the proven PTY grid. No WebView. No Electron.

## Users

The person at this desk who already runs several CLI coding agents (Grok first)
across isolated git checkouts, grouped by product (Sola, Illuno, Wicket, …).
They glance at many panes and need to know which are working, waiting, or done
without opening each one.

## Product Purpose

Run CLI coding agents in real terminals, grouped by **project** and
**workspace**. Status is visible at a glance. **Spawn sibling** creates a
parallel checkout + pane, optionally already briefed. Success is: open a
project, see every live workspace’s state, type the next prompt or spawn a
sibling, restart the app and nothing is gone.

## Positioning

A native Sola tool whose sidebar *is* the orchestrator. Not an IDE. Not an ACP
chat GUI (retired `sola-agent`). Not Orca-the-Electron-suite. Neighbors cannot copy “the
PTY is the agent, the mark is the truth, spawn is the only fan-out verb” without
becoming this product.

## Operating Context

- Physical TTY → Sola desktop; this app is one window among kit apps
- Sibling checkouts live under `<project-root>/.worktrees/<name>`
- Agents are CLI TUIs in PTYs (Grok). Orca may be installed alongside; hooks
  must not fight (`sola-status.json` vs `orca-status.json`)
- `sola-terminal` remains the untitled shell on tmux socket `sola`

## Capabilities and Constraints

**In:** projects, workspaces (main / worktree / folder), agent-aware panes,
spawn sibling (UI: name only, takes the rail; `solactl workspaces workspace.spawn`
is background unless `--select`; can pass `--agent grok` + `--prompt` /
`--prompt-file`; exec/send is a tmux paste then Enter), kit pane splits, Grok hooks
+ OSC 9999 + process-tree presence, quiet `×N` rolled up across Grok panes
in a workspace, sola-call owner `workspaces` (`solactl workspaces …` is
first-class — verbs stay in lockstep with the app), per-project startup
script after spawn (Project → Startup Script…),
tmux persist on socket `sola-ws`, unfocused desk card
(title `{project} · {tab}`, body `grok is done` / `needs attention`),
`workspace.rm --worktree` (tab then git checkout; gone paths reap the tab),
`workspace.set --name` (rail slug + `git worktree move` to `.worktrees/<name>`;
`--branch` is `git branch -m`).

**First-class CLI:** **Grok.** Implement and test Grok first whenever adding
agent support. Other CLIs are presence-only until Grok status is trustworthy.

**Out (v1):** editor, browser, issue trackers, remotes, mobile, ACP chat,
mailbox orchestration, 15 hook adapters.

**Undecided (do not invent):** Claude hook installer vs presence. App-down
is fail (call plane).

## Brand Commitments

- Crate / app id: `sola-workspaces`. Window title: **Workspaces**.
- Design law: impeccable Operate + frontend-design before UI; kit may be refined
- Theme rides the Sola bus; do not silently restyle other apps
- Voice: operator-plain. “Spawn sibling,” “working,” “done.” No marketplace copy

## Evidence on Hand

- Idea: `docs/ideas/2026-08-12-sola-agent-terminal.md` (Orca + sola-terminal research)
- Freeze: `docs/specs/2026-08-13-sola-agent-terminal-design.md`
- Incumbent grid: `crates/sola-terminal`
- Incumbent kit: `crates/sola-kit` (atoms, `SidebarIndicator`, `status_mark`, `SidebarPanel`)
- Surface record: `crates/sola-workspaces/DESIGN.md` (status chrome)
- Grok hooks: `~/.grok/hooks/sola-status.json` (do not touch `orca-status.json`)
- No customer quotes, screenshots of this app, or usage stats. Do not fabricate.

## Product Principles

1. The sidebar is the orchestrator; status is the product.
2. Spawn sibling is the only fan-out protocol; a prompt file is the handoff.
   `solactl workspaces` is the same protocol agents use.
3. Terminals stay terminals — no inferred state from titles.
4. One signature (the status mark); everything else is quiet and scannable.
5. Refuse Orca cruft. Refuse rebuilding `sola-agent`.
