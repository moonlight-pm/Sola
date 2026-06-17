//! Memory sampling from /proc/meminfo and /proc/<pid>/status (RSS).

use crate::stats::cpu::Proc;

#[derive(Clone, Copy, Debug, Default)]
pub struct MemInfo {
    pub total_kb: u64,
    pub avail_kb: u64,
    pub free_kb: u64,
    pub buffers_kb: u64,
    pub cached_kb: u64,
    pub swap_total_kb: u64,
    pub swap_free_kb: u64,
}

impl MemInfo {
    pub fn pressure_pct(&self) -> f32 {
        if self.total_kb == 0 {
            return 0.0;
        }
        let used = self.total_kb.saturating_sub(self.avail_kb) as f32;
        (used / self.total_kb as f32) * 100.0
    }
    /// (used, cache, free) in kB. used = total-available, cache = cached+buffers.
    pub fn segments_kb(&self) -> (u64, u64, u64) {
        (
            self.total_kb.saturating_sub(self.avail_kb),
            self.cached_kb + self.buffers_kb,
            self.free_kb,
        )
    }
}

pub fn parse_meminfo(s: &str) -> MemInfo {
    let mut m = MemInfo::default();
    for line in s.lines() {
        let mut it = line.split_whitespace();
        let key = it.next().unwrap_or("");
        let val: u64 = it.next().and_then(|v| v.parse().ok()).unwrap_or(0);
        match key {
            "MemTotal:" => m.total_kb = val,
            "MemAvailable:" => m.avail_kb = val,
            "MemFree:" => m.free_kb = val,
            "Buffers:" => m.buffers_kb = val,
            "Cached:" => m.cached_kb = val,
            "SwapTotal:" => m.swap_total_kb = val,
            "SwapFree:" => m.swap_free_kb = val,
            _ => {}
        }
    }
    m
}

pub fn pressure_pct() -> f32 {
    parse_meminfo(&std::fs::read_to_string("/proc/meminfo").unwrap_or_default()).pressure_pct()
}

#[derive(Clone, Debug, Default)]
pub struct MemDetail {
    pub info: MemInfo,
    pub top: Vec<Proc>, // by RSS, value in MB
}

pub fn detail() -> MemDetail {
    let info = parse_meminfo(&std::fs::read_to_string("/proc/meminfo").unwrap_or_default());
    let mut top = Vec::new();
    if let Ok(dir) = std::fs::read_dir("/proc") {
        for ent in dir.flatten() {
            let Ok(pid) = ent.file_name().to_string_lossy().parse::<i32>() else { continue };
            let Ok(status) = std::fs::read_to_string(format!("/proc/{pid}/status")) else { continue };
            let rss_kb = status
                .lines()
                .find(|l| l.starts_with("VmRSS:"))
                .and_then(|l| l.split_whitespace().nth(1))
                .and_then(|v| v.parse::<u64>().ok());
            if let Some(kb) = rss_kb {
                if kb > 50_000 {
                    // >~50MB
                    let name = std::fs::read_to_string(format!("/proc/{pid}/comm"))
                        .unwrap_or_default()
                        .trim()
                        .to_string();
                    top.push(Proc { name, value: kb as f32 / 1024.0 });
                }
            }
        }
    }
    crate::stats::cpu::cap_top(&mut top, 4);
    MemDetail { info, top }
}

#[cfg(test)]
mod tests {
    use super::*;
    const MEMINFO: &str = "MemTotal:      131000000 kB\nMemFree:        2000000 kB\nMemAvailable:   40000000 kB\nBuffers:         500000 kB\nCached:        20000000 kB\nSwapTotal:       8000000 kB\nSwapFree:        8000000 kB\n";

    #[test]
    fn parses_fields_and_pressure() {
        let m = parse_meminfo(MEMINFO);
        assert_eq!(m.total_kb, 131000000);
        // pressure = (total - available)/total = (131-40)/131 ≈ 69.5%
        assert!((m.pressure_pct() - 69.46).abs() < 0.1);
    }

    #[test]
    fn segments_sum_reasonably() {
        let m = parse_meminfo(MEMINFO);
        let (used, cache, free) = m.segments_kb();
        assert_eq!(free, 2000000);
        assert_eq!(cache, 20500000); // Cached + Buffers
        assert_eq!(used, 131000000 - 40000000); // total - available
    }
}
