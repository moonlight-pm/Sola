# sola-workspaces — design law

Applies when touching **`crates/sola-workspaces`** UI, or kit tokens /
components / atoms **in service of that app**.

Canonical sketch: [`docs/ideas/2026-08-12-sola-agent-terminal.md`](../../docs/ideas/2026-08-12-sola-agent-terminal.md) (Design law).

## Must

1. Load **impeccable** (Operate mode) and **frontend-design** before any UI
   layout, chrome, or visual change. Shape first; do not grey-box then polish.
2. Status scanability and project grouping outrank decoration. One signature
   (likely the status mark); keep the rest quiet.
3. **`sola-kit` is not frozen.** If a cleaner token, atom, indicator, density,
   or component is evident, refine the kit. If the need is this app only,
   keep the widget local. Do not silently restyle mail / settings / terminal.
4. Spawn sibling is a v1 product verb — design it as a first-class control,
   not a hidden menu item.
5. **Grok is the first-class CLI.** Implement and test Grok first (hooks,
   presence, OSC, spawn `--agent`). Other agents are presence-only until
   Grok status is trustworthy. Do not lead with Claude/Codex adapters.

## Do not

- Cargo-cult Orca’s worktree cards or today’s graphite sidebar as the look.
- Skip the skills because “it’s just a list of terminals.”
- Rewrite unrelated storybook pages without asking (see `kit-storybook-pages`).
