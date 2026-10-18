//! Compx report-8 protocol spoken by Lamzu receivers (reverse engineered).
//!
//! The receiver's vendor interface carries 16-byte reports under report
//! ID 8: payload byte 0 is the command and byte 15 a checksum that makes
//! the whole report (ID included) sum to `0x55`. Command 4 asks the mouse
//! for its battery; the reply puts the percent in payload byte 5 and the
//! power state in byte 6. Layout cross-checked against lamzu-ctl
//! (github.com/anany1702/lamzu-ctl) and verified on a `3554:f510` receiver.

use std::fs;
use std::io::{self, Read, Write};
use std::os::fd::AsFd;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

/// USB VID:PID of receivers verified to speak report 8 (LAMZU 4K Receiver).
const RECEIVERS: &[(u32, u32)] = &[(0x3554, 0xF510)];

const REPORT_ID: u8 = 0x08;
const CMD_BATTERY: u8 = 0x04;
/// Report length on the wire: report ID + 16 payload bytes.
const REPORT_LEN: usize = 17;
const CHECKSUM: u8 = 0x55;
/// The receiver answers within a few ms; leave room for a sleepy mouse.
const REPLY_WAIT: Duration = Duration::from_millis(250);

/// One battery answer from the mouse.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Battery {
    pub percent: u8,
    pub charging: bool,
}

/// hidraw nodes of known receivers whose interface declares report 8.
/// Rescanned on every poll so a re-plugged receiver is found again.
pub fn find() -> Vec<PathBuf> {
    let Ok(dir) = fs::read_dir("/sys/class/hidraw") else {
        return Vec::new();
    };
    let mut nodes: Vec<PathBuf> = dir
        .flatten()
        .filter(|e| {
            let dev = e.path().join("device");
            let known = fs::read_to_string(dev.join("uevent"))
                .ok()
                .and_then(|u| hid_id(&u))
                .is_some_and(|id| RECEIVERS.contains(&id));
            known
                && fs::read(dev.join("report_descriptor"))
                    .is_ok_and(|d| declares_report(&d, REPORT_ID))
        })
        .map(|e| Path::new("/dev").join(e.file_name()))
        .collect();
    nodes.sort();
    nodes
}

/// Ask the mouse behind `node` for its battery. `Ok(None)` when the
/// receiver gave no valid answer in time.
pub fn read_battery(node: &Path) -> io::Result<Option<Battery>> {
    let mut f = fs::OpenOptions::new().read(true).write(true).open(node)?;
    let mut buf = [0u8; 64];
    // Drop reports queued before the question so a stale answer can't match.
    while readable(&f, Duration::ZERO) {
        f.read(&mut buf)?;
    }
    f.write_all(&packet(CMD_BATTERY))?;
    let deadline = Instant::now() + REPLY_WAIT;
    while let Some(left) = deadline.checked_duration_since(Instant::now()) {
        if !readable(&f, left) {
            break;
        }
        let n = f.read(&mut buf)?;
        if let Some(b) = parse_battery(&buf[..n]) {
            return Ok(Some(b));
        }
    }
    Ok(None)
}

/// Report-8 request for `cmd` with its checksum.
fn packet(cmd: u8) -> [u8; REPORT_LEN] {
    let mut p = [0u8; REPORT_LEN];
    p[0] = REPORT_ID;
    p[1] = cmd;
    p[REPORT_LEN - 1] = CHECKSUM.wrapping_sub(sum(&p[..REPORT_LEN - 1]));
    p
}

fn parse_battery(r: &[u8]) -> Option<Battery> {
    let r = r.get(..REPORT_LEN)?;
    if r[0] != REPORT_ID || r[1] != CMD_BATTERY || sum(r) != CHECKSUM {
        return None;
    }
    let percent = r[6];
    (percent <= 100).then_some(Battery {
        percent,
        // 0 discharging, 1 charging, 2 full.
        charging: r[7] == 1,
    })
}

fn sum(bytes: &[u8]) -> u8 {
    bytes.iter().fold(0u8, |a, b| a.wrapping_add(*b))
}

/// `HID_ID=0003:00003554:0000F510` in a hid uevent → `(0x3554, 0xF510)`.
fn hid_id(uevent: &str) -> Option<(u32, u32)> {
    let id = uevent.lines().find_map(|l| l.strip_prefix("HID_ID="))?;
    let mut parts = id.split(':').skip(1);
    let vid = u32::from_str_radix(parts.next()?, 16).ok()?;
    let pid = u32::from_str_radix(parts.next()?, 16).ok()?;
    Some((vid, pid))
}

/// True when a HID report descriptor has a Report ID item equal to `id`.
fn declares_report(desc: &[u8], id: u8) -> bool {
    let mut i = 0;
    while let Some(&prefix) = desc.get(i) {
        if prefix == 0xFE {
            // Long item: data size in the next byte, then a tag byte.
            i += 3 + desc.get(i + 1).copied().unwrap_or(0) as usize;
            continue;
        }
        let size = match prefix & 0x03 {
            3 => 4,
            n => n as usize,
        };
        // Global item tag 8 (Report ID) with one data byte.
        if prefix == 0x85 && desc.get(i + 1) == Some(&id) {
            return true;
        }
        i += 1 + size;
    }
    false
}

fn readable(f: &fs::File, wait: Duration) -> bool {
    use nix::poll::{PollFd, PollFlags, PollTimeout, poll};
    let mut fds = [PollFd::new(f.as_fd(), PollFlags::POLLIN)];
    let timeout = PollTimeout::try_from(wait).unwrap_or(PollTimeout::MAX);
    match poll(&mut fds, timeout) {
        Ok(n) if n > 0 => fds[0]
            .revents()
            .is_some_and(|r| r.contains(PollFlags::POLLIN)),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Interface 1 of a `3554:f510` receiver (vendor pages, report 8 among them).
    const VENDOR_DESC: &str = "0605ff0900a10185100900150026ff00750895078102c00603ff0900a10185020900150026ff00750895078102c0050c0901a10185051500263c0219002a3c02751095018100c005010980a10185031981298315002501950375018102950175058101c00602ff0902a1018508150026ff00750895100902810009029100c00604ff0902a10185060902150026ff0075089507b102c00606ff0902a1018509150026ff00750895300902810009029100c0";
    /// Interface 0 of the same receiver (boot keyboard, no report IDs).
    const KEYBOARD_DESC: &str = "05010906a1010508190129031500250175019503910295059101050719e029e795088102750895018101050719002aff0026ff0095068100c0";

    fn hex(s: &str) -> Vec<u8> {
        (0..s.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap())
            .collect()
    }

    #[test]
    fn battery_packet_matches_capture() {
        assert_eq!(
            packet(CMD_BATTERY).to_vec(),
            hex("0804000000000000000000000000000049")
        );
        assert_eq!(packet(0x12)[REPORT_LEN - 1], 0x3b);
    }

    #[test]
    fn parses_captured_reply() {
        let reply = hex("0804000000022d000ef500000000000017");
        assert_eq!(
            parse_battery(&reply),
            Some(Battery {
                percent: 45,
                charging: false
            })
        );
    }

    #[test]
    fn rejects_bad_checksum_other_command_and_overflow() {
        let mut reply = hex("0804000000022d000ef500000000000017");
        reply[16] ^= 1;
        assert_eq!(parse_battery(&reply), None);
        // Firmware reply (command 0x12) is not a battery answer.
        assert_eq!(
            parse_battery(&hex("0812000000020124000000000000000014")),
            None
        );
        // Percent above 100, checksum fixed up.
        let mut over = hex("0804000000026500000000000000000000");
        over[16] = CHECKSUM.wrapping_sub(sum(&over[..16]));
        assert_eq!(parse_battery(&over), None);
        assert_eq!(parse_battery(&reply[..8]), None);
    }

    #[test]
    fn charging_state() {
        let mut r = hex("0804000000025001000000000000000000");
        r[16] = CHECKSUM.wrapping_sub(sum(&r[..16]));
        assert_eq!(
            parse_battery(&r),
            Some(Battery {
                percent: 80,
                charging: true
            })
        );
    }

    #[test]
    fn finds_report_8_only_on_vendor_interface() {
        assert!(declares_report(&hex(VENDOR_DESC), REPORT_ID));
        assert!(!declares_report(&hex(KEYBOARD_DESC), REPORT_ID));
    }

    #[test]
    fn hid_id_from_uevent() {
        let u =
            "DRIVER=hid-generic\nHID_ID=0003:00003554:0000F510\nHID_NAME=Compx LAMZU 4K Receiver\n";
        assert_eq!(hid_id(u), Some((0x3554, 0xF510)));
        assert_eq!(hid_id("DRIVER=hid-generic\n"), None);
    }
}
