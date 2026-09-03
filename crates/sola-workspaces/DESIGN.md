---
name: sola-workspaces
description: Operate rail — status marks are the product
colors:
  canvas: "#0c0e12"
  raised: "#151922"
  hover: "#1e2533"
  fg: "#e9ecf2"
  fg-muted: "#8b94a8"
  accent: "#3dd6f5"
  success: "#3ecf8e"
  warning: "#e8b84a"
typography:
  ui:
    fontFamily: "SF Pro Text, Inter, system-ui, sans-serif"
    fontSize: "14px"
    fontWeight: 400
    lineHeight: 1.2
    letterSpacing: "normal"
  ui-caption:
    fontFamily: "SF Pro Text, Inter, system-ui, sans-serif"
    fontSize: "11px"
    fontWeight: 400
    lineHeight: 1.2
    letterSpacing: "normal"
  mono:
    fontFamily: "Iosevka Term Slab, JetBrains Mono, ui-monospace, monospace"
    fontSize: "15px"
    fontWeight: 400
    lineHeight: 1.2
    letterSpacing: "normal"
spacing:
  mark-slot: "12px"
  sidebar-default: "240px"
components:
  status-working:
    textColor: "{colors.accent}"
    size: "{spacing.mark-slot}"
  status-waiting:
    textColor: "{colors.warning}"
    size: "{spacing.mark-slot}"
  status-done:
    textColor: "{colors.success}"
    size: "{spacing.mark-slot}"
  status-idle:
    textColor: "{colors.fg-muted}"
    size: "{spacing.mark-slot}"
---

# DESIGN

Recorded from the built status-chrome surface. Tokens ride the Sola bus /
sola-kit atoms — do not fork a private palette.

## Overview

Operate mode. Left rail (projects as quiet section headers, workspaces as
rows) + right terminal grid. Status is the product. One signature: the
reserved status mark. Everything else stays graphite and quiet.

## Colors

Restrained. Canvas / raised / hover from kit graphite. Accent (`#3dd6f5`)
only for selection and the **working** ring. Warning diamond for waiting.
Success check for done. Idle is foreground at ~40% alpha. Never infer
state from hue alone — shape differs too.

## Typography

`fonts::ui()` on chrome. `fonts::mono()` on the grid. No display face.
Workspace title 14. No agent name on the row — status is the mark, the
desk card names project, tab, and agent. Sibling rows use the kit hover × (`on_close`, lucide/x).
Root has no row close. **Project** menu: New Project…, Startup Script…,
Drop Project. **Edit** menu: Copy / Paste (⌘C / ⌘V) — script editor when
open, otherwise the focused pane (same as sola-terminal). Startup is a per-project `/bin/sh` run in each new
worktree (copy `.grok`, etc.). Script env: `$PROJECT` (folder on disk),
`$WORKTREE` (this tab), `$NAME` (tab name). The editor lists them. **Drop Project** unregisters the project
and kills its tmux. Worktrees stay on disk.
⌘W closes the focused **pane**. A workspace is always one rail row —
splits stay in the grid, not as child tabs. The mark rolls up every
Grok pane in that tab (waiting beats working beats done beats idle).

## Layout

```
[ PROJECT                    + ]
[ ●  root                      ]
[ ●  workspace-a               ]
[ NEXT PROJECT               + ]
              terminal grid →
```

Mark slot is always 12×12 so titles do not shift. Groups stack at the
top; a lone project may fill to scroll. Group `+` opens a name-only
modal (worktree + branch). The new pane is a shell — start grok yourself.

## Shapes

- **Working** — open ring, round-cap stroke, accent, ~0.85s spin
- **Waiting** — filled diamond, warning
- **Done** — two-stroke check, success
- **Idle** — dim disc, same slot
- **Active** (generic kit apps) — filled success disc; unchanged

Motion is state only (the working ring). No page-load choreography.

## Components

Kit `SidebarPanel` + `SidebarIndicator` / `status_mark`. Section labels
toggle collapse; section `+` opens the name modal. Hover close is kit
`SidebarItem::on_close` (not the session-card trash). App-local: catalog,
modal. Do not restyle mail / settings / terminal.

## Do's and Don'ts

- Do reserve the mark slot on every row, including idle.
- Do keep who off the row; state is the mark. The desk card names
  project, tab, and agent.
- Do stack project groups at the top (do not fill the selected group).
- Do use the kit hover × on siblings only; never on root.
- Do drop the **project** from the menu only (unregister + kill every
  tmux session in the group). Hover × never `git worktree remove`.
  CLI `workspace.rm --worktree` is the explicit checkout delete; a gone
  path reaps the tab.
- Do show **Start new shell** only when the **last** pane's PTY has
  exited (Ctrl-D). A split leaf that dies just closes. Hover must not
  start a shell — only the button (or a sidebar click that attaches
  every live leaf).
- Do show a quiet `×N` on the workspace row when a Grok pane in that
  tab has compacted (loudest session: `compaction/segment_*.md`,
  checkpoints, then `signals.json` `compactionCount` — Grok often
  leaves the signal at 0). Shell panes do not contribute.
- Do keep splits off the rail. One row per workspace; the mark watches
  every Grok pane in the tab. Waiting (needs attention) beats working
  beats done beats idle. A grok+shell split looks like a single pane.
- Do return the mark to idle (grey disc) when no Grok pane is live
  (every leaf is a shell). `/exit` / process gone is idle, not a stuck
  done check.
- Do put `+` on the project group, not a form in the rail.
- Do notify done and waiting only when unfocused (desk card, not
  menubar whisper): title `{project} · {tab}`, body `grok is done` /
  `grok needs attention`. Tab is the rail label (`root`, slug, or
  `slug · title`).
- Do bind ⌘T spawn sibling, ⌘N new project, ⌘⇧↓ split down, ⌘⇧→ split
  right, ⌘W close pane.
- Don't infer status from OSC 0/2 titles.
- Don't cargo-cult Orca worktree cards or amber-everything dots.
- Don't put siblings anywhere but `<root>/.worktrees/<slug>`.
