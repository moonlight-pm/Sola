# Sola Agent — UI backlog (post-v1)

**Date:** 2026-07-23  
**Branch:** `agent-acp-runner`  
**Parent design:** `2026-07-23-sola-agent-acp-runner-design.md`  
**Tracker:** `.grok/rules/active-work.md` (Current)

User feedback after first polish screenshot. Implement in phases A–I.
Do not invent extra scope.

---

## Star (pin) — clarified

| Control | Meaning |
|---|---|
| ★ / ☆ | **Pin** session in Sola’s overlay store. Pinned rows sort first. |
| Storage | `~/.config/sola/agent/overlay.json` → `pinned: [...]` |
| Not | Not a Grok TUI flag; not “favorite project”; not multi-select |

If pins feel noise, later option: pin only via context menu / long-press.

---

## Phase A — Composer

**Goals**

- Text entry uses **full available width** of the main pane (no narrow centered shell, or much wider max).
- **Roomier** field: more vertical padding / min height (multi-line feel).
- **No Send button.** Submit on Enter (document Shift+Enter = newline if iced supports it; otherwise Enter sends and note the limitation).
- Keep **Stop** while `streaming`.

**Acceptance**

- Composer visually balanced; typing area is the primary control.
- Enter starts a turn when not gated by permission.

---

## Phase B — Sidebar vertical fill

**Goals**

- Left session column spans full window height (kit raised bg edge-to-edge under menubar).
- Header (Sessions + New) fixed top; **cwd footer** fixed bottom; **list scrolls** the middle.

**Acceptance**

- No empty gap under the list that isn’t the scrollable region.
- Footer path always visible.

---

## Phase C — Cwd on each session row

**Goals**

- Each row shows:
  1. Title  
  2. **Project path** (shortened `~/…`) from session `info.cwd`  
  3. Relative time (can share the secondary line or a third caption)

**Acceptance**

- Two sessions in different projects are distinguishable without opening them.

---

## Phase D — New session project picker

**Goals**

- **New** does not silently use process cwd only.
- Flow: choose project directory → then `session/new { cwd }`.
- Affordances (pick one, keep simple):
  - default = last overlay cwd or current
  - recent projects list (from session index / overlay)
  - optional path text field or directory chooser if iced allows; otherwise text field + “Use”

**Acceptance**

- Can start a session under `~/Workspace/Other` without restarting the app from that directory.
- Last choice persisted in overlay.

---

## Phase E — Transcript type + width

**Goals**

- Body text **larger** (target ~15–16px for message body; captions stay smaller).
- Content column uses **more width** (raise or remove `CHAT_MAX` 720; e.g. 960–1200 or `Fill` with comfortable side padding only).

**Acceptance**

- Readable on a half-screen ~1400px-wide agent window without sparse margins.

---

## Phase F — Markdown rendering

**Goals**

- Assistant text rendered as markdown (at least: paragraphs, **bold**, *italic*, `` `code` ``, fenced code blocks, lists, headings, links as underlined/accent text).
- Streaming: either re-parse full buffer each delta (simple) or append plain until turn end then re-render (acceptable v1 of this phase).
- No WebView / HTML engine.

**Acceptance**

- Code fences monospace + distinct background.
- Lists and headings visibly structured.

**Implementation hint**

- Evaluate small crates (`pulldown-cmark` → iced widgets) vs hand subset; prefer pulldown-cmark walk → column of kit text/code blocks.

---

## Phase G — Token usage format

**Goals**

- Status bar shows: **`{pct}% · {usedK}K/{sizeK}K`**  
  Example: `51% · 258K/500K`
- `size` from ACP `usage_update.size` or Grok window (500000 typical).
- `used` from `usage_update.used` or best available (`totalTokens` / signals).
- Hide or show “—” only when completely unknown.

**Acceptance**

- Matches user mental model of Grok TUI context meter.

---

## Phase H — Rename session titles

**Goals**

- User can rename the display title of a session from the sidebar (inline edit or dialog).
- Persistence preference order:
  1. Grok-native rename if available (CLI / ACP / summary write if safe)
  2. Else **overlay title override** keyed by session id (document that TUI still shows Grok title until native rename exists)

**Acceptance**

- Rename survives app restart.
- Sidebar and window title update immediately.

---

## Phase I — Lazy history + scroll to bottom

**Goals**

1. **On select/load:** do not load entire `updates.jsonl` into memory/UI.  
   - Load a **tail window** (e.g. last N events or last ~256–512 KiB of the file).  
   - Rebuild turns from that tail only.  
2. **Scroll to bottom** after load and after each streaming delta (unless user has scrolled up — optional “stick to bottom” flag).  
3. **Load older on demand:** when scroll position near top, fetch previous chunk and **prepend** turns; preserve scroll anchor as much as iced allows.

**Acceptance**

- Opening a 1000+ message Grok session does not freeze the UI.
- First paint shows the **latest** messages; scrolling up reveals older ones.
- New messages while at bottom keep viewport pinned to bottom.

**Implementation hint**

- Byte-offset index or reverse line scan for jsonl; keep `history_cursor` in session UI state.
- ACP `session/load` may still full-load server-side — client display path must still be lazy even if agent has full context.

---

## Out of scope for this backlog

- Multi-agent picker  
- Full TUI slash parity  
- Fugu/Sakana revival  

(Leader multi-client attach is product default — see ACP runner design.)

---

## Progress

| Phase | Status |
|---|---|
| A Composer | done |
| B Sidebar fill | done |
| C Row cwd | done |
| D Project picker | done |
| E Type + width | done |
| F Markdown | done |
| G Token format | done |
| H Rename | done |
| I Lazy load + scroll bottom | done |
