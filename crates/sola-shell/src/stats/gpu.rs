//! GPU sampling via NVML (filled in Phase 6).

#[derive(Clone, Debug, Default)]
pub struct GpuDetail;

pub fn lite() -> Option<crate::stats::GpuLite> {
    None
}

pub fn detail() -> Option<GpuDetail> {
    None
}
