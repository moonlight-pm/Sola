# sola-paint — default image app

**Date:** 2026-08-14  
**Status:** first pass in code (`naturalethic/sola-paint`)  
**Related:** [preview freeze](2026-08-04-sola-preview-and-selection-capture-design.md) (screenshot capture still lives there; **destination is now paint**)

| | |
|--|--|
| **Implementation** | New crate `crates/sola-paint`. MIME + `OpenImage` + argv + `solactl open` image paths. Shell screenshots open/raise paint. Left `SidebarPanel` Large tabs. Crop / rotate / flip / undo / save. Open/Save via kit `FilePicker`. |
| **Dogfood** | `paint` + `kit` installed locally; FilePicker used. Screenshot dest still needs `install shell`. |
| **Gaps** | No single-instance (second spawn is a new process). No zoom/pan, no clipboard image, no adjust/filters, no undo-after-save distinction. Crop mapping assumes the last stage size. |

## Intent

Sola needs one default place images land — file open, MIME, `solactl open`, and screenshots — with enough editing to crop and save. Not a Photoshop. Graphite tool UI, kit chrome.

## Locked (first pass)

| Topic | Decision |
|-------|----------|
| App | `sola-paint` kit iced app; `app_id` matches binary |
| Default dest | MIME `image/*` via `sola-paint.desktop`; `Topic::OpenImage`; screenshot handoff |
| Preview | Remains a standalone argv viewer; no longer consumes `OpenImage` |
| Chrome | Left tab strip (`SidebarPanel` Large) + top tool strip + checker stage |
| Edits | Crop (drag + Apply), rotate 90°, flip H/V, 8-step undo, save / save-as |
| Formats | PNG, JPEG, GIF, WebP, BMP, TIFF |
| Single-instance | Not this slice (same gap as browser URL open) |

## Out of scope

Zoom, selection tools, layers, color adjust, clipboard image, print, export presets.
