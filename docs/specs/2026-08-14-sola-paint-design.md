# sola-paint — default image app

**Date:** 2026-08-14  
**Status:** first pass + singleton + zoom/pan in code (`naturalethic/sola-paint`)  
**Related:** [preview freeze](2026-08-04-sola-preview-and-selection-capture-design.md) (screenshot dest is **preview**)

| | |
|--|--|
| **Implementation** | New crate `crates/sola-paint`. MIME + argv + `solactl open` image paths. Second spawn hands off via `OpenImage` (`app_id=sola-paint`). Wheel/drag zoom-pan; crop maps through the live dest. Left `SidebarPanel` Large tabs. Crop / rotate / flip / undo / save. Open/Save via kit `FilePicker`. |
| **Dogfood** | `paint` + `kit` installed locally; FilePicker used. Singleton + zoom/pan need reinstall `paint`. Screenshots stay on preview (need `install shell` if that dest was flipped). |
| **Gaps** | No clipboard image, no adjust/filters, no undo-after-save distinction. Unsaved buffers are not persisted. Crop shortcut is **⌘⇧K** (⌘K is the shell shortcuts overlay). |

## Intent

Sola needs one default place images land — file open, MIME, `solactl open`, and screenshots — with enough editing to crop and save. Not a Photoshop. Graphite tool UI, kit chrome.

## Locked (first pass)

| Topic | Decision |
|-------|----------|
| App | `sola-paint` kit iced app; `app_id` matches binary |
| Default dest | MIME `image/*` via `sola-paint.desktop`; `Topic::OpenImage` with default/`sola-paint` dest |
| Preview | Argv / launcher viewer; consumes `OpenImage` only when `app_id=sola-preview`. Shell hotkeys copy to the clipboard. |
| Chrome | Left tab strip (`SidebarPanel` Large) + top tool strip + checker stage |
| Edits | Crop (drag + Apply), rotate 90°, flip H/V, 8-step undo, save / save-as |
| View | Wheel zoom toward cursor; drag to pan; ⌘+/⌘−/⌘0 |
| Formats | PNG, JPEG, GIF, WebP, BMP, TIFF |
| Single-instance | Second `sola-paint` spawn emits `OpenImage` and exits |
| Session | Bus `PaintSession` (`~/.config/sola/paint.yaml`): tab paths + selected. Missing files skipped. |

## Out of scope

Selection tools, layers, color adjust, clipboard image, print, export presets.
