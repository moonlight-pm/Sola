# sola-preview + selection capture — Design

**Date:** 2026-08-04  
**Status:** approved (approach A); **screenshot dest is the clipboard** (paint is MIME / `solactl open` dest; Preview is argv / launcher)  
**Implementation:** Super+Shift+4 freeze-then-crop (RGBA still, no dim, GPU-ready before show) **installed** `river`+`shell` debug 2026-08-31 and smoked. Super+Shift+3/4/5 Fast PNG → clipboard + toast (no file, no Preview) is [image clipboard](2026-09-01-image-clipboard-design.md) — **installed** `kit`+`shell`+`river` debug 2026-09-01.  
**Depends on:** `docs/specs/2026-07-20-screenshot-capture-plan.md` (screencopy live); [paint](2026-08-14-sola-paint-design.md)

## 1. Goal

1. **sola-preview** — kit app that can show image files (argv / launcher).
2. Shell hotkeys copy a Fast PNG to the clipboard and toast **Screenshot copied**.
3. **Selection capture** via Super+Shift+4 (macOS order), with chords remapped.

## 2. Product decisions

| Decision | Choice |
|----------|--------|
| Architecture | Shell orchestrates; river captures; preview is a normal kit app |
| Chords | Super+Shift+3 full · **4 selection** · **5 focused window** |
| Selection UI | Shell full-screen marquee overlay |
| After capture (shell hotkeys) | Fast PNG on the compositor clipboard + toast **Screenshot copied**. No file, no Preview |
| Already open | Replace main view with new image; **session sidebar history** of recent paths |
| `solactl screenshot` | Path on stdout only — **no** preview open |
| V1 scope | Screenshot-focused viewer (path from shell/CLI args/`OpenImage`); not a full file browser |

## 3. Architecture

```
Super+Shift+3/4/5
        │
        ▼
sola-shell
  · 3 → compositor.screenshot format=rgba (full output)
  · 4 → format=rgba freeze → overlay still → crop in-process
  · 5 → compositor.screenshot format=rgba --app (toplevel scene)
        │
        ▼
sola-river  →  packed RGBA (tmpfs dump, deleted after read)
        │
        ▼
shell: Fast PNG → data-control clipboard; toast "Screenshot copied"
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

`compositor.screenshot` also takes `format`: `png` (default, writes a PNG) or `rgba` (packed RGBA8 on tmpfs, no PNG encode). RGBA reply is `{ path, width, height, format: "rgba8" }`. Super+Shift+4 uses this so the picker can freeze a 5K output without a multi-second encode.

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
| Super+Shift+4 | Freeze live output (RGBA), then selection overlay |
| Super+Shift+5 | Focused window (existing Super+Shift+4 logic) |

Register `KEY_5.meta_shift()` in `shell_key_chords`.

### 5.2 Selection overlay

- New `WindowKind::Selection` / title `"selection"` — fifth daemon window, boot-opened, composition-gated (like launcher/switcher).
- Full **output** frame `(0, 0, w, h)` so pointer coords match compositor coords (not work-area-below-menubar).
- **Freeze first.** Super+Shift+4 does **not** map the overlay immediately (that steals focus and drops menus / text selections). River captures the live output as packed RGBA8 (`compositor.screenshot` `format=rgba`, tmpfs dump, **no PNG encode**). The overlay is that still at full brightness (no dim — it is the desktop) and joins composition only after the freeze texture is on the GPU, so the first visible frame matches the live output. Cyan marquee while dragging.
- View: freeze image + dim scrim + cyan marquee; Escape cancels (also while freeze is in flight).
- Pointer: iced listen while active (press → start, move → current, release → finish).
- Min size: 2×2 px; smaller → cancel without capture.
- On successful release:
  1. clone the freeze `Handle` (refcount)
  2. `selection.active = false` / `emit_composition()` (drop overlay)
  3. crop the freeze in-process and Fast-encode PNG onto the clipboard (no second screencopy, no file)
- The marquee/scrim never enter the PNG because the crop is from the freeze, not a live capture.

### 5.3 After capture

```text
on_screenshot Ok:
  toast "Screenshot copied"
  restore pre-capture keyboard focus
on_screenshot Err:
  toast failure
```

`solactl compositor screenshot` still writes a PNG path to stdout and does not touch the clipboard. Preview is launcher / argv only.

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
| Overlay in PNG | Crop from the freeze still, not a live recapture |
| Freeze feels slow | Skip PNG encode of the 5K frame; RGBA dump on tmpfs (`/dev/shm`); convert off the Wayland thread |
| Cold-start delay | LaunchApp with path arg so first paint can load without OpenImage race |
| Tiny drag | Min 2×2; treat smaller as cancel |
| Focus race on LaunchApp | Rely on session spawn + zoning; OpenImage only when window already known |
