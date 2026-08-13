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
chat (`sola-agent`). Not Orca-the-Electron-suite. Neighbors cannot copy “the
PTY is the agent, the mark is the truth, spawn is the only fan-out verb” without
becoming this product.

## Operating Context

- Physical TTY → Sola desktop; this app is one window among kit apps
- Checkouts often live under `~/orca/workspaces/<Project>/` (this desk)
- Agents are CLI TUIs in PTYs (Grok). Orca may be installed alongside; hooks
  must not fight (`sola-status.json` vs `orca-status.json`)
- `sola-terminal` remains the untitled shell on tmux socket `sola`

## Capabilities and Constraints

**In:** projects, workspaces (main / worktree / folder), agent-aware panes,
spawn sibling with `--prompt`, Grok hooks + OSC 9999 + process-tree presence,
`sat` CLI, tmux persist on socket `sola-at`.

**Out (v1):** editor, browser, issue trackers, remotes, mobile, ACP chat,
mailbox orchestration, 15 hook adapters.

**Undecided (do not invent):** display name, worktree-base convention as
policy, whether `sat` may launch the app, Claude hook installer vs presence.

## Brand Commitments

- Crate / app id: `sola-agent-terminal` (working title)
- Design law: impeccable Operate + frontend-design before UI; kit may be refined
- Theme rides the Sola bus; do not silently restyle other apps
- Voice: operator-plain. “Spawn sibling,” “working,” “done.” No marketplace copy

## Evidence on Hand

- Idea: `docs/ideas/2026-08-12-sola-agent-terminal.md` (Orca + sola-terminal research)
- Freeze: `docs/specs/2026-08-13-sola-agent-terminal-design.md`
- Incumbent grid: `crates/sola-terminal`
- Incumbent kit: `crates/sola-kit` (atoms, `SidebarIndicator`, `SidebarPanel`)
- No customer quotes, screenshots of this app, or usage stats. Do not fabricate.

## Product Principles

1. The sidebar is the orchestrator; status is the product.
2. Spawn sibling is the only fan-out protocol; a prompt file is the handoff.
3. Terminals stay terminals — no inferred state from titles.
4. One signature (the status mark); everything else is quiet and scannable.
5. Refuse Orca cruft. Refuse rebuilding `sola-agent`.
