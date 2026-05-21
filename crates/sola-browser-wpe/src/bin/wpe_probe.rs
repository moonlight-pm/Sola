//! Phase 0b probe — placeholder. To be implemented after 0a:
//! 1. Hand-rolled FFI for `libwpe` + `wpebackend-fdo` (no Rust crate
//!    covers the surface we need cleanly).
//! 2. Create a WPE view backend with `wpe_view_backend_exportable_dmabuf_create`.
//! 3. Load a hardcoded URL via WebKit's `webkit_web_view_load_uri`.
//! 4. On every new buffer, mmap the DMA-BUF and write the first ~5 frames
//!    to `/tmp/wpe-probe-frame-NNN.png` to verify WPE actually renders.
//!
//! Runs entirely independent of wgpu. If WPE produces sane frames here,
//! the engine integration is known-good and we can focus on transport
//! and import in 0c.

fn main() {
    eprintln!("wpe-probe: not yet implemented");
    std::process::exit(2);
}
