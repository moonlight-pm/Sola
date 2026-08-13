# sola-agent-terminal — target freeze

**Date:** 2026-08-13  
**Branch:** `naturalethic/sola-agent-terminal`  
**Status:** approved for implementation (promoted from idea)  
**Idea:** [`docs/ideas/2026-08-12-sola-agent-terminal.md`](../ideas/2026-08-12-sola-agent-terminal.md)  
**Product record:** [`crates/sola-agent-terminal/PRODUCT.md`](../../crates/sola-agent-terminal/PRODUCT.md)  
**Design law (session):** [`.grok/rules/agent-terminal-design.md`](../../.grok/rules/agent-terminal-design.md)

**Implementation:** skeleton (kit boot + one pane) in flight  
**Dogfood:** not installed  
**Gaps:** projects persist, spawn sibling, hooks/status, `sat`, toast-on-done — see below

---

## Goal

Ship `crates/sola-agent-terminal`: a native kit app whose unit of work is
**project → workspace → agent-aware terminal**. The sidebar is the
orchestrator. Status is the product. Spawn sibling is how work fans out.
Terminals stay terminals.

It is **not** `crates/sola-agent` (ACP / Grok-leader chat). That crate stays
deprecated for this line of work.

---

## Decisions (locked)

| Topic | Choice |
|---|---|
| Product | Host user-launched CLI agents in PTYs, grouped by project / workspace |
| Fan-out | **Spawn sibling** (UI + `sat`). `--prompt` / `--prompt-file` is the handoff. No mailbox, no Run/Dispatch |
| UI stack | iced + sola-kit. Design law: **impeccable** (Operate) + **frontend-design** before any UI |
| Kit | Not a museum. Refine tokens/atoms/indicators when the improvement is generally true; keep app-local what is this product’s. Do not silently restyle mail / settings / terminal |
| Engine | Reuse `sola-terminal` as a **library** (grid, PTY, input). Do not share tmux socket `sola` or `Topic::TerminalSession` |
| Persistence (PTY) | tmux socket **`sola-at`**, session prefix `sat-`, own systemd unit `sola-at-tmux.service` |
| Status | Hooks (Grok first) + OSC `9999` + process-tree presence. **Never** infer from OSC 0/2 titles |
| Status vocab | `working` / `waiting` / `done` / idle. Reserved indicator slot (no layout shift) |
| Process | One `iced::application` window. Independently restartable kit app |
| Crate / app id | `sola-agent-terminal` |
| CLI | `sat` (later slice). Not on `solactl` |

---

## Interim (ask before treating as product policy)

Recorded so the skeleton can ship. Do **not** pretend these are locked.
Decision points: [`open-questions.md`](../open-questions.md) D3.

| Topic | Interim |
|---|---|
| Window title | `Workspaces` |
| Worktree base | Per-project; this desk’s shape is `~/orca/workspaces/<Project>/` |
| Main checkout | A first-class workspace under the project |
| Drop workspace | Unregister + kill tmux sessions; `git worktree remove` is a separate confirm |
| `sat` if app down | Fail loudly (do not launch a Wayland window as a side effect) |
| Claude in v1 | Presence-only until Grok hooks are trustworthy |

---

## Non-goals (v1)

Embedded editor, browser, Design Mode, Linear/GitHub/Jira, SSH/WSL/remote,
mobile, plugins, ACP chat, mailbox orchestration, 15 hook adapters, usage
dashboards, auto-rename, unread badges, setup-hook runners, sparse checkouts.

`sola-terminal` remains the untitled-shell app.

---

## Architecture

```text
sat  ──unix──▶  sola-agent-terminal (iced)
                      │
                      ├── sola-terminal lib  (grid / pty / input / tmux)
                      ├── tmux socket sola-at
                      ├── hook socket  (later)
                      └── sola-bus     (theme, later stickies, toasts, menu)
```

### Module layout (crate)

```text
crates/sola-agent-terminal/
  PRODUCT.md
  Cargo.toml
  src/main.rs          # iced application, boot, bus
  src/sidebar.rs       # project / workspace rail
  src/workspace.rs     # in-memory project + workspace + pane ids
  src/menu.rs
```

Engine stays in `crates/sola-terminal` (`lib.rs`). This crate does not fork it.

### First slice (skeleton) — this change

- Kit `startup` + `BusSetup` (Theme, MenuAction, CloseApp)
- Own tmux socket configured **before** any PTY
- Sidebar: one hardcoded project, one workspace row, reserved idle mark
- One live pane in that workspace’s cwd (reuse term lib)
- Float chrome via kit CSD
- Compiles under `cargo make build sola-agent-terminal`

Not in skeleton: persist, spawn, hooks, `sat`, splits, toasts.

---

## Information model (v1, later slices)

- **Project** — name, color, collapse, root path, worktree base
- **Workspace** — name, path, kind (main / worktree / folder), optional parent, panes
- **Pane** — tmux session, emulator + PTY, status snapshot

Sidebar rebuilds from **one** status snapshot per tick.

---

## Design (Operate, this surface)

New surface **inside** Sola’s established world (bus theme, kit type roles,
mono grid). Not a replacement visual identity.

- **Mode:** Operate. Scanability and reserved status slots outrank decoration.
- **Color:** Restrained. Theme atoms. Accent only for selection + state marks.
- **Type:** `fonts::ui()` chrome, `fonts::mono()` grid. No display face in UI.
- **Layout:** left rail (projects as quiet section headers, workspaces as
  rows) + right terminal. No right sidebar.
- **Signature (one risk):** the status mark — working ring, waiting amber,
  done check, idle reserved dim. Who (agent name) stays separate from state.
- **Motion:** state only, 150–250 ms. No page-load choreography.

Write `DESIGN.md` from the built surface when the first dogfoodable chrome
exists (impeccable document), not before.

---

## Build order (after this freeze)

1. **Skeleton** — this slice  
2. **Status chrome** — kit `SidebarIndicator` extended; fake states  
3. **Grok hooks + process-tree + OSC 9999**  
4. **Projects + workspaces + spawn sibling**  
5. **`sat`** — `ps`, `workspace spawn`, `pane send`  
6. **Toasts on done**, then stop

---

## Open questions

See idea + D3. Do not invent answers in code that users will live with
(worktree path on disk, `sat` launching the app, display name).
