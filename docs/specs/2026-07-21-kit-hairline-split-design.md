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

**Approach 1 — fat hit, thin paint**

| Constant | Value | Role |
|----------|--------|------|
| `DIVIDER_HIT_PX` | `8.0` | Layout thickness + drag hit strip |
| Line | `1.0` | Centered hairline |
| Fill (rest of strip) | transparent | Underlying surface shows through |
| Hover / drag paint | none | Resize cursor only |

**Color:** kit `border` path (`extended_palette` background strong /
border atom already used for hairlines) — **not** `background.stronger`
slab fill.

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
| **sola-terminal** | `state::DIVIDER_PX` sources kit `DIVIDER_HIT_PX` so rect / drag math stays aligned |
| **sola-monitor** | No change this pass |

**Out of scope:** terminal active-pane accent border, inactive dimming,
1px layout reservation, hover fill, new theme tokens, monitor migration.

---

## Verification

1. `cargo make build` (kit + terminal)
2. Storybook Split — vertical + horizontal hairlines, no 8px slab
3. User install/smoke: split + drag both axes; nested splits still grabable
