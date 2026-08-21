//! GPU sampling via NVML (nvml-wrapper). All reads are best-effort; any failure
//! (no NVIDIA GPU, NVML not loadable) yields None so the indicator hides.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::OnceLock;

use nvml_wrapper::enum_wrappers::device::{Clock, TemperatureSensor};
use nvml_wrapper::enums::device::UsedGpuMemory;
use nvml_wrapper::struct_wrappers::device::ProcessInfo;
use nvml_wrapper::Nvml;

use crate::stats::cpu::Proc;
use crate::stats::GpuLite;

fn nvml() -> Option<&'static Nvml> {
    static NVML: OnceLock<Option<Nvml>> = OnceLock::new();
    NVML.get_or_init(|| Nvml::init().ok()).as_ref()
}

/// Last `process_utilization_stats` timestamp (µs) so each tick asks only for
/// samples since the previous query.
static LAST_SM_TS: AtomicU64 = AtomicU64::new(0);

/// Tier-1 summary for the bar. None when no GPU/NVML.
pub fn lite() -> Option<GpuLite> {
    let dev = nvml()?.device_by_index(0).ok()?;
    let util = dev.utilization_rates().ok()?.gpu as f32;
    let temp = dev.temperature(TemperatureSensor::Gpu).ok().unwrap_or(0) as f32;
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
    /// Top processes by SM / compute utilization (%).
    pub top_gpu: Vec<Proc>,
    /// Top processes by VRAM (MB).
    pub top_vram: Vec<Proc>,
}

/// One NVML process-utilization sample (pid + SM %).
#[derive(Clone, Copy, Debug)]
pub struct SmSample {
    pub pid: u32,
    pub timestamp: u64,
    pub sm_util: u32,
}

/// Latest SM util per pid, dropping zeros. Unsorted.
pub fn latest_sm_by_pid(samples: &[SmSample]) -> Vec<(u32, u32)> {
    let mut best: HashMap<u32, (u64, u32)> = HashMap::new();
    for s in samples {
        if s.pid == 0 {
            continue;
        }
        match best.get(&s.pid) {
            Some((ts, sm)) if *ts > s.timestamp || (*ts == s.timestamp && *sm >= s.sm_util) => {}
            _ => {
                best.insert(s.pid, (s.timestamp, s.sm_util));
            }
        }
    }
    best.into_iter()
        .filter(|(_, (_, sm))| *sm > 0)
        .map(|(pid, (_, sm))| (pid, sm))
        .collect()
}

/// Max used-bytes per pid, dropping zeros. Unsorted.
pub fn max_vram_by_pid(entries: &[(u32, u64)]) -> Vec<(u32, u64)> {
    let mut best: HashMap<u32, u64> = HashMap::new();
    for &(pid, bytes) in entries {
        if pid == 0 || bytes == 0 {
            continue;
        }
        best.entry(pid)
            .and_modify(|b| *b = (*b).max(bytes))
            .or_insert(bytes);
    }
    best.into_iter().collect()
}

pub fn detail() -> Option<GpuDetail> {
    let n = nvml()?;
    let dev = n.device_by_index(0).ok()?;
    let mem = dev.memory_info().ok()?;
    Some(GpuDetail {
        name: dev.name().unwrap_or_default(),
        util: dev
            .utilization_rates()
            .ok()
            .map(|u| u.gpu as f32)
            .unwrap_or(0.0),
        mem_used_mb: mem.used as f32 / 1024.0 / 1024.0,
        mem_total_mb: mem.total as f32 / 1024.0 / 1024.0,
        temp_c: dev.temperature(TemperatureSensor::Gpu).unwrap_or(0) as f32,
        power_w: dev.power_usage().unwrap_or(0) as f32 / 1000.0,
        fan_pct: dev.fan_speed(0).unwrap_or(0) as f32,
        clock_mhz: dev.clock_info(Clock::Graphics).unwrap_or(0),
        top_gpu: top_by_sm(&dev),
        top_vram: top_by_vram(&dev),
    })
}

fn top_by_sm(dev: &nvml_wrapper::Device<'_>) -> Vec<Proc> {
    let last = LAST_SM_TS.load(Ordering::Relaxed);
    let last_seen = if last == 0 { None } else { Some(last) };
    let Ok(samples) = dev.process_utilization_stats(last_seen) else {
        return Vec::new();
    };
    if let Some(max_ts) = samples.iter().map(|s| s.timestamp).max() {
        LAST_SM_TS.store(max_ts, Ordering::Relaxed);
    }
    let sm: Vec<SmSample> = samples
        .iter()
        .map(|s| SmSample {
            pid: s.pid,
            timestamp: s.timestamp,
            sm_util: s.sm_util,
        })
        .collect();
    let mut rows: Vec<Proc> = latest_sm_by_pid(&sm)
        .into_iter()
        .map(|(pid, util)| Proc {
            name: proc_name(pid),
            value: util as f32,
        })
        .collect();
    crate::stats::cpu::cap_top(&mut rows, 4);
    rows
}

fn top_by_vram(dev: &nvml_wrapper::Device<'_>) -> Vec<Proc> {
    let mut entries = Vec::new();
    if let Ok(procs) = dev.running_compute_processes() {
        push_vram_entries(&mut entries, &procs);
    }
    if let Ok(procs) = dev.running_graphics_processes() {
        push_vram_entries(&mut entries, &procs);
    }
    let mut rows: Vec<Proc> = max_vram_by_pid(&entries)
        .into_iter()
        .map(|(pid, bytes)| Proc {
            name: proc_name(pid),
            value: bytes as f32 / 1024.0 / 1024.0,
        })
        .collect();
    crate::stats::cpu::cap_top(&mut rows, 4);
    rows
}

fn push_vram_entries(out: &mut Vec<(u32, u64)>, procs: &[ProcessInfo]) {
    for p in procs {
        let used = match p.used_gpu_memory {
            UsedGpuMemory::Used(b) => b,
            _ => 0,
        };
        out.push((p.pid, used));
    }
}

fn proc_name(pid: u32) -> String {
    let name = std::fs::read_to_string(format!("/proc/{pid}/comm"))
        .unwrap_or_default()
        .trim()
        .to_string();
    if name.is_empty() {
        pid.to_string()
    } else {
        name
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::stats::cpu::cap_top;

    #[test]
    fn latest_sm_keeps_newest_sample_per_pid() {
        let samples = [
            SmSample {
                pid: 1,
                timestamp: 10,
                sm_util: 5,
            },
            SmSample {
                pid: 1,
                timestamp: 20,
                sm_util: 40,
            },
            SmSample {
                pid: 2,
                timestamp: 15,
                sm_util: 90,
            },
            SmSample {
                pid: 3,
                timestamp: 20,
                sm_util: 0,
            },
        ];
        let mut rows: Vec<Proc> = latest_sm_by_pid(&samples)
            .into_iter()
            .map(|(pid, sm)| Proc {
                name: pid.to_string(),
                value: sm as f32,
            })
            .collect();
        cap_top(&mut rows, 4);
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].name, "2");
        assert_eq!(rows[0].value, 90.0);
        assert_eq!(rows[1].name, "1");
        assert_eq!(rows[1].value, 40.0);
    }

    #[test]
    fn latest_sm_same_timestamp_keeps_higher_util() {
        let samples = [
            SmSample {
                pid: 7,
                timestamp: 5,
                sm_util: 10,
            },
            SmSample {
                pid: 7,
                timestamp: 5,
                sm_util: 25,
            },
        ];
        let rows = latest_sm_by_pid(&samples);
        assert_eq!(rows, vec![(7, 25)]);
    }

    #[test]
    fn max_vram_merges_compute_and_graphics() {
        let entries = [(1, 100), (1, 50), (2, 200), (3, 0), (0, 999)];
        let mut rows: Vec<Proc> = max_vram_by_pid(&entries)
            .into_iter()
            .map(|(pid, bytes)| Proc {
                name: pid.to_string(),
                value: bytes as f32,
            })
            .collect();
        cap_top(&mut rows, 4);
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].name, "2");
        assert_eq!(rows[0].value, 200.0);
        assert_eq!(rows[1].name, "1");
        assert_eq!(rows[1].value, 100.0);
    }
}
