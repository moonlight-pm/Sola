# sola-workspaces — target freeze

**Date:** 2026-08-13  
**Renamed:** 2026-08-14 — crate / app id / face from `sola-agent-terminal` / `at` to `sola-workspaces` / `ws` (D4.1)  
**Branch:** base on master; polish on `workspaces-polish`  
**Status:** approved for implementation (promoted from idea)  
**Idea:** [`docs/ideas/2026-08-12-sola-agent-terminal.md`](../ideas/2026-08-12-sola-agent-terminal.md)  
**Product record:** [`crates/sola-workspaces/PRODUCT.md`](../../crates/sola-workspaces/PRODUCT.md)  
**Design law (session):** [`.grok/rules/workspaces-design.md`](../../.grok/rules/workspaces-design.md)

**Implementation:** persist + spawn modal + done/waiting desk card (title `{project} · {tab}`, body `grok is done`) + sola-call owner `workspaces`; Add project expands `~`; groups stack at the top; no agent label on the workspace row; kit hover × on siblings (not root); kit pane splits (⌘⇧↓ / ⌘⇧→) stay in the grid (one rail row per workspace; Grok mark rolls up waiting > working > done > idle); ⌘W close pane; Drop Project menu-only (`project.rm`); dead last pane **Start new shell** (split leaf exit retracts; hover does not spawn; switch attaches every leaf); quiet `×N` on the workspace row is the loudest Grok session (session dir segments/checkpoints; `signals.json` can stay 0); restart binds tmux by `SOLA_WS_PATH` / cwd (quarantine leftovers); shell launcher builtin **Workspaces**; ⌘T/⌘N; working ring spins. Grok lead hooks (`SessionStart`, `UserPromptSubmit`) reclaim the pane after `/new` / `grok -r` (SessionStart idles a leftover working ring); `StopCancelled` maps to done; child `subagentType` events ignored. `workspace.rm` replies before tmux teardown; `--worktree` also `git worktree remove`s; a gone checkout reaps the tab. `pane.send` / exec `--prompt` bracketed-paste then Enter. Grid selection follows scrolled PTY text (`sel_follow`; CUP rewrite + local scrollback). **CLI control plane:** [`2026-08-18-workspaces-cli-design.md`](2026-08-18-workspaces-cli-design.md) (`workspace.spawn` background unless `--select`)  
**Dogfood:** app installed; rail, splits, drop-project, dead-pane, and `×N` smoked on `workspaces-polish`. Session-reclaim fix installed. Super-chord no longer latches LOGO (⌘T/⌘V used to kill typing until quit). Exiting Grok back to the shell idles the mark (grey disc; was stuck on done). Grid selection follow (`sel_follow`) installed `workspaces`+`terminal` 2026-09-02 (desk smoke). `solactl workspaces` still needs a desk smoke.  
**Gaps:** UI rename modal / recolor / reorder; Claude presence-only (D4). CLI `workspace.set --name` moves the worktree; `--branch` renames HEAD.

---

## Goal

Ship `crates/sola-workspaces`: a native kit app whose unit of work is
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
| Fan-out | **Spawn sibling** (UI + `solactl workspaces workspace.spawn`). `--prompt` is the CLI handoff. No mailbox, no Run/Dispatch |
| UI stack | iced + sola-kit. Design law: **impeccable** (Operate) + **frontend-design** before any UI |
| Kit | Not a museum. Refine tokens/atoms/indicators when the improvement is generally true; keep app-local what is this product’s. Do not silently restyle mail / settings / terminal |
| Engine | Reuse `sola-terminal` as a **library** (grid, PTY, input). Do not share tmux socket `sola` or `Topic::TerminalSession` |
| Persistence (PTY) | tmux socket **`sola-ws`**, session prefix `sws-`, own systemd unit `sola-ws-tmux.service` |
| Status | Hooks (Grok first) + OSC `9999` + process-tree presence. **Never** infer from OSC 0/2 titles |
| First-class CLI | **Grok.** Always implement and test Grok first. Other agents are presence-only until Grok hooks are trustworthy |
| Status vocab | `working` / `waiting` / `done` / idle. Reserved indicator slot (no layout shift) |
| Process | One `iced::application` window. Independently restartable kit app |
| Crate / app id | `sola-workspaces` |
| Window title | `Workspaces` |
| CLI | sola-call owner `workspaces`. Face is `solactl workspaces …`. App/host down → fail |
| Config | `~/.config/sola/workspaces/` (one-shot migrate from `agent-terminal/`) |

---

## Interim (ask before treating as product policy)

Recorded so the skeleton can ship. Do **not** pretend these are locked.
Decision points: [`open-questions.md`](../open-questions.md) D4.

| Topic | Interim |
|---|---|
| Window title | **Locked (D4.1):** `Workspaces` / crate `sola-workspaces` |
| Worktree base | **Locked (D4.2):** `<project-root>/.worktrees/<name>` |
| Main checkout | A first-class workspace under the project |
| Drop workspace | Unregister + kill tmux sessions. Hover × / plain `workspace.rm` leave the checkout. CLI `--worktree` is the explicit `git worktree remove` (not a silent hover). A gone checkout reaps the tab. |
| CLI if app down | Fail loudly (do not launch a Wayland window as a side effect) |
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
solactl workspaces  ──sola-call──▶  sola-workspaces (iced)
                                        │
                                        ├── sola-terminal lib
                                        ├── tmux socket sola-ws
                                        ├── hook socket  $XDG_RUNTIME_DIR/sola-ws-hooks.sock
                                        └── sola-bus     (theme, toasts, menu)
```

### Module layout (crate)

```text
crates/sola-workspaces/
  PRODUCT.md
  DESIGN.md            # Operate surface, recorded from status chrome
  Cargo.toml
  src/main.rs          # iced application, boot, bus
  src/sidebar.rs       # project / workspace rail
  src/status.rs        # working / waiting / done / idle + persist
  src/hooks.rs         # Grok installer + UDS server
  src/presence.rs      # process-tree who (Grok first)
  src/workspace.rs     # project + workspace + catalog persist
  src/spawn.rs         # git worktree add under .worktrees/
  src/calls.rs         # sola-call MethodSpec list (owner workspaces)
  src/cli.rs           # call-plane payloads, targeting, prompt, wait
  src/startup.rs       # per-project script after sibling spawn
  src/paths.rs         # config dir + legacy migrate
  src/menu.rs
```

Engine stays in `crates/sola-terminal` (`lib.rs`). This crate does not fork it.

### First slice (skeleton) — this change

- Kit `startup` + `BusSetup` (Theme, MenuAction, CloseApp)
- Own tmux socket configured **before** any PTY
- Sidebar: one hardcoded project, one workspace row, reserved idle mark
- One live pane in that workspace’s cwd (reuse term lib)
- Float chrome via kit CSD
- Compiles under `cargo make build sola-workspaces`

Not in skeleton: persist, spawn, hooks, splits, toasts.

### Status chrome slice

- Kit `status_mark`: working ring (accent, spinning), waiting diamond
  (warning), done check (success), idle reserved disc. `Active` unchanged
  for generic apps.
- Rail: one live workspace + labeled `demo` rows covering working /
  waiting / done. Who (agent name) is a trailing secondary.
- Sidebar rebuilds from the in-memory status on each workspace.

### Grok hooks slice

- Installer writes `~/.grok/hooks/sola-status.json` +
  `~/.config/sola/workspaces/grok-hook.sh`. Leaves `orca-status.json`
  alone.
- UDS `$XDG_RUNTIME_DIR/sola-ws-hooks.sock`. Pane env: `SOLA_PANE_ID`,
  `SOLA_WS_HOOKS_SOCK`.
- Process-tree presence (Grok first). OSC 9999 stripped in `sola-terminal`
  before the grid. Titles never drive state. Child `Subagent*` events
  ignored.

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

`DESIGN.md` records the built status-chrome surface. Update it when the
look changes; do not treat it as a second freeze.

---

## Build order (after this freeze)

1. **Skeleton** — done  
2. **Status chrome** — done (kit `status_mark`)  
3. **Grok hooks + process-tree + OSC 9999** — done  
4. **Projects + workspaces + spawn sibling** — done (catalog.json; `.worktrees/`; name modal)  
5. **`sat` private UDS** — retired; methods on sola-call (`ws`)  
6. **Toasts on done** — done (shell `AppNotification` desk card; skip focused + hydrate)  
7. **Rename to sola-workspaces** — done (D4.1)

---

## Open questions

See idea + D4. Name and worktree path are locked. Do not invent
Claude hook policy. CLI if down is fail (call plane).
