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

pub fn parse_loadavg(s: &str) -> [f32; 3] {
    let mut it = s.split_whitespace().filter_map(|v| v.parse::<f32>().ok());
    [it.next().unwrap_or(0.0), it.next().unwrap_or(0.0), it.next().unwrap_or(0.0)]
}

/// Sort processes by value descending and keep the top `n`.
pub fn cap_top(rows: &mut Vec<Proc>, n: usize) {
    rows.sort_by(|a, b| b.value.partial_cmp(&a.value).unwrap_or(std::cmp::Ordering::Equal));
    rows.truncate(n);
}

/// Build tier-2 CPU detail. `per_core_pct` is computed by the sampler from
/// successive /proc/stat snapshots (see mod.rs); load/uptime/top read here.
pub fn detail(per_core_pct: Vec<f32>, top: Vec<Proc>) -> CpuDetail {
    let load = parse_loadavg(&std::fs::read_to_string("/proc/loadavg").unwrap_or_default());
    let uptime_secs = std::fs::read_to_string("/proc/uptime")
        .ok()
        .and_then(|s| s.split_whitespace().next().and_then(|v| v.parse::<f32>().ok()))
        .map(|f| f as u64)
        .unwrap_or(0);
    CpuDetail { per_core: per_core_pct, load, uptime_secs, top }
}

/// Top processes by CPU between two scans of /proc/<pid>/stat (utime+stime).
/// `total_delta` is the aggregate cpu total-jiffies delta over the interval.
pub fn top_processes(
    prev: &std::collections::HashMap<i32, u64>,
    total_delta: u64,
    ncpu: usize,
) -> (std::collections::HashMap<i32, u64>, Vec<Proc>) {
    use std::collections::HashMap;
    let mut cur: HashMap<i32, u64> = HashMap::new();
    let mut rows: Vec<Proc> = Vec::new();
    let Ok(dir) = std::fs::read_dir("/proc") else { return (cur, rows) };
    for ent in dir.flatten() {
        let Ok(pid) = ent.file_name().to_string_lossy().parse::<i32>() else { continue };
        let Ok(stat) = std::fs::read_to_string(format!("/proc/{pid}/stat")) else { continue };
        // comm is in parens (field 2); split after the closing paren to avoid spaces in names.
        let Some(rparen) = stat.rfind(')') else { continue };
        // A truncated read (PID vanished mid-scan) can leave nothing after the
        // closing paren; skip rather than panic on an out-of-bounds slice.
        let Some(rest_str) = stat.get(rparen + 2..) else { continue };
        let rest: Vec<&str> = rest_str.split_whitespace().collect();
        // After comm, field indices: state=0, ... utime=11, stime=12 (0-based in `rest`).
        let (Some(utime), Some(stime)) = (
            rest.get(11).and_then(|v| v.parse::<u64>().ok()),
            rest.get(12).and_then(|v| v.parse::<u64>().ok()),
        ) else { continue };
        let jiffies = utime + stime;
        cur.insert(pid, jiffies);
        if total_delta > 0 {
            if let Some(p) = prev.get(&pid) {
                let d = jiffies.saturating_sub(*p) as f32;
                // % of one core summed across the machine: scale by ncpu.
                let pct = (d / total_delta as f32) * 100.0 * ncpu as f32;
                if pct >= 0.5 {
                    let name = std::fs::read_to_string(format!("/proc/{pid}/comm"))
                        .unwrap_or_default().trim().to_string();
                    rows.push(Proc { name, value: pct });
                }
            }
        }
    }
    cap_top(&mut rows, 4);
    (cur, rows)
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

    #[test]
    fn loadavg_parsed() {
        assert_eq!(parse_loadavg("4.20 3.80 3.10 2/1234 5678"), [4.20, 3.80, 3.10]);
    }

    #[test]
    fn top_sorted_desc_and_capped() {
        let mut rows = vec![
            Proc { name: "a".into(), value: 5.0 },
            Proc { name: "b".into(), value: 22.0 },
            Proc { name: "c".into(), value: 7.0 },
        ];
        cap_top(&mut rows, 2);
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].name, "b");
        assert_eq!(rows[1].name, "c");
    }
}
