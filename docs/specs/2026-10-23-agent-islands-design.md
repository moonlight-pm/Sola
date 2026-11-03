# Agent islands — get back to an agent from the top row

**Date:** 2026-10-23
**Status:** **Accepted** 2026-10-23 (Joshua: "go ahead"); built as Phase 14
on `next`, applied and smoked on canto (a permission prompt unsmoked).
**Amended** 2026-11-03: the card is gone (Joshua: "they open a sort of popup
to do interaction right there, but thats not necessary, clicking them should
just take me to the window. Let's give them a 'hover close' just to dismiss
them as well"). A2, A3 and A4 below are superseded.
**Related:** [Next](2026-10-21-next-tab-bar-design.md) (board 03, the top
row is islands; board 05/06, Projects' sessions), plan
[`plans/2026-10-21-next-tab-bar-plan.md`](../plans/2026-10-21-next-tab-bar-plan.md)
Phase 14.

## Why

An agent session in Projects stops to ask (a permission, a question) or
finishes its turn while you are in another window. Today the bar's row
for its project gets a diamond and nothing more: you go there to see what
it wants. The ask comes up as a small island in the top row, and a click
takes you to the session.

## Decided (Joshua, 2026-10-23)

| | Decision |
|--|--|
| A1 | **One island per session** that needs you, on the **left of the top row, after the flower** (board 03's "Agents on the left"). |
| A2 | ~~An answer reaches the agent as keystrokes into its terminal.~~ Superseded 2026-11-03: nothing is answered from the row. |
| A3 | ~~A finished session's card shows its last message and a reply box.~~ Superseded 2026-11-03: no card. |
| A4 | ~~A separate button pulls the session's tab up.~~ Superseded 2026-11-03: **a click on the island does.** |
| A5 | An island goes when you **click or dismiss it**, or when you **look at that session's tab** yourself; the session you are looking at gets none. **While the desk is idle-fading, nothing is being looked at** (2026-10-24, Joshua: "the island should appear if we are in inactivity mode"): a session that finishes or asks then gets its island even with its tab focused, and the fade leaves the islands in full sight. Coming back and looking at the focused tab hides it. |

## The islands

- Right of the flower, a gap apart, one island per session that is
  **waiting** (a diamond, board 03's mark) or **done** (a check). Newest
  first. An island reads `◆ Sola (menubar-pass) wants to run cargo make
  build` / `◆ Sola (kit-foundation) asks: Which layout?` / `✓ Sola (root)` —
  the project in ink, its tab in parentheses and what it wants quieter; a
  finished one's check says the rest (Joshua, 2026-10-24: "root for what?").
- One line, always, 340 px at most. The tab and what it wants share the
  room: each is as wide as it needs while the other can give way, and two
  long ones get half each, so a long session name never pushes out the ask.
- They take what room the left side has (to the centre); past that, a
  **+N** island lists the rest.
- Working sessions get no island (the bar's row already rings).

## What you do with one

- **Click:** the session's tab is selected and its window comes up, on
  whichever workspace it is; the island goes (A5: you are looking at it).
- **Hover:** the island lifts and a **×** takes the mark's place, so the
  words do not move. Click it to **dismiss**: the session carries on, and
  the island stays gone until the session next asks or finishes (a new need).
- **The +N list:** the same two on each row, the × always showing. Closing a
  row leaves the list up; the list goes with its last row.

## Where the content comes from

Projects already knows each session's state from the agents' hooks
(`hooks/map.rs`: working / waiting / done). It keeps, per session, **what it
asks**. Claude: `PreToolUse` / `PermissionRequest` carry `tool_name` and
`tool_input` (a Bash `command`, an Edit's `file_path`, `AskUserQuestion`'s
first question). Others: "is waiting".

**Only a change makes an island.** A status Projects restored from before a
restart, or first sees, makes none; islands left from a previous run go when
Projects starts.

## Mechanics

- **Bus.** Projects publishes sticky `Topic::AgentIsland { key, project,
  name, state (waiting / done), title, since }`, keyed by the session (`key`),
  and retracts it when the session works again, closes, is looked at (its tab
  selected in a focused project window while the desk is not idle-fading —
  `Topic::IdleFade` from sola-river), or is dismissed. The shell sends
  `Topic::AgentIslandAction { key, action: Show | Dismiss }`.
- **Projects** marks the need seen for either, so it does not come back while
  the session stays as it is, and says so on the session's computer's hook
  log; for Show it selects the tab and shows its window (`TabWindows::show`).
- **Shell.** The islands in `agents/view.rs`, placed from the top row's left
  group in `menubar/`. The width comes from `fonts::line_width`, which
  measures as a `text` widget lays out, and the text does not wrap.

## Not now

Islands for other apps (a meeting, a download), board 03's centre "live"
island, approving from a notification card, answering from the row at all
(built 2026-10-23, removed 2026-11-03).
