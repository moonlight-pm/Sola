# sola-agent-terminal — a native, Orca-shaped workspace tool

**Status:** promoted. Freeze: [`docs/specs/2026-08-13-sola-agent-terminal-design.md`](../specs/2026-08-13-sola-agent-terminal-design.md). Persist + spawn sibling into `.worktrees/` landed.  
**Name:** `sola-agent-terminal` (working title).  
**Out of scope by request:** `crates/sola-agent` (ACP / Grok-leader chat). That crate is a different product and is not a starting point.  
**Living focus pointer:** root [`CURRENT.md`](../../CURRENT.md) **Now** item 1 (this branch).  
**Session rule:** [`.grok/rules/agent-terminal-design.md`](../../.grok/rules/agent-terminal-design.md).

This note is a research-backed sketch: what Orca actually is, what `sola-terminal` already is, and a *small* first version that keeps the parts that feel great and drops the rest.

### Where a new session left off (2026-08-13)

Promoted to freeze. Persist + spawn sibling into `<root>/.worktrees/`. Demo rows gone.

**Next:** dogfood `solactl at`. No `sat` binary. Remaining D4: name, Claude hooks.

**Decided here (not still a fork):**

- **Spawn sibling** is a v1 verb — UI and `sat`. `--prompt` / `--prompt-file` is the handoff.
- **Design law** below is mandatory for every UI slice of this app.

---

## Why this exists

`sola-terminal` is a real, fast, kit-native terminal: alacritty grid, PTY + tmux persistence, splits, sidebar tabs. It is a **shell**. It does not know what a project is, and it does not know that a pane is running Grok.

Orca (Electron, `stablyai/orca`, currently ~1.4.x) is the opposite: a large IDE/orchestrator whose best daily feeling is *“I can see every agent, what it is doing, and which checkout it belongs to.”* That feeling is worth stealing. Almost everything around it is not.

The bet: a **native Sola app** whose unit of work is *project → workspace → agent-aware terminal*, polished and cheap, instead of an Electron IDE that also happens to host terminals.

---

## What Orca actually is

Orca is an Electron + React + Zustand desktop with a Node main process, a CLI (`orca`), a relay for remotes, a mobile companion, and a very large renderer. Marketing line: *run Codex / Claude / OpenCode / Pi side-by-side, each in its own worktree.*

The parts that matter for this idea:

| Surface | What it really is |
|---|---|
| **Agent status** | Explicit hook protocol (`working` / `blocked` / `waiting` / `done`), **not** title scraping. Hooks POST JSON; OSC `9999` is a side channel; titles are explicitly distrusted. |
| **Status chrome** | Shared `AgentStateDot`: spinner = working, amber `?` = waiting, check = done, dim dot = idle. Who (Claude/Grok icon) is separate from *what state*. |
| **Worktrees** | Isolated git checkouts. `orca worktree create --name … --agent grok --prompt "…"` is the core “fan out” verb. |
| **Project groups** | Sidebar clusters: name, color, collapse, optional parent path. This is *organization*, not execution. |
| **Projects / host setups** | Durable identity (`github:owner/repo`) plus per-host clone paths. Exists so the same “project” can live on local + SSH + GPU boxes. |
| **Folder workspaces** | Non-git folders hanging off a project group. Parallel type to worktrees. |
| **Orchestration** | Runs, dispatches, mailboxes, `worker_done`, ask/reply, circuit breakers, SQLite. A coordinator/worker protocol on top of terminals. |
| **CLI** | Agents drive Orca: `worktree create/ps`, `terminal list/read/send/wait/create/split`, `orchestration *`. |
| **Terminals** | `node-pty` + xterm/WebGL, scrollback files under `~/.config/orca/terminal-history/`, **no tmux**. Env stamps `ORCA_PANE_KEY`, `ORCA_WORKTREE_ID`, `ORCA_TERMINAL_HANDLE`. |

The status path is the product’s heart, and Orca paid for it. A production trace of 100 expanded worktree cards hit **9,279 Zustand listeners**; a 2,000-event status burst was ~3.7 s of store time until they batched into one transaction. That is the cost of “every card independently subscribed to the live map” in a web renderer. We should learn the lesson without copying the machinery.

Hook truth lives in `src/shared/agent-status-types.ts` and `src/main/agent-hooks/`. The four states are the whole vocabulary. Extra UI states (`permission`, `failed`, `interrupted`) are derived. Freshness decays: a `working` row with no hook for 30 minutes is no longer treated as live. Nested CLIs inherit the parent `ORCA_PANE_KEY`, so a child `done` must not clear the parent turn.

Grok (the agent this desk actually runs) is hooked via `~/.grok/hooks/orca-status.json`: `SessionStart`, `UserPromptSubmit`, `Stop`, `StopFailure`, `SessionEnd`, `PreToolUse` / `PostToolUse` / `PostToolUseFailure`, `Notification`. `StopFailure` exists specifically so a sidebar does not stick on *working* after an API error.

---

## What to leave on the floor

Orca is also: an embedded VS Code editor, an embedded Chromium + Design Mode, Linear / GitHub / Jira / GitLab first-class UIs, SSH remotes + WSL + relay, mobile, plugins, native chat, AI vault, automations, computer-use, Android emulator, speech/dictation, i18n, telemetry, updater, ~15 managed hook installers, per-provider account switchers, usage dashboards, pets.

That is the cruft. A Sola app that tries to be “Orca, but Iced” will become Orca. The first version should refuse all of it.

Also refuse, for this app:

- Rebuilding `sola-agent` (ACP session UI, Grok leader turn-loop). Different product. The locked model *“attach to the shared Grok leader; do not spawn private turn-loop agents”* stays about that crate. This app **hosts user-launched CLI agents in PTYs**. It is not a turn-loop.
- Inferring agent state from OSC 0/2 titles. Orca tried; the types file says status is never inferred from titles.
- One process that is both “plain terminal” and “project orchestrator.” Keep `sola-terminal` as the simple shell.

---

## What `sola-terminal` already gives us

The iced port is a serious terminal, not a sketch.

| Piece | Role | Reuse? |
|---|---|---|
| `emulator.rs` | `alacritty_terminal::Term` + VTE processor, 33 ms output coalesce, lock-free cursor snap | **yes** — this is the grid |
| `pty.rs` | openpty, reader/writer threads, never write on the UI thread | **yes** |
| `term_view.rs` | canvas renderer: snapshot under lock, paint after drop; uncached cursor overlay | **yes** |
| `input.rs` / `extkeys.rs` | keyboard, mouse SGR, kitty/CSI-u, Shift+Enter | **yes** |
| `tmux.rs` | socket `sola`, persist across app death, OSC 7 cwd fallback | **pattern yes, socket no** |
| `state.rs` | tab → binary split tree of panes | **yes** as workspace-local layout |
| `sidebar.rs` | kit `SidebarPanel`, reorder, collapse | **starting point**; too flat for projects |
| `links.rs` | OSC 8 + URL scan | **yes** |
| Bus `TerminalSession` / `TerminalConfig` | sticky tab restore | **do not share** — new topics, new socket |

Performance notes we should keep: PTY output is already batched (~30 Hz); the canvas cache is cleared per-pane, not globally; blink does not invalidate the grid; wheel reports to mouse-mode TUIs are rate-limited. That stack already hosts Grok TUIs without the Electron key-echo problem.

Two collisions to design around on day one:

1. **tmux socket.** `sola-terminal` owns `sola`. This app needs its own (`sola-at` or similar). Sharing a server would mix “just a shell” tabs with workspace PTYs and make restore logic hostile.
2. **Bus stickies.** `Topic::TerminalSession` is sola-terminal’s. New topic kinds (or a single namespaced document) for projects / workspaces / pane layouts.

Suggested reuse shape, when this is promoted: **keep `sola-terminal` as a binary**, expose its engine as a library (`emulator`, `pty`, `term_view`, `input`, `links`) from the same crate, and have `sola-agent-terminal` depend on that lib. Do not invent `sola-term` as a third crate until a third consumer exists.

---

## Product thesis

> A native app for running CLI coding agents across isolated checkouts.  
> The sidebar is the orchestrator. Status is the product.  
> Spawn sibling is how work fans out.  
> Terminals stay terminals.

Daily loop we want to be excellent at:

1. Open the **Sola** project. See every live workspace.
2. See, without clicking, which panes are **working**, **waiting**, or **done**.
3. Focus a done agent and type the next prompt — or **spawn a sibling workspace** for a parallel slice (optionally already briefed).
4. Restart the app; every PTY and its last status is still there.

If a feature does not serve that loop, it is not v1.

---

## Design law (mandatory)

This app is allowed to be *the* design-quality bar for Sola, not another kit consumer that inherits whatever is there.

**Any UI work — first surface, sidebar, status chrome, empty states, toasts, spawn dialogs, storybook if we add one — must load and follow:**

1. **`impeccable`** — Operate mode (this is a tool). Shape before code when the surface is new; critique / distill / polish before calling a slice done. Craft floor applies: hierarchy, scanability, reserved status slots, no decoration that does not carry information.
2. **`frontend-design`** — distinctive, subject-grounded visual choices. Status and project grouping are the subject; do not ship a generic “dark dashboard with accent dots.” Take one justified aesthetic risk and keep everything else quiet.

Do this *before* drawing widgets, not as a pass after a grey layout exists. If the two skills disagree, **impeccable’s Operate constraints win** (scan, native affordance, density) and frontend-design spends its boldness on the signature (likely the status mark + project grouping), not on chrome noise.

### The kit is not a museum

`sola-kit` tokens, atoms, sidebar assumptions, indicator vocabulary, density, and components are **in scope to reassess** for this project.

- If a cleaner refinement is evident (e.g. `SidebarIndicator` growing `Working | Waiting | Done`, tighter section headers, a reserved-slot status mark that other apps can use), **change the kit**.
- If a more appealing design choice is evident (palette role, type role, spacing rhythm, card vs row), **do it** — prefer a kit-level change when the improvement is generally true, an app-local widget when the need is this product’s (spawn lineage, agent spinner).
- Shared bus theme still exists. Do not silently restyle mail / settings / terminal. A kit change should be an intentional, reviewable improvement to the system; an experiment that only this app should carry stays in this crate.
- Existing storybook-page rule still applies: ask before rewriting unrelated kit demo pages.

When this is promoted, write a short `DESIGN.md` / surface brief for the app (impeccable `init` / `document`) so later sessions do not re-litigate the world. Until then, this section is the law.

---

## Early feature set (tight)

### 1. Projects (the group you liked)

A **project** is a named, colored, collapsible sidebar group bound to one root directory (the main checkout) and a worktree base (where siblings land).

That is Orca’s *project group*, not Orca’s *project + host setup*. We do not need `github:owner/repo` identities, per-host clone records, or nested-repo import scanners.

Example that matches this desk:

```text
Sola      ~/src/sola          worktrees → ~/orca/workspaces/Sola/
Illuno    ~/src/illuno        worktrees → ~/orca/workspaces/Illuno/
Wicket    …
```

v1 operations: add project (pick a folder), rename, recolor, collapse, reorder, remove (does not delete the git repo).

No nested groups. No auto-scan of the home directory.

### 2. Workspaces (checkouts, not “runs”)

A **workspace** is one checkout under a project:

- the **main** tree, or
- a **git worktree** created by the app, or
- a **folder** (escape hatch; not every useful directory is a git repo).

Each workspace has a display name, a path, optional parent (lineage, see below), and one or more terminal panes.

Creating a workspace from the UI or CLI:

```text
sat workspace spawn --project sola --name kvm-perf --agent grok
```

does `git worktree add`, opens the workspace, starts a pane in that cwd, optionally execs `grok`.

Orca’s `Worktree` type carries linked GitHub/Linear/Jira fields, sparse presets, push targets, unread badges, automation provenance, first-message auto-rename, … We keep **name, path, parent, created-at, last-activity**. Auto-rename from the first prompt is a nice later polish, not a v1 requirement.

### 3. Agent-aware terminals (the thing you liked most)

Every pane is a real PTY (same engine as sola-terminal). A pane may be a shell, or it may be a CLI agent.

**Presence** (is an agent in this pane?) and **state** (what is it doing?) are separate:

| Signal | How | Tells us |
|---|---|---|
| Process tree | walk the tmux pane’s descendants for known binaries (`grok`, `claude`, `codex`, …) | *an agent is here* |
| Hooks | Grok first; Claude if cheap | *working / waiting / done* + tool name + last prompt |
| OSC `9999` | strip `\x1b]9999;{json}\x07` from the byte stream (Orca’s existing side channel) | same payload, for agents we have not hooked |
| Title | OSC 0/2 as today | **never** used for state |

State vocabulary for v1 — steal Orca’s four, render three plus idle:

| State | Sidebar | Meaning |
|---|---|---|
| `working` | spinner | turn in flight (tools, streaming) |
| `waiting` | amber mark | needs a human (question, permission) |
| `done` | check | turn finished (or interrupted — tooltip says which) |
| *(none)* | dim reserved dot | shell, or agent present but idle / stale |

Always reserve the indicator slot so rows do not shift when a turn starts. Kit `SidebarIndicator` is currently `Active | Idle`. Treat that as **evidence, not a ceiling** — extend the kit (`Working | Waiting | Done | Idle`, or a better mark) if the scan is cleaner there; invent an app-local mark only if the kit shape is wrong. Design law decides, not “match existing apps.”

Do **not** ship 15 hook installers. Ship **Grok** (this machine’s agent) as the first-class hook, plus OSC `9999` so anything that can emit it works, plus process-tree presence so a pane running `claude` still shows *an agent lives here* even without state. Claude hooks are the obvious second installer; they wait until Grok status is trustworthy.

Hook transport: a **Unix socket** owned by this app (`$XDG_RUNTIME_DIR/sola-at-hooks.sock`), not Orca’s localhost HTTP server. Managed hook scripts write JSON lines / HTTP-over-UDS with `SOLA_PANE_ID`. If Orca is also installed, leave `~/.grok/hooks/orca-status.json` alone and write `sola-status.json` next to it. Both can fire; identity is the env var, not the hook file name.

Hard-won rules to copy, not rediscover:

- A child CLI inheriting `SOLA_PANE_ID` must not mark the parent `done`.
- `StopFailure` / equivalent maps to `done` (or `waiting` if we can see it is a retryable error) — never leave the spinner up.
- Hydrated status from disk is `restoredUnconfirmed` until a live hook arrives; do not toast “done” on startup.
- Coalesce status into the UI at the same 33 ms cadence as PTY output. One snapshot per tick, not one message per hook.

### 4. Orchestration = spawn sibling

Orca’s orchestration (Run / Dispatch / mailbox / `worker_done`) is a distributed workflow engine bolted onto terminals. Useful if you want agents to be a job queue. Heavy if you want to *see and steer a few parallel slices*.

**Different take: spawn sibling is the protocol. The sidebar is the dashboard.**

Confirmed v1 verbs:

| Verb | What it does |
|---|---|
| **Spawn sibling** | New worktree under the same project, new workspace row, new pane. `--agent` starts a CLI; `--prompt` (or `--prompt-file`) briefs that first turn. `--parent` (default when spawned from a workspace / `sat` inside one) records lineage so the child nests under the caller. Available from the UI and from `sat`. |
| **New pane** | Another PTY in the *same* workspace (split or tab). For “a second Grok in this checkout,” not a new tree. |
| **Focus / send** | CLI can focus a pane or type into it (`sat pane send --text … --enter`). |
| **ps** | Compact dump: project → workspace → pane → state. The text form of the sidebar. |

No run objects. No mailbox. No circuit breaker. If two agents need to share context, they share the git repo, the spawn prompt, a file, or a human looking at two panes.

Lineage is **visual metadata**, not a scheduler: a child row indents under its parent; collapsing the parent hides children; deleting the parent does not delete children unless asked. Orca’s 100-deep expanded lineage is a failure mode we should make awkward (cap visible depth, default-collapse grandchildren).

`--prompt` / `--prompt-file` on spawn *is* the handoff. Do not build ask/reply.

### 5. A tiny CLI (`sat`)

Agents already know how to call `orca`. Give them a smaller surface, Unix-socket to the running app (same pattern as Orca’s runtime client, but talking to a Sola process).

v1 commands:

```text
sat project list
sat workspace list [--project <id>]
sat workspace spawn --project <id> --name <name> [--agent grok] [--prompt <text>|--prompt-file <path>] [--parent]
sat workspace rm --workspace <id>          # unregisters; git worktree remove is explicit
sat pane list [--workspace <id>]
sat pane send [--pane <id>] --text <t> [--enter]
sat pane read [--pane <id>] [--lines n]    # last N grid lines, for scripts
sat ps                                     # status table
```

If the app is not running, `sat` starts it (or fails loudly — pick one in the freeze; prefer fail-loudly in v1 so a headless agent does not launch a Wayland window as a side effect).

Do not put this on `solactl` yet. Different audience (agents vs operator).

---

## UI shape

One `iced::application` window (like sola-terminal / sola-monitor), not a shell daemon.

```text
┌──────────── sidebar ────────────┬────────── workspace ──────────┐
│ Sola                         ▾  │  [kvm-perf]  pane 1 │ pane 2  │
│   kvm-perf        ◐ working     │                               │
│     grok          spinner       │     alacritty grid            │
│     shell         dim           │                               │
│   distribution    ✓ done        │                               │
│ Illuno                       ▾  │                               │
│   sc-17947        ? waiting     │                               │
└─────────────────────────────────┴───────────────────────────────┘
```

- Sidebar: project headers + workspace rows + *optional* per-pane agent rows when the workspace is expanded or focused. Spawn sibling lives here (and on the focused workspace).
- Workspace header: name, branch, tiny cwd. No GitHub/Linear chips.
- Body: the existing split-pane terminal. Focus-follows-mouse inside the grid, as today.
- **No right sidebar.** No file tree. No diff view. No preview. The editor is whatever you already use.

This ASCII is a *map*, not a look. The look is designed under **Design law** — do not cargo-cult Orca’s worktree cards, and do not assume today’s kit graphite/sidebar density is the answer.

Done-while-unfocused: emit a shell toast (“kvm-perf · grok is done”). That is the only notification in v1. No sound pack, no mobile push.

---

## Persistence

| What | Where |
|---|---|
| Projects, workspaces, lineage, colors, collapse | bus stickies, new topic(s) under `~/.config/sola/state.toml` (same restart story as terminal tabs) |
| Pane split trees + tmux session names | same stickies, *or* a per-workspace document. Not `TerminalSession`. |
| Live PTY + scrollback | tmux on socket `sola-at` |
| Last hook status | small file `~/.config/sola/agent-terminal/last-status.json` (hydrate on boot, mark unconfirmed) |
| Hook socket | `$XDG_RUNTIME_DIR/sola-at-hooks.sock` |
| CLI socket | `$XDG_RUNTIME_DIR/sola-at-cli.sock` |

Dropping a workspace unregisters metadata and, on confirm, `tmux kill-session` for its panes. `git worktree remove` is a separate, explicit step so a misclick does not delete a dirty tree.

---

## Architecture (when promoted)

```text
sat  ──unix──▶  sola-agent-terminal (iced)
                      │
                      ├── sola-terminal lib  (grid / pty / input)
                      ├── tmux socket sola-at
                      ├── hook socket  (Grok scripts → status)
                      └── sola-bus     (theme, stickies, toasts, app menu)
```

New crate: `crates/sola-agent-terminal` (app + `sat` bin, or `sat` as a second `[[bin]]`).  
Session/desktop: register like other kit apps so the launcher can start it.  
Make/install: a normal `cargo make` target; do not auto-install.

Internal modules (small files, kit style): `project`, `workspace`, `pane`, `status`, `hooks`, `cli`, `sidebar`, plus the reused term engine.

---

## Performance budget (write it down before we repeat Orca)

- Sidebar rebuilds from **one** status snapshot per tick. No per-row subscriptions (Iced is immediate-mode; the failure mode is “rebuild a huge tree every hook,” not Zustand listeners — same shape of bug).
- Default-collapse project groups that are not focused. Do not mount every pane row in every workspace.
- Cap visible lineage depth.
- Grid paint stays on the existing cache/lock protocol; status chrome must not `clear_all_caches`.
- Hook payloads are tiny (state + short tool/prompt). Do not persist assistant novels.

If we ever need 50+ workspaces visible, virtualize the sidebar section. We will not need that on day one if collapse is the default.

---

## Non-goals (v1)

Embedded editor, browser, Design Mode, Linear/GitHub/Jira, SSH/WSL/remote hosts, mobile, plugins, ACP chat, multi-agent mailbox, 15 hook adapters, usage dashboards, auto-rename, unread badges, worktree setup-hook runners, sparse checkouts.

`sola-terminal` remains the untitled-shell app. This app does not replace it.

---

## Suggested build order (after a freeze, not now)

1. **Skeleton app** — kit boot, bus, empty sidebar, one hardcoded project, one pane using the term lib. Own tmux socket.
2. **Status chrome** — design the mark (impeccable + frontend-design first); extend or replace kit indicator as needed; fake states until hooks exist; prove scanability.
3. **Grok hooks + process-tree presence** — real working/waiting/done on a live pane. OSC `9999` parser in the PTY reader (strip from the grid).
4. **Projects + workspaces + spawn sibling** — persist, restore, `git worktree add`, optional `--prompt`, lineage indent, cwd the pane.
6. **Toasts on done**, polish, then stop.

Each step should be dogfoodable alone. Do not start with the CLI, and do not start by extracting the term lib “cleanly” for a week — extract at the moment the second crate needs it.

---

## Open questions (ask when this is promoted)

These are product forks, not things to invent in an ideas file:

1. **Worktree location convention.** **Decided:** `<project-root>/.worktrees/<name>`.
2. **Main checkout as a workspace.** Is the project root itself a first-class workspace (probably yes), or only spawned worktrees?
3. **Killing git worktrees.** Unregister-only vs offer `git worktree remove` in the same dialog?
4. **`sat` when the app is down.** Fail, or launch the Wayland app?
5. **Claude in v1.** Hook installer, or presence-only until Grok is solid?
6. **Name.** `sola-agent-terminal` is accurate and ugly. Fine for a crate; the window title can be shorter (`Agents`, `Workspaces`, …).

---

## Pointers (research)

| Topic | Where |
|---|---|
| Orca status model | `/tmp/orca-research/src/shared/agent-status-types.ts` (or upstream `stablyai/orca`) |
| OSC 9999 | `src/shared/agent-status-osc.ts` |
| Hook server + persist | `src/main/agent-hooks/server.ts` |
| Grok hook install | `src/main/grok/hook-service.ts` |
| Project groups | `src/shared/types.ts` (`ProjectGroup`, `FolderWorkspace`, `Worktree`) |
| Orca CLI surface | `src/cli/specs/core.ts`, `orchestration.ts` |
| Status perf lesson | `docs/reference/renderer-agent-status-performance.md` |
| sola-terminal engine | `crates/sola-terminal/src/{emulator,pty,term_view,state,tmux,sidebar}.rs` |
| Kit status dots | `crates/sola-kit/src/components/sidebar.rs` (`SidebarIndicator`) — starting point, not law |
| Design skills | `.grok/skills/impeccable`, frontend-design; rule `.grok/rules/agent-terminal-design.md` |
| Tab persist (do not reuse) | `crates/sola-bus/src/topics.rs` (`TerminalSession`) |
