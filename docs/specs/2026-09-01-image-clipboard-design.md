# Image clipboard

**Date:** 2026-09-01  
**Status:** Frozen — implemented in `sola-kit` + `sola-preview` + CEF paste  
**Related:** [preview](2026-08-04-sola-preview-and-selection-capture-design.md); [paint](2026-08-14-sola-paint-design.md); [kvm clipboard](2026-07-30-sola-kvm-clipboard-design.md) (text only)  
**Implementation:** Preview **Copy** (image bytes) next to **Copy path**. Wrapper / browser ⌘V pastes an image File into the focused frame. Super+Shift+3/4/5 Fast-encode PNG onto the compositor clipboard (shell owns the offer); no file, no Preview. **Installed** `kit`+`preview`+`browser`+`wrapper` debug 2026-09-01; screenshot dest + promised paste + Fastest **installed** `kit`+`shell` release 2026-09-01.  
**Gaps:** paint has no image paste yet; kvm stays text; screenshot → Slack desk smoke.

## Intent

Put real image bytes on the compositor clipboard so a screenshot in Preview can be pasted into native Wayland clients **and** CEF wrappers (Slack). Iced’s clipboard is text-only; Chromium OSR does not share that clipboard with Wayland.

## Why this shape

| Constraint | Consequence |
|------------|-------------|
| `iced::clipboard::{read,write}` is `String` | Cannot offer `image/png`. Do not extend iced for v1. |
| smithay-clipboard **read** can drop the current offer (browser already writes text back after ⌘V) | CEF page paste must **not** go through iced when the offer is an image. |
| Windowless CEF (`--ozone-platform=wayland`) has no working seat clipboard | `frame.paste()` is empty and can **replace** the offer with nothing. Keep the existing JS inject, extended to files. |
| Wayland sources must stay alive to serve bytes | A background thread in the copying process serves `wlr-data-control` / `ext-data-control` (same protocol as `wl-copy`). Closing that process drops the clipboard. |
| Mixed `text/uri-list` + `image/png` | Some clients paste the path. **Copy** offers **only** the image MIME. **Copy path** stays a separate text write. |

River already advertises data-control (kvm’s `wl-copy` path). Kit talks to it in-process via `wl-clipboard-rs` (serve on a thread, **not** `fork` — iced/wgpu processes must not fork).

## Offer

| Action | MIME | Body |
|--------|------|------|
| Super+Shift+3/4/5 | `image/png` (Fast zlib) | In-memory encode; shell serves the offer. No disk PNG. |
| Preview **Copy** | `image/png` (or jpeg/gif/webp/bmp from sniff/ext) | File bytes as stored. No re-encode. |
| Preview **Copy path** | text (iced / smithay) | Absolute path. Unchanged. |
| Cap | — | 32 MiB compressed payload. |

No `text/plain`, no `text/uri-list` on the image action.

## Read (CEF page paste)

On Edit Paste / page-menu Paste (not the omnibox, not vault fields):

1. Probe MIME types with data-control.
2. If an image MIME is offered, read those bytes (prefer `image/png`).
3. Inject a synthetic `paste` `ClipboardEvent` whose `clipboardData` holds a `File` (focused frame only — same rule as `PasteText`).
4. Else read text via data-control and `PasteText`.
5. If data-control is unavailable, fall back to iced text read + write-back (today’s path).

Do **not** call `frame.paste()` for this.

URL-bar and vault paste stay text-only (iced).

## v1 surface

- Preview header: **Copy** then **Copy path** (both compact secondary). **Copied** flashes on the button that succeeded.
- Super+Shift+3/4/5 copy a Fast PNG to the clipboard and toast **Screenshot copied**. They do **not** write a PNG or open Preview. `solactl compositor screenshot` still writes a file (also Fast PNG).

## Non-goals

Clipboard history, paint paste/copy, kvm image sync, primary selection, toast-click-to-open Preview.

## Key decisions

| Decision | Choice |
|----------|--------|
| Protocol | `ext-data-control` / `wlr-data-control` via `wl-clipboard-rs` |
| Home | `sola_kit::clipboard` (kit apps). kvm keeps its own text helper. |
| Image MIME | File bytes, sniff then extension. PNG for screenshots. |
| CEF | Chrome reads Wayland; focused-frame JS `File` paste. |
| Lifetime | Screenshot offers: **shell** (session-long). Preview **Copy**: preview process. |
| Encode | `png::Compression::Fastest` (fdeflate + Up). `Fast` still uses Adaptive filters (~2s on 5K). |
| Promise | Advertise `image/png` at the chord; fill the Wayland pipe on Send. Slack ⌘V can land before encode finishes. |
