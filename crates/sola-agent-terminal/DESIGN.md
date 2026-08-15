---
name: sola-agent-terminal
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
Workspace title 14, subtitle 11 muted. Agent name (who) is a trailing
secondary, never inside the mark.

## Layout

```
[ PROJECT                    + ]
[ ●  root              who     ]
[ ●  workspace-a       who     ]
              terminal grid →
```

Mark slot is always 12×12 so titles do not shift. Group `+` opens a
name-only modal (worktree + branch). The new pane is a shell — start
grok yourself.

## Shapes

- **Working** — open ring, round-cap stroke, accent, ~0.85s spin
- **Waiting** — filled diamond, warning
- **Done** — two-stroke check, success
- **Idle** — dim disc, same slot
- **Active** (generic kit apps) — filled success disc; unchanged

Motion is state only (the working ring). No page-load choreography.

## Components

Kit `SidebarPanel` + `SidebarIndicator` / `status_mark`. Section labels
toggle collapse; section `+` opens the name modal. App-local: catalog,
modal, drop confirm. Do not restyle mail / settings / terminal.

## Do's and Don'ts

- Do reserve the mark slot on every row, including idle.
- Do keep who (agent name) separate from state (the mark).
- Do put `+` on the project group, not a form in the rail.
- Do toast done only when unfocused: `{workspace} · grok is done` (menubar).
- Don't infer status from OSC 0/2 titles.
- Don't cargo-cult Orca worktree cards or amber-everything dots.
- Don't put siblings anywhere but `<root>/.worktrees/<slug>`.
