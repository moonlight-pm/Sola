//! Screenshot handler — currently unimplemented.
//!
//! ## Status
//!
//! `solactl screenshot` will return `"screenshot not yet implemented"`.
//! The bus protocol (`Topic::CaptureScreen` / `Topic::Screenshot`) and CLI
//! flags (`--app`, `--window`, `-o`) are wired and ready; only the capture
//! body is missing.
//!
//! ## Why not delegate to `grim`?
//!
//! Spawning the `grim` binary works (and was the original approach) but
//! requires `pkgs.grim` in the system config. We'd rather be self-contained.
//!
//! ## Why not use the `grim-rs` crate?
//!
//! Tried it. It hardcodes `bytes_per_pixel = 4` throughout its pipeline
//! (stride, row copy, save_png), so any compositor that advertises a 3-bpp
//! format like `wl_shm::Format::Bgr888` (which our River instance does)
//! triggers either a wl_shm `INVALID_STRIDE` rejection or an out-of-bounds
//! panic in the row-copy loop. Patching grim-rs to track bpp per-format and
//! generalize the pixel pipeline is a meaningful change worth submitting
//! upstream, but not on the critical path for solactl.
//!
//! ## Plan: hand-roll wlr-screencopy
//!
//! ~150 LOC, fully under our control:
//!
//! 1. Vendor `wlr-screencopy-unstable-v1.xml` in `crates/sola-river/protocols/`,
//!    add a module to `protocol.rs` (mirrors the existing `wlr-virtual-pointer`
//!    work).
//! 2. Bind `zwlr_screencopy_manager_v1` in the registry handler in
//!    `client/mod.rs`. Bind `wl_shm` (already advertised, currently unbound).
//! 3. Per request: `manager.capture_output(0, output)` for full output, or
//!    `capture_output_region(0, output, x, y, w, h)` for `CaptureTarget::Window`.
//! 4. On the `Buffer { format, width, height, stride }` event, allocate a
//!    `wl_shm` buffer:
//!    - `memfd_create` (we already use `rustix::fs::memfd_create` in
//!      `virtual_keyboard.rs`).
//!    - `ftruncate` to `stride * height` (use the EVENT's stride, not
//!      `width * bpp` — see the grim-rs note above).
//!    - `wl_shm.create_pool(fd, size)` → `wl_shm_pool.create_buffer(...)`
//!      using the event's exact format / stride.
//! 5. Call `frame.copy(buffer)`. On `Ready`, mmap the fd, walk pixels into
//!    RGBA via a per-format dispatch (Bgr888: pack 3 → 4 with α=255;
//!    Xrgb8888: swap B↔R, α=255; Argb8888: swap B↔R; Xbgr8888/Abgr8888:
//!    pass-through with optional α=255). On `Failed`, emit
//!    `Topic::Screenshot { result: Err(...) }`.
//! 6. PNG-encode the RGBA via the `png` crate (~5 lines, no transitive deps).
//!
//! Async coordination: the screencopy events fire on river's existing
//! event loop. Track in-flight captures keyed by frame proxy id, with
//! the request's `path` and `target` stashed alongside. When `Ready`
//! fires, finalize and emit on the bus.
//!
//! Existing infrastructure that helps:
//! - `virtual_keyboard.rs::make_keymap_fd` already shows the
//!   memfd_create + write + size pattern.
//! - `WindowRegistry::find_by_app_title` and `Entry::frame` already
//!   provide the per-window region geometry from inbound `Frame` topics.
//! - The `CaptureTarget` enum on the bus is already in place.
//!
//! ## Until then
//!
//! Use a non-Sola screenshot tool (e.g. `grim`) directly; solactl
//! will return a clear error message.

use sola_bus::topics::{CaptureScreenPayload, ScreenshotPayload, Topic};

use crate::client::AppData;

pub fn handle(state: &mut AppData, _req: CaptureScreenPayload) {
    state.bus.emit(Topic::Screenshot(ScreenshotPayload {
        result: Err(
            "screenshot not yet implemented; see crates/sola-river/src/client/screenshot.rs"
                .to_string(),
        ),
    }));
}
