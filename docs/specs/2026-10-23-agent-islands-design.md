# Agent islands — answer an agent from the top row

**Date:** 2026-10-23
**Status:** **Accepted** 2026-10-23 (Joshua: "go ahead"); built as Phase 14
on `next`, applied and smoked on canto (a permission prompt unsmoked).
**Related:** [Next](2026-10-21-next-tab-bar-design.md) (board 03, the top
row is islands; board 05/06, Projects' sessions), plan
[`plans/2026-10-21-next-tab-bar-plan.md`](../plans/2026-10-21-next-tab-bar-plan.md)
Phase 14.

## Why

An agent session in Projects stops to ask (a permission, a question) or
finishes its turn while you are in another window. Today the bar's row
for its project gets a diamond and nothing more: you go there to see what
it wants. Board 03 drew the answer: the ask comes up as a small island in
the top row, and the island grows into a card where you answer.

## Decided (Joshua, 2026-10-23)

| | Decision |
|--|--|
| A1 | **One island per session** that needs you, on the **left of the top row, after the flower** (board 03's "Agents on the left"). |
| A2 | An answer reaches the agent as **keystrokes into its terminal**, as you would type them — so Claude, Codex and Grok all work. |
| A3 | A **finished** session's card shows **its last message and a reply box**; replying does **not** jump to its tab. |
| A4 | A separate button **pulls the session's tab up** (no answer). |
| A5 | An island goes when you **answer it**, or when you **look at that session's tab** yourself; the session you are looking at gets none. |

## The islands (proposed)

- Right of the flower, a gap apart, one island per session that is
  **waiting** (a diamond, board 03's mark) or **done** (a check). Newest
  first. An island reads `◆ menubar-pass wants to run cargo make build` /
  `◆ kit-foundation asks: Which layout?` / `✓ mail-focus finished` — the
  session's name in ink, the rest quieter, truncated to fit.
- They take what room the left side has (to the centre); past that, a
  **+N** island lists the rest (the same cards, one at a time).
- Working sessions get no island (the bar's row already rings).

## The card (proposed)

Click an island: it grows into a card under it (board 03's "the live
bubble becomes the card"), a panel like the clock's.

| | Waiting — a permission | Waiting — a question | Done |
|--|--|--|--|
| Head | ◆ name · project · how long ago | ◆ name · project · ago | ✓ name · project · ago |
| Body | What it wants to do: the command, the file, the URL | The question (each, if several) | Its last message (scrolls) |
| Act | The prompt's choices as buttons (*Yes*, *Yes, don't ask again*, *No*) | The question's options as buttons; *Other…* opens a text field | A reply field — Return sends |
| Also | **Show** (A4) · Esc closes the card | Show · Esc | Show · Esc |

- Choosing sends the keys and the card goes; the island goes when the
  session moves on (working) — usually at once.
- **Show** selects the session's tab and brings its project window up;
  the island goes with it (A5: you are looking at it).
- Esc, or a click elsewhere, closes the card; the island stays.

## Where the content comes from (proposed)

Projects already knows each session's state from the agents' hooks
(`hooks/map.rs`: working / waiting / done). It keeps, per session:

- **What it asks.** Claude: `PreToolUse` / `PermissionRequest` carry
  `tool_name` and `tool_input` (a Bash `command`, an Edit's `file_path`,
  `AskUserQuestion`'s `questions` with their options). Others: the
  screen.
- **The choices.** Read off the pane's screen a moment after the ask
  (its last lines): the prompt's numbered lines (`1. Yes` …) are the
  buttons, and the digit is the key. Agent-agnostic, and right whatever
  the TUI's wording. A "Type something" option is not a button: the
  card's reply field picks it and types the answer.
- **The last message** (done). Claude: the session's transcript, the
  turn's own answer (Claude writes it a moment after its Stop hook, so it
  is read again until it is there). Others: the end of the screen, the
  input box left out.
- **Only a change makes an island.** A status Projects restored from
  before a restart, or first sees, makes none; islands left from a
  previous run go when Projects starts.

## Mechanics (proposed)

- **Bus.** Projects publishes sticky `Topic::AgentIsland { key, project,
  name, state (waiting / done), title, body, choices: [{label, keys}],
  reply: bool, since }`, keyed by the session (`key`), and retracts it
  when the session works again, closes, is answered, or is looked at
  (its tab selected in a focused project window). The shell sends
  `Topic::AgentIslandAction { key, action: Choose(i) | Reply(text) |
  Show }`.
- **Projects** turns an action into keystrokes into the pane (`pane.send`:
  the choice's keys; a reply's text then Return), or, for Show, selects
  the tab and shows its window (`TabWindows::show`).
- **Shell.** The islands in `menubar/` (left, after the flower); the card
  a menu-overlay panel anchored under its island (like the clock's).
  While a card is open its first action is the default (Return) and the
  reply field has the keyboard.

## Not now

Islands for other apps (a meeting, a download), board 03's centre "live"
island, answering several questions of one `AskUserQuestion` in one card
(the first is shown; Show for the rest), approving from a notification
card.
