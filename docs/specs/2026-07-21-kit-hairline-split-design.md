# Kit hairline split dividers

**Date:** 2026-07-21  
**Status:** approved — implement  
**Scope:** `sola-kit` divider / split; terminal layout constant sync  
**Roadmap:** adjacent to P7 kit controls; one signature move  

---

## Goal

Make shared kit split panes read as **macOS Dark Mode** chrome: a quiet
**1px hairline** separator with an **8px invisible hit strip**, so drag
usability is unchanged while the permanent “chonky” gutter goes away.

Terminal inherits via `sola_kit::components::split`. Storybook Split page
is the regression surface. Monitor’s local divider is **out of scope**.

---

## Decision

**Approach 1 — fat hit, thin paint, consumer colours**

| Constant | Value | Role |
|----------|--------|------|
| `DIVIDER_HIT_PX` | `8.0` | Layout thickness + drag hit strip |
| `LINE_PX` | `1.0` | Centre band only — true 1px hairline |
| Side bands | `(HIT − 1) / 2` each | Consumer-owned colours |
| Hover / drag paint | none | Resize cursor only |

**`DividerColors { a, line, b }`:** each consumer matches **a**/**b** to the
surfaces being split so the hit strip is invisible and only the 1px line
shows. Helpers: `uniform`, `from_theme`, `raised`, `raised_to_canvas`.

| Consumer | Colours |
|----------|---------|
| Terminal pane split | `uniform(term_bg, border)` |
| Terminal sidebar edge | raised \| border \| term_bg |
| Storybook card split | `raised(theme)` |
| Browser tab column | `raised_to_canvas(theme)` |

APIs: `vertical_divider_with` / `horizontal_divider_drag_with` / `split_with`
/ `SidebarPanel::resizable_with`. Theme-default variants keep prior call sites.

---

## Widget structure

`vertical_divider` / `horizontal_divider_drag` keep the same public
signatures. Internally:

```
mouse_area(
  container(            // HIT_PX × Fill, transparent background
    centered 1px line   // border-colored fill
  )
)
.interaction(ResizingColumn | ResizingRow)
.on_press(...)
```

- Export `pub const DIVIDER_HIT_PX: f32 = 8.0` from the divider module
  (re-export via `components` if useful).
- Non-interactive `horizontal_divider` remains a true 1px line.
- `split(...)` API unchanged.

---

## Consumers

| Consumer | Change |
|----------|--------|
| **sola-kit storybook** | Visual only; optional copy note about hairline + hit |
| **sola-kit sidebar** | Resize edge uses shared `vertical_divider` (was a solid 6px slab) |
| **sola-terminal** | `state::DIVIDER_PX` sources kit `DIVIDER_HIT_PX`; drop cyan active-pane border (cursor shows focus) |
| **sola-monitor** | No change this pass |

**Out of scope:** inactive pane dimming, 1px layout reservation (no dead
strip — would need overlay hit zones), hover fill, new theme tokens,
monitor migration.

---

## Verification

1. `cargo make build` (kit + terminal)
2. Storybook Split — vertical + horizontal hairlines, no 8px slab
3. User install/smoke: split + drag both axes; nested splits still grabable
