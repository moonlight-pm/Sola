/// Crate-wide type aliases for concrete Smithay generic types.
///
/// Smithay is heavily generic. These aliases pin the generic parameters
/// to our specific backend choices (GBM allocator, GLES renderer, DRM fd)
/// so the rest of the codebase doesn't need to spell them out.
use smithay::backend::allocator::gbm::GbmAllocator;
use smithay::backend::drm::exporter::gbm::GbmFramebufferExporter;
use smithay::backend::drm::output::{DrmOutput, DrmOutputManager};
use smithay::backend::drm::DrmDeviceFd;
use smithay::backend::renderer::element::texture::TextureRenderElement;
use smithay::backend::renderer::gles::GlesRenderer;
use smithay::backend::renderer::multigpu::gbm::GbmGlesBackend;
use smithay::backend::renderer::multigpu::{MultiRenderer, MultiTexture};

type GlesBackend = GbmGlesBackend<GlesRenderer, DrmDeviceFd>;

/// The multi-GPU renderer type returned by `GpuManager::single_renderer()`.
pub type SolaRenderer<'a> = MultiRenderer<'a, 'a, GlesBackend, GlesBackend>;

/// Simple texture element — used for DRM output initialization.
pub type Element = TextureRenderElement<MultiTexture>;

/// DRM output manager — owns the DRM device and manages compositors.
pub type SolaOutputManager =
    DrmOutputManager<GbmAllocator<DrmDeviceFd>, GbmFramebufferExporter<DrmDeviceFd>, (), DrmDeviceFd>;

/// A single DRM output handle (one per connected display).
pub type SolaOutput =
    DrmOutput<GbmAllocator<DrmDeviceFd>, GbmFramebufferExporter<DrmDeviceFd>, (), DrmDeviceFd>;
