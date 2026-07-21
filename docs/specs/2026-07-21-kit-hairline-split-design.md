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
| Fill (rest of strip) | canvas `background.weakest` | Opaque — not transparent (transparent read as a dark gutter) |
| Hover / drag paint | none | Resize cursor only |

**Color:** hairline uses kit border atom (`background.stronger`); strip
fill uses canvas base.

**Iced pitfall:** `Container::center_x(Length::Fill)` / `center_y` **replace**
width/height with the argument. Using them to center the 1px line collapses
the hit strip to flex `Fill` (≈1px between `FillPortion` panes). Center with
`align_x` / `align_y` and keep `width(Fixed(DIVIDER_HIT_PX))`.

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
