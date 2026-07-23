# Active work

This file is auto-loaded every session (`.grok/rules/*.md`).

## When the user says go

If the user signals go-ahead **without naming a new task** — e.g. "go", "ok go",
"do it", "continue", "keep going", "ship it", "proceed", "lfg", "get on with it",
or similar — **do not re-plan, re-audit, or re-explore**. Execute **Current**
below from the listed next phase, using the linked plan / backlog.

If Current is `none`, ask what they want instead of inventing work.

## Current

**sola-agent UI backlog** — branch `agent-acp-runner` in
`.worktrees/agent-acp-runner`

Design: `docs/specs/2026-07-23-sola-agent-acp-runner-design.md`  
Backlog detail: `docs/specs/2026-07-23-sola-agent-ui-backlog.md`

### Status

v1 ACP runner (Grok stdio) is live and polished enough to use. Next work is
the **UI backlog** below — do **not** merge to master until the user signs
off (merge is still gated on approval).

### Star (pin) — answered

The ★ control **pins** a session in the Sola overlay (`~/.config/sola/agent/overlay.json`).
Pinned sessions sort to the top of the sidebar. It is **not** a Grok-native
feature; it is Sola-only chrome. Keep unless the user asks to remove pins.

### Backlog (ordered for implementation)

When user says **go**, execute phases **A → I** in order (or a named phase if
they specify). Each phase is one focused worktree pass; install only with
explicit permission.

| Phase | Item | Notes |
|---|---|---|
| **A** | **Composer: roomier, fill width, no Send button** | Enter to send; Shift+Enter newline if feasible; taller min height; full available width (drop max-width shell or widen). Stop stays when streaming. |
| **B** | **Sidebar fills vertical space** | Kit raised column must `height: Fill`; list scroll region takes remaining space between header and cwd footer. |
| **C** | **Show cwd on each session row** | Secondary line: short path (`~/…`) of that session’s project dir (from `summary.json` / `info.cwd`), not only relative time. |
| **D** | **New session: pick project dir** | Before `session/new`, UI to choose cwd (default current / last / browse). Persist last cwd in overlay. |
| **E** | **Transcript density + width** | Larger body type (e.g. 15–16px); chat column uses more horizontal space (raise/remove `CHAT_MAX`). |
| **F** | **Markdown rendering** | Render assistant (and user?) markdown: headings, lists, code fences, links, bold/italic. Prefer a small pure-Rust md→iced path or existing crate; no WebView. |
| **G** | **Token usage display** | Status: **percent** + **`nnnK/500K`** (or actual window size from `usage_update` / Grok signals). e.g. `51% · 258K/500K`. Prefer ACP `usage_update`; fallback Grok `_meta` / `signals.json`. |
| **H** | **Rename session titles** | Double-click or edit affordance on sidebar title; persist via Grok if API exists, else overlay title override (document which). |
| **I** | **Lazy transcript load + scroll-to-bottom** | On select: load **tail only** (most recent N turns / bytes of `updates.jsonl` or ACP history if available); scroll to bottom. On scroll-up near top: page in older chunks. Do **not** parse entire multi‑MB jsonl into iced state at once. |

### Future (not this backlog)

- Leader daemon (`ConnectionMode::Leader`) — multi-client, survive UI quit  
  (see ACP runner design “Future: Agent leader daemon”)
- Merge `agent-acp-runner` → master (explicit user approval)
- Remove stale `.worktrees/sola-agent` after merge

### Last completed

**Agent ACP runner v1 + first UI polish** — Grok stdio ACP client, hybrid
sessions, kit sidebar/composer/status; installed for smoke.

### How to resume next session

1. Work in `.worktrees/agent-acp-runner` on branch `agent-acp-runner`.
2. Read `docs/specs/2026-07-23-sola-agent-ui-backlog.md`.
3. On **go**: start at next unchecked phase (default **A**).
4. Install only when user grants permission for that install.
