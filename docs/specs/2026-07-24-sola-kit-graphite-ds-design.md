# sola-kit graphite design system pass

**Status:** merged to `master` (2026-07-24). Overview + shared chrome done;
other storybook tabs deferred — update on demand when touching components
(see `.grok/rules/kit-storybook-pages.md`).  
**Scope:** sola-kit components + storybook + theme seed (`sola-core` palette;
not `sola-bus` protocol). Other apps later.

## Goal

Move kit chrome from macOS system greys toward a **cool graphite tool UI**: dense controls, soft hairlines, sparse cyan accent, quiet selection.

## Tokens (seed)

| Atom | Hex |
|------|-----|
| bg | `#0c0e12` |
| bg_raised | `#151922` |
| bg_hover | `#1e2533` |
| border | `#2a3344` |
| fg | `#e9ecf2` |
| fg_muted | `#8b94a8` |
| accent | `#3dd6f5` |
| success | `#3ecf8e` |
| warning | `#e8b84a` |
| danger | `#f07178` |
| selection | `#163842` |

Shell defaults: menubar `#050608`, switcher glass `#151922e6`.

## Density

- Radii SM/MD/LG: **5 / 7 / 10** (XL 14)
- `PAD_CONTROL` **[7, 14]**, `PAD_CONTROL_SM` **[5, 11]**
- Field padding **[7, 12]**
- `SIDEBAR_WIDTH` **220**

## Control chrome

- Hairlines: white@7% / white@12% (not solid border atom)
- Primary: filled accent, **dark label**, soft glow
- Secondary: soft fill + strong hairline
- Ghost: muted text at rest
- Danger outline: soft tinted fill
- Badges: soft tone fills + borders
- Cards: soft hairline + light shadow, larger radius
- Fields: inset well, strong hairline, accent focus
- Sidebar selected: selection fill + soft accent edge

## Storybook

- Wider content padding, material header strip, live chip
- Button page product moment (Save / Cancel / Revert / Delete)

## Iced limits accepted

No multi-stop gradients or true backdrop-filter materials. Opaque graphite + soft shadows stand in.

## Verify

```text
cargo make build sola-kit
cargo test -p sola-kit -p sola-core --lib
```
