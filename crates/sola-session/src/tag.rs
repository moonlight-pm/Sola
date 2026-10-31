//! Apps that aren't Sola's carry a tag: `SOLA_APP=<app id>` in the
//! environment they are launched with. Every process they start inherits it
//! — Steam's pressure-vessel containers, its webhelpers, the logger that
//! ends up under PID 1 — so the tag finds all of an app when it quits, and
//! the app a window belongs to (its process, or the nearest one above it,
//! carries the tag).

use std::path::Path;

/// The environment variable that carries the tag.
pub const VAR: &str = "SOLA_APP";

/// True when `command` runs something other than a Sola program. Kit apps
/// and web apps (`sola-browser --app …`) are Sola's.
pub fn is_foreign(command: &str) -> bool {
    let program = command.split_whitespace().next().unwrap_or("");
    let name = Path::new(program)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or(program);
    !name.is_empty() && !name.starts_with("sola")
}

/// Every process carrying `app_id`'s tag, this one aside. Zombies have no
/// environment and do not count.
pub fn pids(app_id: &str) -> Vec<u32> {
    let me = std::process::id();
    let Ok(dir) = std::fs::read_dir("/proc") else {
        return Vec::new();
    };
    dir.filter_map(|e| e.ok()?.file_name().to_str()?.parse::<u32>().ok())
        .filter(|&pid| pid != me && tag_of(pid).as_deref() == Some(app_id))
        .collect()
}

/// The app `pid` belongs to: the tag on it, or on the nearest process
/// above it.
pub fn owner(pid: u32) -> Option<String> {
    let mut pid = pid;
    for _ in 0..64 {
        if pid <= 1 {
            return None;
        }
        if let Some(tag) = tag_of(pid) {
            return Some(tag);
        }
        pid = parent_of(pid)?;
    }
    None
}

/// Send `signal` to each of `pids`.
pub fn signal(pids: &[u32], signal: i32) {
    for &pid in pids {
        // SAFETY: kill(2) is safe to call with any pid and signal.
        unsafe { libc::kill(pid as i32, signal) };
    }
}

fn tag_of(pid: u32) -> Option<String> {
    tag_in(&std::fs::read(format!("/proc/{pid}/environ")).ok()?)
}

pub(crate) fn parent_of(pid: u32) -> Option<u32> {
    ppid_in(&std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?)
}

/// The tag in a NUL-separated environment block.
fn tag_in(environ: &[u8]) -> Option<String> {
    let prefix = format!("{VAR}=");
    environ
        .split(|&b| b == 0)
        .find_map(|kv| kv.strip_prefix(prefix.as_bytes()))
        .map(|v| String::from_utf8_lossy(v).into_owned())
}

/// The parent pid in a `/proc/<pid>/stat` line. The name in parentheses
/// may hold spaces and parentheses itself: the state and the parent
/// follow the last `)`.
fn ppid_in(stat: &str) -> Option<u32> {
    let rest = stat.get(stat.rfind(')')? + 1..)?;
    rest.split_whitespace().nth(1)?.parse().ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::{Command, Stdio};

    #[test]
    fn sola_programs_are_not_foreign() {
        assert!(is_foreign("/bin/steam"));
        assert!(is_foreign("steam -silent"));
        assert!(!is_foreign("/bin/sola-mail"));
        assert!(!is_foreign("/opt/sola/bin/sola-browser --app web-1"));
        assert!(!is_foreign(""));
    }

    #[test]
    fn tag_is_read_from_the_environment_block() {
        assert_eq!(tag_in(b"A=1\0SOLA_APP=steam\0B=2\0").as_deref(), Some("steam"));
        assert_eq!(tag_in(b"A=1\0XSOLA_APP=steam\0"), None);
        assert_eq!(tag_in(b""), None);
    }

    #[test]
    fn parent_follows_the_last_parenthesis() {
        assert_eq!(ppid_in("42 (steam) S 7 42 42 0"), Some(7));
        assert_eq!(ppid_in("42 (a (b) c) R 9 1 1 0"), Some(9));
        assert_eq!(ppid_in("garbage"), None);
    }

    #[test]
    fn tagged_processes_are_found_and_own_their_untagged_children() {
        let tag = format!("tag-test-{}", std::process::id());
        // A tagged shell; one child keeps the tag, one clears its environment.
        let mut child = Command::new("sh")
            .args(["-c", "sleep 30 & env -i sleep 30 & wait"])
            .env(VAR, &tag)
            .stdin(Stdio::null())
            .spawn()
            .expect("spawn sh");
        std::thread::sleep(std::time::Duration::from_millis(300));
        let kids: Vec<u32> = std::fs::read_dir("/proc")
            .unwrap()
            .filter_map(|e| e.ok()?.file_name().to_str()?.parse::<u32>().ok())
            .filter(|&pid| parent_of(pid) == Some(child.id()))
            .collect();
        assert_eq!(kids.len(), 2, "{kids:?}");
        let found = pids(&tag);
        assert_eq!(found.len(), 2, "the shell and its tagged sleep: {found:?}");
        assert!(found.contains(&child.id()));
        let bare = kids.iter().copied().find(|k| !found.contains(k)).unwrap();
        assert_eq!(owner(bare).as_deref(), Some(tag.as_str()));
        signal(&kids, libc::SIGKILL);
        signal(&[child.id()], libc::SIGKILL);
        let _ = child.wait();
    }
}
