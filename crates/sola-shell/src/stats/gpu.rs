//! GPU sampling via NVML (nvml-wrapper). All reads are best-effort; any failure
//! (no NVIDIA GPU, NVML not loadable) yields None so the indicator hides.

use std::sync::OnceLock;

use nvml_wrapper::Nvml;

use crate::stats::cpu::Proc;
use crate::stats::GpuLite;

fn nvml() -> Option<&'static Nvml> {
    static NVML: OnceLock<Option<Nvml>> = OnceLock::new();
    NVML.get_or_init(|| Nvml::init().ok()).as_ref()
}

/// Tier-1 summary for the bar. None when no GPU/NVML.
pub fn lite() -> Option<GpuLite> {
    let dev = nvml()?.device_by_index(0).ok()?;
    let util = dev.utilization_rates().ok()?.gpu as f32;
    let temp = dev
        .temperature(nvml_wrapper::enum_wrappers::device::TemperatureSensor::Gpu)
        .ok()
        .unwrap_or(0) as f32;
    Some(GpuLite { util, temp_c: temp })
}

#[derive(Clone, Debug, Default)]
pub struct GpuDetail {
    pub name: String,
    pub util: f32,
    pub mem_used_mb: f32,
    pub mem_total_mb: f32,
    pub temp_c: f32,
    pub power_w: f32,
    pub fan_pct: f32,
    pub clock_mhz: u32,
    pub top: Vec<Proc>, // by VRAM (MB)
}

pub fn detail() -> Option<GpuDetail> {
    use nvml_wrapper::enum_wrappers::device::{Clock, TemperatureSensor};
    let n = nvml()?;
    let dev = n.device_by_index(0).ok()?;
    let mem = dev.memory_info().ok()?;
    let mut top = Vec::new();
    if let Ok(procs) = dev.running_compute_processes() {
        for p in procs {
            let used = match p.used_gpu_memory {
                nvml_wrapper::enums::device::UsedGpuMemory::Used(b) => b,
                _ => 0,
            };
            let name = std::fs::read_to_string(format!("/proc/{}/comm", p.pid))
                .unwrap_or_default()
                .trim()
                .to_string();
            top.push(Proc { name, value: used as f32 / 1024.0 / 1024.0 });
        }
    }
    crate::stats::cpu::cap_top(&mut top, 4);
    Some(GpuDetail {
        name: dev.name().unwrap_or_default(),
        util: dev.utilization_rates().ok().map(|u| u.gpu as f32).unwrap_or(0.0),
        mem_used_mb: mem.used as f32 / 1024.0 / 1024.0,
        mem_total_mb: mem.total as f32 / 1024.0 / 1024.0,
        temp_c: dev.temperature(TemperatureSensor::Gpu).unwrap_or(0) as f32,
        power_w: dev.power_usage().unwrap_or(0) as f32 / 1000.0,
        fan_pct: dev.fan_speed(0).unwrap_or(0) as f32,
        clock_mhz: dev.clock_info(Clock::Graphics).unwrap_or(0),
        top,
    })
}
