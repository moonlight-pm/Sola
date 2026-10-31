//! A computer's hook log: every agent hook there appends its event to it
//! (the script [`super::install`] writes), and every computer watching that
//! computer's sessions follows it — this one by reading the file
//! ([`follow_local`]), another over ssh ([`crate::link`]). So a session's
//! marks and islands reach whichever computers have it open, whichever one
//! started it (sessions anywhere, slice 2).
//!
//! One line per event: `<agent>\t<SOLA_PANE_ID>\t<base64 payload>`. A
//! computer that looks at or answers a session's island appends
//! `seen\t<id>\t`, and every other one lets that island go too. Past
//! 8 MiB the script moves the log to `hooks.log.1` and starts a new one;
//! readers follow it by name.

use std::io::{BufRead, BufReader, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::time::Duration;

use base64::Engine;

use super::event::{self, Incoming};

/// The log, under its computer's home.
pub const REL: &str = ".local/state/sola/projects/hooks.log";

/// What a line of the log says.
#[derive(Debug, Clone)]
pub enum Event {
    Hook(Incoming),
    /// Session `pane` (its id there) was looked at on some computer.
    Seen { host: Option<String>, pane: String },
}

/// This computer's log.
pub fn local_path() -> PathBuf {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
        .join(REL)
}

/// The line `seen` writes for session `pane`.
pub fn seen_line(pane: &str) -> String {
    format!("seen\t{pane}\t\n")
}

/// Read one line of `host`'s log (`None`: this computer's).
pub fn parse(line: &str, host: Option<&str>) -> Option<Event> {
    let mut f = line.trim_end_matches(['\n', '\r']).splitn(3, '\t');
    let (agent, pane, payload) = (f.next()?, f.next()?, f.next().unwrap_or(""));
    if agent == "seen" {
        return (!pane.is_empty()).then(|| Event::Seen {
            host: host.map(str::to_string),
            pane: pane.to_string(),
        });
    }
    let body = base64::engine::general_purpose::STANDARD.decode(payload).ok()?;
    let pane = (!pane.is_empty()).then(|| pane.to_string());
    let mut incoming = event::from_json(pane, Some(agent.to_string()), &body).ok()??;
    incoming.host = host.map(str::to_string);
    Some(Event::Hook(incoming))
}

/// Follow this computer's log on a thread, from its end, sending each
/// event to `tx`.
pub fn follow_local(tx: mpsc::Sender<Event>) {
    let path = local_path();
    let spawned = std::thread::Builder::new()
        .name("hook-log".into())
        .spawn(move || follow(&path, &tx));
    if let Err(e) = spawned {
        tracing::error!("hook log thread: {e}");
    }
}

fn follow(path: &Path, tx: &mpsc::Sender<Event>) {
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let mut from_end = true;
    loop {
        let file = match std::fs::OpenOptions::new().create(true).append(true).read(true).open(path) {
            Ok(f) => f,
            Err(e) => {
                tracing::warn!(path = %path.display(), "hook log: {e}");
                std::thread::sleep(Duration::from_secs(2));
                continue;
            }
        };
        let ino = inode(&file);
        let mut reader = BufReader::new(file);
        let mut at = if from_end {
            reader.seek(SeekFrom::End(0)).unwrap_or(0)
        } else {
            0
        };
        from_end = false;
        let mut line = String::new();
        loop {
            match reader.read_line(&mut line) {
                Ok(0) => {
                    std::thread::sleep(Duration::from_millis(150));
                    // Moved away (rotated) or cut: read the new one from its start.
                    let now = std::fs::metadata(path).ok();
                    if now.as_ref().is_none_or(|m| inode_of(m) != ino || m.len() < at) {
                        break;
                    }
                }
                Ok(n) => {
                    at += n as u64;
                    if !line.ends_with('\n') {
                        // Half a line: the rest is still being written.
                        continue;
                    }
                    if let Some(ev) = parse(&line, None) {
                        if tx.send(ev).is_err() {
                            return;
                        }
                    }
                    line.clear();
                }
                Err(e) => {
                    tracing::warn!(path = %path.display(), "hook log read: {e}");
                    break;
                }
            }
        }
    }
}

fn inode(file: &std::fs::File) -> u64 {
    file.metadata().map(|m| inode_of(&m)).unwrap_or(0)
}

fn inode_of(m: &std::fs::Metadata) -> u64 {
    use std::os::unix::fs::MetadataExt;
    m.ino()
}

/// Say session `pane` (its id on this computer) was looked at.
pub fn append_seen_local(pane: &str) {
    use std::io::Write;
    let path = local_path();
    let written = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .and_then(|mut f| f.write_all(seen_line(pane).as_bytes()));
    if let Err(e) = written {
        tracing::warn!(path = %path.display(), "hook log: {e}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn line(agent: &str, pane: &str, json: &str) -> String {
        let b64 = base64::engine::general_purpose::STANDARD.encode(json);
        format!("{agent}\t{pane}\t{b64}\n")
    }

    #[test]
    fn a_hook_line_is_its_event_from_its_computer() {
        let l = line("claude", "ws-main", r#"{"hook_event_name":"Stop","session_id":"sid","cwd":"/src"}"#);
        let Some(Event::Hook(i)) = parse(&l, Some("novus")) else {
            panic!("not a hook");
        };
        assert_eq!(i.pane_id, "ws-main");
        assert_eq!(i.agent, "claude");
        assert_eq!(i.host.as_deref(), Some("novus"));
        assert_eq!(i.mapped.status, Some(crate::status::AgentStatus::Done));
    }

    #[test]
    fn a_codex_line_may_have_no_pane() {
        let l = line("codex", "", r#"{"hookEventName":"UserPromptSubmit","cwd":"/src/rim"}"#);
        let Some(Event::Hook(i)) = parse(&l, None) else {
            panic!("not a hook");
        };
        assert!(i.pane_id.is_empty());
        assert_eq!(i.cwd.as_deref(), Some("/src/rim"));
    }

    #[test]
    fn seen_and_junk() {
        let Some(Event::Seen { host, pane }) = parse(&seen_line("ws-main"), Some("canto")) else {
            panic!("not seen");
        };
        assert_eq!((host.as_deref(), pane.as_str()), (Some("canto"), "ws-main"));
        assert!(parse("claude\tws\tnot base64!\n", None).is_none());
        assert!(parse("\n", None).is_none());
    }

    #[test]
    fn following_reads_what_is_appended() {
        let dir = std::env::temp_dir().join(format!("sola-hooklog-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("hooks.log");
        std::fs::write(&path, line("grok", "old", r#"{"hookEventName":"Stop"}"#)).unwrap();
        let (tx, rx) = mpsc::channel();
        let p = path.clone();
        std::thread::spawn(move || follow(&p, &tx));
        std::thread::sleep(Duration::from_millis(300));
        use std::io::Write;
        let mut f = std::fs::OpenOptions::new().append(true).open(&path).unwrap();
        f.write_all(line("grok", "new", r#"{"hookEventName":"Stop"}"#).as_bytes()).unwrap();
        let Ok(Event::Hook(i)) = rx.recv_timeout(Duration::from_secs(3)) else {
            panic!("nothing read");
        };
        assert_eq!(i.pane_id, "new", "only what came after it started");
        // Rotated: the new log is read from its start.
        std::fs::rename(&path, dir.join("hooks.log.1")).unwrap();
        std::fs::write(&path, seen_line("after")).unwrap();
        let Ok(Event::Seen { pane, .. }) = rx.recv_timeout(Duration::from_secs(3)) else {
            panic!("rotation not followed");
        };
        assert_eq!(pane, "after");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
