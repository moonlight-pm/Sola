//! CPU sampling from /proc/stat, /proc/loadavg, /proc/uptime, /proc/<pid>/stat.

/// Cumulative jiffies for one cpu line: idle (idle+iowait) and grand total.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct CpuTimes {
    pub idle: u64,
    pub total: u64,
}

/// Parse one `cpu...` line of /proc/stat into idle/total jiffies.
/// Fields after the label: user nice system idle iowait irq softirq steal ...
pub fn parse_cpu_line(line: &str) -> Option<CpuTimes> {
    let mut it = line.split_whitespace();
    let label = it.next()?;
    if !label.starts_with("cpu") {
        return None;
    }
    let vals: Vec<u64> = it.filter_map(|v| v.parse().ok()).collect();
    if vals.len() < 4 {
        return None;
    }
    let idle = vals[3] + vals.get(4).copied().unwrap_or(0); // idle + iowait
    let total: u64 = vals.iter().sum();
    Some(CpuTimes { idle, total })
}

/// Busy percentage between two cumulative samples.
pub fn cpu_pct(prev: &CpuTimes, cur: &CpuTimes) -> f32 {
    let total_d = cur.total.saturating_sub(prev.total);
    let idle_d = cur.idle.saturating_sub(prev.idle);
    if total_d == 0 {
        return 0.0;
    }
    let busy = total_d.saturating_sub(idle_d) as f32;
    (busy / total_d as f32) * 100.0
}

/// Per-core cumulative times (the `cpu0`, `cpu1`, ... lines) in order.
pub fn parse_per_core(stat: &str) -> Vec<CpuTimes> {
    stat.lines()
        .filter(|l| {
            l.starts_with("cpu") && l.as_bytes().get(3).is_some_and(|b| b.is_ascii_digit())
        })
        .filter_map(parse_cpu_line)
        .collect()
}

/// The aggregate (`cpu `) line, if present.
pub fn parse_aggregate(stat: &str) -> Option<CpuTimes> {
    stat.lines().find(|l| l.starts_with("cpu ")).and_then(parse_cpu_line)
}

/// A process row for a "top processes" list.
#[derive(Clone, Debug)]
pub struct Proc {
    pub name: String,
    pub value: f32, // percent (cpu) or MB (mem) depending on the list
}

/// Tier-2 CPU detail.
#[derive(Clone, Debug, Default)]
pub struct CpuDetail {
    pub per_core: Vec<f32>,
    pub load: [f32; 3],
    pub uptime_secs: u64,
    pub top: Vec<Proc>,
}

/// Phase-1 stub — replaced by the real parser in Phase 3 (Task 9).
pub fn detail(_stat: &str, _prev: &[CpuTimes]) -> CpuDetail {
    CpuDetail::default()
}

#[cfg(test)]
mod tests {
    use super::*;

    const STAT: &str = "cpu  100 0 50 1000 20 0 10 0 0 0\ncpu0 50 0 25 500 10 0 5 0 0 0\ncpu1 50 0 25 500 10 0 5 0 0 0\n";

    #[test]
    fn parses_aggregate_idle_and_total() {
        let t = parse_cpu_line("cpu  100 0 50 1000 20 0 10 0 0 0").unwrap();
        // idle = idle(1000) + iowait(20) = 1020
        assert_eq!(t.idle, 1020);
        // total = sum of all = 100+0+50+1000+20+0+10 = 1180
        assert_eq!(t.total, 1180);
    }

    #[test]
    fn pct_from_delta() {
        let prev = CpuTimes { idle: 1000, total: 1100 };
        let cur = CpuTimes { idle: 1050, total: 1200 };
        // busy delta = total_d(100) - idle_d(50) = 50; pct = 50/100 = 50%
        assert!((cpu_pct(&prev, &cur) - 50.0).abs() < 0.01);
    }

    #[test]
    fn pct_zero_when_no_delta() {
        let t = CpuTimes { idle: 10, total: 20 };
        assert_eq!(cpu_pct(&t, &t), 0.0);
    }

    #[test]
    fn per_core_lines_parsed_in_order() {
        let cores = parse_per_core(STAT);
        assert_eq!(cores.len(), 2);
        assert_eq!(cores[0].total, 590); // 50+25+500+10+5
    }
}
