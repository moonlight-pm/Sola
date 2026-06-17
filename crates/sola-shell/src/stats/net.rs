//! Network sampling from /proc/net/dev, default route, getifaddrs.

use std::collections::HashMap;

/// rx_bytes, tx_bytes per interface (loopback excluded).
#[derive(Clone, Debug, Default)]
pub struct Counters(pub HashMap<String, (u64, u64)>);
impl Counters {
    pub fn get(&self, iface: &str) -> Option<&(u64, u64)> {
        self.0.get(iface)
    }
}

pub fn parse_dev(s: &str) -> Counters {
    let mut c = Counters::default();
    for line in s.lines() {
        let Some((name, rest)) = line.split_once(':') else { continue };
        let name = name.trim();
        if name == "lo" || name.is_empty() {
            continue;
        }
        let f: Vec<u64> = rest.split_whitespace().filter_map(|v| v.parse().ok()).collect();
        if f.len() >= 9 {
            c.0.insert(name.to_string(), (f[0], f[8]));
        }
    }
    c
}

pub fn read_counters() -> Counters {
    parse_dev(&std::fs::read_to_string("/proc/net/dev").unwrap_or_default())
}

pub fn rate_for(prev: &Counters, cur: &Counters, iface: &str, dt: f32) -> (f32, f32) {
    match (prev.get(iface), cur.get(iface)) {
        (Some(&(pr, pt)), Some(&(cr, ct))) if dt > 0.0 => (
            (cr.saturating_sub(pr) as f32) / dt,
            (ct.saturating_sub(pt) as f32) / dt,
        ),
        _ => (0.0, 0.0),
    }
}

/// Default-route interface name from /proc/net/route (destination 00000000).
pub fn default_iface() -> Option<String> {
    let s = std::fs::read_to_string("/proc/net/route").ok()?;
    for line in s.lines().skip(1) {
        let mut it = line.split_whitespace();
        let iface = it.next()?;
        let dest = it.next()?;
        if dest == "00000000" {
            return Some(iface.to_string());
        }
    }
    None
}

/// Rate on the default interface (used by the bar).
pub fn rate(prev: &Counters, cur: &Counters, dt: f32) -> (f32, f32) {
    match default_iface() {
        Some(iface) => rate_for(prev, cur, &iface, dt),
        None => (0.0, 0.0),
    }
}

/// IPv4 address of `iface` via getifaddrs.
pub fn iface_ip(iface: &str) -> Option<String> {
    use nix::ifaddrs::getifaddrs;
    for ifa in getifaddrs().ok()? {
        if ifa.interface_name == iface {
            if let Some(addr) = ifa.address.and_then(|a| a.as_sockaddr_in().map(|s| s.ip())) {
                return Some(std::net::Ipv4Addr::from(addr).to_string());
            }
        }
    }
    None
}

#[derive(Clone, Debug, Default)]
pub struct NetDetail {
    pub iface: String,
    pub ip: String,
    pub total_down: u64, // cumulative bytes since boot (from the counter)
    pub total_up: u64,
}

pub fn detail(cur: &Counters) -> NetDetail {
    let iface = default_iface().unwrap_or_default();
    let ip = iface_ip(&iface).unwrap_or_else(|| "—".into());
    let (down, up) = cur.get(&iface).copied().unwrap_or((0, 0));
    NetDetail { iface, ip, total_down: down, total_up: up }
}

#[cfg(test)]
mod tests {
    use super::*;
    const DEV: &str = "Inter-|   Receive                                                |  Transmit\n face |bytes    packets errs drop fifo frame compressed multicast|bytes    packets\n    lo: 1000 10 0 0 0 0 0 0 1000 10 0 0 0 0 0 0\n  eth0: 5000 50 0 0 0 0 0 0 2000 20 0 0 0 0 0 0\n";

    #[test]
    fn parses_counters_skipping_lo() {
        let c = parse_dev(DEV);
        assert_eq!(c.get("eth0"), Some(&(5000, 2000)));
        assert_eq!(c.get("lo"), None); // loopback excluded
    }

    #[test]
    fn rate_from_delta() {
        let mut prev = Counters::default();
        prev.0.insert("eth0".into(), (1000, 500));
        let mut cur = Counters::default();
        cur.0.insert("eth0".into(), (3000, 1500));
        // over 2s: down=(3000-1000)/2=1000 B/s, up=(1500-500)/2=500 B/s
        let (d, u) = rate_for(&prev, &cur, "eth0", 2.0);
        assert_eq!((d, u), (1000.0, 500.0));
    }
}
