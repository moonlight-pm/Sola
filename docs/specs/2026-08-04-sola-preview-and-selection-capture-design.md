# sola-preview + selection capture — Design

**Date:** 2026-08-04  
**Status:** approved (approach A); capture still as-built; **screenshot dest is sola-preview** (paint is MIME / `solactl open` dest)  
**Depends on:** `docs/specs/2026-07-20-screenshot-capture-plan.md` (screencopy live); [paint](2026-08-14-sola-paint-design.md)

## 1. Goal

1. **sola-preview** — kit app that shows screenshots (macOS Preview–like for captures).
2. Shell hotkeys open/raise preview with the new image and keep a short toast.
3. **Selection capture** via Super+Shift+4 (macOS order), with chords remapped.

## 2. Product decisions

| Decision | Choice |
|----------|--------|
| Architecture | Shell orchestrates; river captures; preview is a normal kit app |
| Chords | Super+Shift+3 full · **4 selection** · **5 focused window** |
| Selection UI | Shell full-screen marquee overlay |
| After capture (shell hotkeys) | Toast path **and** open/raise sola-preview |
| Already open | Replace main view with new image; **session sidebar history** of recent paths |
| `solactl screenshot` | Path on stdout only — **no** preview open |
| V1 scope | Screenshot-focused viewer (path from shell/CLI args/`OpenImage`); not a full file browser |

## 3. Architecture

```
Super+Shift+3/4/5
        │
        ▼
sola-shell
  · 3 → CaptureScreen FullOutput
  · 4 → open selection overlay → drag rect → CaptureScreen Region
  · 5 → CaptureScreen Window(focused)
  · set open_preview_on_next = true
        │
        ▼
sola-river screenshot::handle  →  PNG under /tmp/sola/screenshots/
        │
        ▼
Topic::Screenshot { Ok(path) | Err }
        │
        ├─ shell: always toast
        └─ shell if open_preview_on_next:
              if sola-preview window known → Focus + OpenImage(path)
              else → LaunchApp sola-preview with path arg
        │
        ▼
sola-preview
  · argv path and/or OpenImage → load PNG, push history, select it
```

## 4. Bus contract

### 4.1 `CaptureTarget` extension

```rust
pub enum CaptureTarget {
    FullOutput,  // default
    Window { app_id: String, title: Option<String> },
    /// Absolute compositor coords on the first wl_output (V1).
    Region { x: i32, y: i32, width: i32, height: i32 },
}
```

River: `capture_output_region` with the rect; reject non-positive width/height.

### 4.2 `OpenImage` (ephemeral)

Mirror `OpenUrl`:

```rust
pub struct OpenImageRequest {
    pub path: PathBuf,
    pub activate: bool,
    /// Screenshots set `sola-preview`. Missing / `sola-paint` is MIME dest.
    pub app_id: Option<String>,
}

// Topic::OpenImage(OpenImageRequest)
```

- Emitted by shell when preview is already running.
- Consumed only by `sola-preview`.
- `activate: true` → preview should take focus (shell also emits `Focus` when it knows the window id).

### 4.3 Unchanged

- `CaptureScreen` / `Screenshot` payloads otherwise unchanged.
- Default path still `/tmp/sola/screenshots/<ms>.png`.

## 5. Shell

### 5.1 Chords

| Chord | Action |
|-------|--------|
| Super+Shift+3 | Full output |
| Super+Shift+4 | Open selection overlay (no capture yet) |
| Super+Shift+5 | Focused window (existing Super+Shift+4 logic) |

Register `KEY_5.meta_shift()` in `shell_key_chords`.

### 5.2 Selection overlay

- New `WindowKind::Selection` / title `"selection"` — fifth daemon window, boot-opened, composition-gated (like launcher/switcher).
- Full **output** frame `(0, 0, w, h)` so pointer coords match compositor coords (not work-area-below-menubar).
- View: dim translucent scrim + marquee while dragging; Escape cancels.
- Pointer: iced listen while active (press → start, move → current, release → finish).
- Min size: 2×2 px; smaller → cancel without capture.
- On successful release:
  1. `selection.active = false`
  2. `emit_composition()` (drop overlay from stack)
  3. `CaptureScreen { path: None, target: Region {…} }`
  4. `open_preview_on_next = true`
- Capture runs **after** the overlay is hidden so the marquee/scrim is not in the PNG (composition emitted before CaptureScreen on the bus).

### 5.3 Handoff flag

```rust
/// When true, the next successful Topic::Screenshot from sola-river
/// should open/raise sola-preview. Set only by shell hotkey / selection paths.
open_preview_on_next: bool,
```

- Cleared after handling a Screenshot (success or failure).
- `solactl` never sets it → no preview from CLI.

### 5.4 Open / raise

```text
on_screenshot Ok(path):
  toast "Screenshot saved: …"
  if open_preview_on_next:
    if let Some(wid) = first window with app_id == "sola-preview":
      raise (no keyboard steal); OpenImage { path, activate: false, app_id: preview }
    else:
      LaunchApp { app_id: "sola-preview",
                  command: "/opt/sola/bin/sola-preview <path>" }
on_screenshot Err:
  toast failure; clear flag
```

Builtin catalog entry:

```rust
Application {
  app_id: "sola-preview",
  label: "Preview",
  command: "/opt/sola/bin/sola-preview",
  icon: "lucide/image",
}
```

## 6. sola-river

In `screenshot::handle`, add:

```rust
CaptureTarget::Region { x, y, width, height } => {
  if width <= 0 || height <= 0 { err }
  manager.capture_output_region(0, &output, x, y, width, height, …)
}
```

No second capture path; same encode pipeline.

## 7. sola-preview

New crate `crates/sola-preview`, binary `/opt/sola/bin/sola-preview`.

### 7.1 Behavior

- Kit app: `startup`, `BusSetup`, theme, float titlebar, quit menu.
- Subscribe: `OpenImage`, `Theme`, `MenuAction`, `CloseApp` (not full audit bus).
- Boot: optional path from argv (`sola-preview [path…]`); each path enters history.
- `OpenImage`: push path to history (dedupe by path, MRU front), select it, load image.
- UI:
  - Left sidebar: session history (filename + short path); click to select.
  - Main: `image::Handle::from_path` fitted in available space (Contain).
- Zoom V1: fit only (optional ± later).
- History is **in-memory for the process lifetime** only.
- Not MANAGED by `sola`; launched on demand via session/`LaunchApp`.

### 7.2 Layout density

Kit sidebar + card chrome; design language greys/spacing. No new design system tokens.

## 8. solactl

Optional later: `solactl screenshot --region x,y,w,h`. **Not required for V1** (shell owns selection). Docs may note Region exists on the bus.

## 9. File map

| File | Change |
|------|--------|
| `crates/sola-bus/src/topics.rs` | `CaptureTarget::Region`, `OpenImageRequest`, `Topic::OpenImage` |
| `crates/sola-river/src/client/screenshot.rs` | Handle `Region` |
| `crates/sola-shell/src/capture/` (or `selection/`) | State + view for marquee |
| `crates/sola-shell/src/app.rs` + `app/bus.rs` | Chords, overlay, handoff |
| `crates/sola-shell/src/builtins.rs` | Preview entry |
| `crates/sola-preview/` | New crate |
| `docs/manual/` visual notes | Optional follow-up |

## 10. Non-goals (V1)

- Multi-output picker
- Clipboard image copy
- Annotate / crop / export formats
- Persistent history across restarts
- Always-on managed preview process
- Opening non-image files

## 11. Verification

1. `cargo make build` (workspace or shell/river/preview/bus).
2. User install + smoke (no agent install):
   - Super+Shift+3 → toast + preview with full image
   - Super+Shift+4 → drag → toast + preview crop
   - Super+Shift+5 → focused window → toast + preview
   - Second capture appends sidebar history
   - Escape during selection cancels
   - `solactl screenshot -o /tmp/t.png` → file only, no preview

## 12. Risks

| Risk | Mitigation |
|------|------------|
| Overlay in PNG | Hide selection + emit Composition before CaptureScreen |
| Cold-start delay | LaunchApp with path arg so first paint can load without OpenImage race |
| Tiny drag | Min 2×2; treat smaller as cancel |
| Focus race on LaunchApp | Rely on session spawn + zoning; OpenImage only when window already known |
