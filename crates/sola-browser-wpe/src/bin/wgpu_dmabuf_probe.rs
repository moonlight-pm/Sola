//! Phase 0a probe — placeholder. To be implemented next:
//! 1. Spin up a Vulkan device via `ash`.
//! 2. Allocate a `VkImage` with `VK_KHR_external_memory_fd`, export as DMA-BUF.
//! 3. Draw a known checkerboard via Vulkan into that image.
//! 4. Import the DMA-BUF FD into wgpu via `wgpu_hal::vulkan::Device::texture_from_raw`.
//! 5. Sample the texture inside an iced `widget::shader::Program` and confirm
//!    the checkerboard renders correctly.
//!
//! If this probe works end-to-end, every later phase's wgpu side is
//! known-good and we can focus on the WPE plumbing in isolation.

fn main() {
    eprintln!("wgpu-dmabuf-probe: not yet implemented");
    std::process::exit(2);
}
