//! Where a project's files and processes are: this computer, or another
//! one over ssh. Its tabs' git, startup scripts and agent sessions are
//! asked of a [`Machine`], which answers the same for both. A
//! [`Machine::Remote`] answers over the shared ssh connection
//! ([`sola_core::ssh`]), each question a command there, so ask it off the
//! UI thread.
//!
//! The remote answers come from plain tools (`find`, `head`, `tail`,
//! `cut`, `stat`). Each is a command the local machine can run too, so the
//! tests run the remote path here (the `*_by_tools` methods).

use std::io::{BufRead, BufReader, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use sola_core::ssh;

#[derive(Clone, Debug)]
pub enum Machine {
    Local,
    Remote(Arc<Remote>),
}

/// Another computer, as its link found it.
#[derive(Debug, Clone, PartialEq)]
pub struct Remote {
    pub host: String,
    pub home: PathBuf,
    /// `$GROK_HOME`, `$CODEX_HOME` and `$CLAUDE_CONFIG_DIR` there, or
    /// their defaults under `home`.
    pub grok_home: PathBuf,
    pub codex_home: PathBuf,
    pub claude_home: PathBuf,
}

/// An agent whose sessions Projects reads.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Agent {
    Grok,
    Codex,
    Claude,
}

/// A folder's entry. `name` is the file name, or the path below the folder
/// for [`Machine::walk`].
#[derive(Debug, Clone, PartialEq)]
pub struct Entry {
    pub name: String,
    pub dir: bool,
    pub file: bool,
    pub len: u64,
    pub modified: SystemTime,
}

/// The heads of a file's complete lines from some offset
/// ([`Machine::line_heads`]).
#[derive(Debug, Default, PartialEq)]
pub struct Heads {
    pub heads: Vec<Vec<u8>>,
    /// The offset just past the last complete line read.
    pub end: u64,
}

impl Machine {
    pub fn is_local(&self) -> bool {
        matches!(self, Machine::Local)
    }

    /// The host, for a remote machine.
    pub fn host(&self) -> Option<&str> {
        match self {
            Machine::Local => None,
            Machine::Remote(r) => Some(&r.host),
        }
    }

    /// Run `argv` there, in `dir`, with `env` added. `Err` only when it
    /// could not run (or ssh could not reach the host); its exit status is
    /// the caller's to read.
    pub fn run(
        &self,
        dir: Option<&Path>,
        env: &[(&str, &str)],
        argv: &[&str],
        stdin: Option<&[u8]>,
    ) -> Result<Output, String> {
        match self {
            Machine::Local => {
                let (program, args) = argv.split_first().ok_or("an empty command")?;
                let mut cmd = Command::new(program);
                cmd.args(args).envs(env.iter().copied());
                if let Some(dir) = dir {
                    cmd.current_dir(dir);
                }
                output_of(cmd, stdin).map_err(|e| format!("{program}: {e}"))
            }
            Machine::Remote(r) => {
                let dir = dir.map(|d| d.to_string_lossy().into_owned());
                let script = ssh::argv_script(dir.as_deref(), env, argv);
                ssh::run(ssh::batch(&r.host, &script), stdin)
            }
        }
    }

    /// Where `agent` keeps its sessions there.
    pub fn agent_home(&self, agent: Agent) -> PathBuf {
        match self {
            Machine::Remote(r) => match agent {
                Agent::Grok => r.grok_home.clone(),
                Agent::Codex => r.codex_home.clone(),
                Agent::Claude => r.claude_home.clone(),
            },
            Machine::Local => {
                let (var, dir) = match agent {
                    Agent::Grok => ("GROK_HOME", ".grok"),
                    Agent::Codex => ("CODEX_HOME", ".codex"),
                    Agent::Claude => ("CLAUDE_CONFIG_DIR", ".claude"),
                };
                std::env::var_os(var).map(PathBuf::from).unwrap_or_else(|| {
                    std::env::var_os("HOME")
                        .map(PathBuf::from)
                        .unwrap_or_else(|| PathBuf::from("."))
                        .join(dir)
                })
            }
        }
    }

    /// `dir`'s entries (symlinks are neither dirs nor files). `None`: no
    /// such folder.
    pub fn list(&self, dir: &Path) -> Option<Vec<Entry>> {
        match self {
            Machine::Local => {
                let rd = std::fs::read_dir(dir).ok()?;
                Some(
                    rd.filter_map(|e| e.ok())
                        .filter_map(|e| {
                            let ft = e.file_type().ok()?;
                            let meta = e.metadata().ok()?;
                            Some(Entry {
                                name: e.file_name().to_string_lossy().into_owned(),
                                dir: ft.is_dir(),
                                file: ft.is_file(),
                                len: meta.len(),
                                modified: meta.modified().unwrap_or(UNIX_EPOCH),
                            })
                        })
                        .collect(),
                )
            }
            Machine::Remote(_) => self.list_by_tools(dir, 1),
        }
    }

    /// Everything below `dir`, `depth` levels down, named by its path
    /// below `dir`.
    pub fn walk(&self, dir: &Path, depth: u8) -> Vec<Entry> {
        match self {
            Machine::Local => {
                let mut out = Vec::new();
                walk_local(dir, Path::new(""), depth, &mut out);
                out
            }
            Machine::Remote(_) => self.list_by_tools(dir, depth).unwrap_or_default(),
        }
    }

    /// The first folder (`want_dir`) or file named like `glob` (`*` for any
    /// run of characters) from `min` to `max` levels below `dir`. Here,
    /// each folder's own entries are tried before its subfolders.
    pub fn find(&self, dir: &Path, min: u8, max: u8, want_dir: bool, glob: &str) -> Option<PathBuf> {
        match self {
            Machine::Local => find_local(dir, 1, min, max, want_dir, glob),
            Machine::Remote(_) => self.find_by_tools(dir, min, max, want_dir, glob),
        }
    }

    /// What `path` is (following symlinks).
    pub fn stat(&self, path: &Path) -> Option<Entry> {
        match self {
            Machine::Local => {
                let meta = std::fs::metadata(path).ok()?;
                Some(Entry {
                    name: file_name(path),
                    dir: meta.is_dir(),
                    file: meta.is_file(),
                    len: meta.len(),
                    modified: meta.modified().unwrap_or(UNIX_EPOCH),
                })
            }
            Machine::Remote(_) => self.stat_by_tools(path),
        }
    }

    pub fn exists(&self, path: &Path) -> bool {
        self.stat(path).is_some()
    }

    pub fn read(&self, path: &Path) -> Option<Vec<u8>> {
        match self {
            Machine::Local => std::fs::read(path).ok(),
            Machine::Remote(_) => self.output_of(&["cat", "--", &lossy(path)]),
        }
    }

    pub fn read_to_string(&self, path: &Path) -> Option<String> {
        self.read(path)
            .map(|b| String::from_utf8_lossy(&b).into_owned())
    }

    /// The first `n` bytes of `path`.
    pub fn read_head(&self, path: &Path, n: u64) -> Option<Vec<u8>> {
        match self {
            Machine::Local => {
                let mut buf = Vec::new();
                std::fs::File::open(path)
                    .ok()?
                    .take(n)
                    .read_to_end(&mut buf)
                    .ok()?;
                Some(buf)
            }
            Machine::Remote(_) => self.output_of(&["head", "-c", &n.to_string(), "--", &lossy(path)]),
        }
    }

    /// The last `n` bytes of `path`.
    pub fn read_tail(&self, path: &Path, n: u64) -> Option<Vec<u8>> {
        match self {
            Machine::Local => {
                let mut file = std::fs::File::open(path).ok()?;
                let len = file.metadata().ok()?.len();
                file.seek(SeekFrom::Start(len.saturating_sub(n))).ok()?;
                let mut buf = Vec::new();
                file.read_to_end(&mut buf).ok()?;
                Some(buf)
            }
            Machine::Remote(_) => self.output_of(&["tail", "-c", &n.to_string(), "--", &lossy(path)]),
        }
    }

    /// The first `head` bytes of each complete line of `path` from byte
    /// `start` (a line's start). A last line with no newline yet is left
    /// for later: `end` stops before it. Long lines are never held whole —
    /// there, only their heads leave the computer.
    pub fn line_heads(&self, path: &Path, start: u64, head: usize) -> Option<Heads> {
        match self {
            Machine::Local => line_heads_local(path, start, head),
            Machine::Remote(_) => self.line_heads_by_tools(path, start, head),
        }
    }

    pub fn mkdir_p(&self, dir: &Path) -> Result<(), String> {
        match self {
            Machine::Local => {
                std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))
            }
            Machine::Remote(_) => self.check(&["mkdir", "-p", "--", &lossy(dir)]),
        }
    }

    /// Write `bytes` to `path` (whole, by a rename), `mode` set if given.
    pub fn write(&self, path: &Path, bytes: &[u8], mode: Option<u32>) -> Result<(), String> {
        match self {
            Machine::Local => {
                std::fs::write(path, bytes).map_err(|e| format!("{}: {e}", path.display()))?;
                if let Some(mode) = mode {
                    use std::os::unix::fs::PermissionsExt;
                    std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode))
                        .map_err(|e| format!("{}: {e}", path.display()))?;
                }
                Ok(())
            }
            Machine::Remote(_) => self.write_by_tools(path, bytes, mode),
        }
    }

    // ── The remote answers, by plain tools (any machine runs them) ──────

    fn list_by_tools(&self, dir: &Path, depth: u8) -> Option<Vec<Entry>> {
        let depth = depth.max(1).to_string();
        let dir = lossy(dir);
        let out = self.output_of(&[
            "find", &dir, "-mindepth", "1", "-maxdepth", &depth, "-printf", FIND_FORMAT_BELOW,
        ])?;
        Some(parse_entries(&out))
    }

    fn find_by_tools(&self, dir: &Path, min: u8, max: u8, want_dir: bool, glob: &str) -> Option<PathBuf> {
        let (min, max) = (min.max(1).to_string(), max.max(1).to_string());
        let out = self.output_of(&[
            "find",
            &lossy(dir),
            "-mindepth",
            &min,
            "-maxdepth",
            &max,
            "-type",
            if want_dir { "d" } else { "f" },
            "-name",
            glob,
            "-print",
            "-quit",
        ])?;
        let found = String::from_utf8_lossy(&out).trim_end_matches('\n').to_string();
        (!found.is_empty()).then(|| PathBuf::from(found))
    }

    fn stat_by_tools(&self, path: &Path) -> Option<Entry> {
        let out = self.output_of(&["find", "-L", &lossy(path), "-maxdepth", "0", "-printf", FIND_FORMAT_NAME])?;
        parse_entries(&out).into_iter().next()
    }

    fn line_heads_by_tools(&self, path: &Path, start: u64, head: usize) -> Option<Heads> {
        let out = self.output_of(&[
            "sh",
            "-c",
            LINE_HEADS_SCRIPT,
            "sh",
            &lossy(path),
            &start.to_string(),
            &head.max(1).to_string(),
        ])?;
        parse_heads(&out)
    }

    fn write_by_tools(&self, path: &Path, bytes: &[u8], mode: Option<u32>) -> Result<(), String> {
        let mode = format!("{:o}", mode.unwrap_or(0o644));
        let out = self.run(None, &[], &["sh", "-c", WRITE_SCRIPT, "sh", &lossy(path), &mode], Some(bytes))?;
        if out.status.success() {
            Ok(())
        } else {
            Err(format!("{}: {}", path.display(), ssh::reason(&out.stderr)))
        }
    }

    /// `argv`'s stdout when it exits 0.
    fn output_of(&self, argv: &[&str]) -> Option<Vec<u8>> {
        let out = self.run(None, &[], argv, None).ok()?;
        out.status.success().then_some(out.stdout)
    }

    /// `argv` exits 0, else what it said.
    fn check(&self, argv: &[&str]) -> Result<(), String> {
        let out = self.run(None, &[], argv, None)?;
        if out.status.success() {
            Ok(())
        } else {
            Err(ssh::reason(&out.stderr))
        }
    }
}

/// `find -printf`: type, size, mtime, then the path below the start (or
/// the name). Tabs part the fields.
const FIND_FORMAT_BELOW: &str = "%y\t%s\t%T@\t%P\n";
const FIND_FORMAT_NAME: &str = "%y\t%s\t%T@\t%f\n";

/// `$1` from `$2` with heads of `$3` bytes: the end offset on the first
/// line, then each complete line's head.
const LINE_HEADS_SCRIPT: &str = r#"f="$1"; s="$2"; h="$3"
n=$(stat -L -c %s -- "$f") || exit 3
if [ "$n" -le "$s" ]; then echo "$s"; exit 0; fi
if [ "$(tail -c 1 -- "$f" | od -An -tx1 | tr -d ' ')" = "0a" ]; then p=0; else p=$(tail -n 1 -- "$f" | wc -c); fi
e=$((n - p))
if [ "$e" -lt "$s" ]; then e=$s; fi
echo "$e"
if [ "$e" -gt "$s" ]; then tail -c +$((s + 1)) -- "$f" | head -c $((e - s)) | cut -b "1-$h"; fi
exit 0"#;

/// `$1` from stdin, mode `$2`, by a rename in its folder.
const WRITE_SCRIPT: &str = r#"t="$1.sola-$$" && cat > "$t" && chmod "$2" "$t" && mv -f "$t" "$1" || { rm -f "$t"; exit 1; }"#;

fn parse_entries(out: &[u8]) -> Vec<Entry> {
    String::from_utf8_lossy(out)
        .lines()
        .filter_map(|line| {
            let mut f = line.splitn(4, '\t');
            let kind = f.next()?;
            let len = f.next()?.parse().ok()?;
            let secs: f64 = f.next()?.parse().ok()?;
            let name = f.next()?.to_string();
            Some(Entry {
                name,
                dir: kind == "d",
                file: kind == "f",
                len,
                modified: UNIX_EPOCH + Duration::from_secs_f64(secs.max(0.0)),
            })
        })
        .collect()
}

fn parse_heads(out: &[u8]) -> Option<Heads> {
    let mut lines = out.split(|&b| b == b'\n');
    let end = std::str::from_utf8(lines.next()?).ok()?.trim().parse().ok()?;
    let mut heads: Vec<Vec<u8>> = lines.map(<[u8]>::to_vec).collect();
    // The output's own last newline leaves an empty piece.
    if heads.last().is_some_and(|h| h.is_empty()) {
        heads.pop();
    }
    Some(Heads { heads, end })
}

fn line_heads_local(path: &Path, start: u64, head: usize) -> Option<Heads> {
    let mut file = std::fs::File::open(path).ok()?;
    if start > 0 {
        file.seek(SeekFrom::Start(start)).ok()?;
    }
    let mut reader = BufReader::new(file);
    let mut out = Heads {
        heads: Vec::new(),
        end: start,
    };
    loop {
        let buf = match reader.fill_buf() {
            Ok(b) if !b.is_empty() => b,
            Ok(_) => break,
            Err(_) => return None,
        };
        match buf.iter().position(|&b| b == b'\n') {
            Some(i) => {
                out.heads.push(buf[..i.min(head)].to_vec());
                reader.consume(i + 1);
                out.end += (i + 1) as u64;
            }
            None => {
                // The line runs past this buffer: keep its head, skip on.
                let taken = buf[..buf.len().min(head)].to_vec();
                let n = buf.len();
                reader.consume(n);
                let Some(skipped) = skip_line(&mut reader) else {
                    // No newline yet: a line still being written.
                    break;
                };
                out.heads.push(taken);
                out.end += n as u64 + skipped;
            }
        }
    }
    Some(out)
}

/// Bytes through the next newline; `None` at the end with none.
fn skip_line(reader: &mut impl BufRead) -> Option<u64> {
    let mut n = 0u64;
    loop {
        let buf = match reader.fill_buf() {
            Ok(b) if !b.is_empty() => b,
            _ => return None,
        };
        if let Some(i) = buf.iter().position(|&b| b == b'\n') {
            reader.consume(i + 1);
            return Some(n + (i + 1) as u64);
        }
        let len = buf.len();
        reader.consume(len);
        n += len as u64;
    }
}

fn find_local(dir: &Path, level: u8, min: u8, max: u8, want_dir: bool, glob: &str) -> Option<PathBuf> {
    let entries = Machine::Local.list(dir)?;
    if level >= min {
        let hit = entries
            .iter()
            .find(|e| (if want_dir { e.dir } else { e.file }) && glob_match(glob, &e.name));
        if let Some(e) = hit {
            return Some(dir.join(&e.name));
        }
    }
    if level >= max {
        return None;
    }
    entries
        .iter()
        .filter(|e| e.dir)
        .find_map(|e| find_local(&dir.join(&e.name), level + 1, min, max, want_dir, glob))
}

/// `name` is like `glob`, whose `*` stands for any run of characters.
fn glob_match(glob: &str, name: &str) -> bool {
    let mut parts = glob.split('*');
    let first = parts.next().unwrap_or_default();
    let Some(mut rest) = name.strip_prefix(first) else {
        return false;
    };
    let parts: Vec<&str> = parts.collect();
    let Some((last, middle)) = parts.split_last() else {
        return rest.is_empty();
    };
    for part in middle {
        match rest.find(part) {
            Some(i) => rest = &rest[i + part.len()..],
            None => return false,
        }
    }
    rest.len() >= last.len() && rest.ends_with(last)
}

fn walk_local(dir: &Path, below: &Path, depth: u8, out: &mut Vec<Entry>) {
    if depth == 0 {
        return;
    }
    let Some(entries) = Machine::Local.list(dir) else {
        return;
    };
    for mut e in entries {
        let rel = below.join(&e.name);
        if e.dir {
            walk_local(&dir.join(&e.name), &rel, depth - 1, out);
        }
        e.name = rel.to_string_lossy().into_owned();
        out.push(e);
    }
}

fn output_of(mut cmd: Command, stdin: Option<&[u8]>) -> std::io::Result<Output> {
    cmd.stdin(if stdin.is_some() {
        Stdio::piped()
    } else {
        Stdio::null()
    })
    .stdout(Stdio::piped())
    .stderr(Stdio::piped());
    let mut child = cmd.spawn()?;
    if let (Some(bytes), Some(mut pipe)) = (stdin, child.stdin.take()) {
        pipe.write_all(bytes)?;
    }
    child.wait_with_output()
}

fn lossy(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

fn file_name(path: &Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("sola-machine-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn listing_by_tools_matches_listing_here() {
        let dir = tmp("list");
        std::fs::create_dir_all(dir.join("sub/deeper")).unwrap();
        std::fs::write(dir.join("a b.jsonl"), "12345").unwrap();
        std::fs::write(dir.join("sub/c.txt"), "x").unwrap();
        let m = Machine::Local;
        let sorted = |mut v: Vec<Entry>| {
            v.sort_by(|a, b| a.name.cmp(&b.name));
            v.into_iter()
                .map(|e| (e.name, e.dir, e.file, if e.dir { 0 } else { e.len }))
                .collect::<Vec<_>>()
        };
        let here = sorted(m.list(&dir).unwrap());
        assert_eq!(here, sorted(m.list_by_tools(&dir, 1).unwrap()));
        assert_eq!(here[0], ("a b.jsonl".into(), false, true, 5));
        let walked = sorted(m.walk(&dir, 3));
        assert_eq!(walked, sorted(m.list_by_tools(&dir, 3).unwrap()));
        assert!(walked.iter().any(|e| e.0 == "sub/deeper" && e.1));
        assert!(m.list(&dir.join("gone")).is_none());
        assert!(m.list_by_tools(&dir.join("gone"), 1).is_none());
        let f = m.stat_by_tools(&dir.join("a b.jsonl")).unwrap();
        assert_eq!((f.file, f.len), (true, 5));
        assert!(m.stat_by_tools(&dir.join("gone")).is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn finding_by_tools_matches_finding_here() {
        let dir = tmp("find");
        std::fs::create_dir_all(dir.join("2026/10/28")).unwrap();
        std::fs::create_dir_all(dir.join("g/sid-a")).unwrap();
        std::fs::write(dir.join("2026/10/28/rollout-2026-10-28T01-02-03-sid-b.jsonl"), "").unwrap();
        let m = Machine::Local;
        let both = |min, max, want_dir, glob: &str| {
            let here = m.find(&dir, min, max, want_dir, glob);
            assert_eq!(here, m.find_by_tools(&dir, min, max, want_dir, glob), "{glob}");
            here
        };
        let rollout = both(1, 5, false, "rollout-*-sid-b.jsonl").unwrap();
        assert!(rollout.ends_with("2026/10/28/rollout-2026-10-28T01-02-03-sid-b.jsonl"));
        assert!(both(1, 3, false, "rollout-*-sid-b.jsonl").is_none());
        assert_eq!(both(2, 2, true, "sid-a"), Some(dir.join("g/sid-a")));
        assert!(both(1, 1, true, "sid-a").is_none());
        assert!(both(1, 5, false, "rollout-*-sid-c.jsonl").is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn globs_take_any_run_for_a_star() {
        assert!(glob_match("rollout-*-abc.jsonl", "rollout-2026T1-abc.jsonl"));
        assert!(!glob_match("rollout-*-abc.jsonl", "rollout-abc.jsonl"));
        assert!(glob_match("abc.jsonl", "abc.jsonl"));
        assert!(!glob_match("abc.jsonl", "xabc.jsonl"));
        assert!(glob_match("a*b*c", "a-b-b-c"));
        assert!(glob_match("*", "anything"));
    }

    #[test]
    fn line_heads_by_tools_match_heads_here() {
        let dir = tmp("heads");
        let path = dir.join("log.jsonl");
        let long = "x".repeat(20_000);
        std::fs::write(&path, format!("one\n{long}\nthree\npart")).unwrap();
        let m = Machine::Local;
        let here = m.line_heads(&path, 0, 8).unwrap();
        assert_eq!(here.heads, [b"one".to_vec(), b"xxxxxxxx".to_vec(), b"three".to_vec()]);
        assert_eq!(here.end, (4 + 20_001 + 6) as u64);
        assert_eq!(m.line_heads_by_tools(&path, 0, 8).unwrap(), here);
        // From the third line on; then nothing new; then the line ends.
        let from = m.line_heads(&path, 4 + 20_001, 8).unwrap();
        assert_eq!(from.heads, [b"three".to_vec()]);
        assert_eq!(m.line_heads_by_tools(&path, 4 + 20_001, 8).unwrap(), from);
        let none = m.line_heads_by_tools(&path, here.end, 8).unwrap();
        assert_eq!((none.heads.len(), none.end), (0, here.end));
        assert_eq!(m.line_heads(&path, here.end, 8).unwrap(), none);
        std::fs::write(&path, format!("one\n{long}\nthree\npart\n")).unwrap();
        let ended = m.line_heads_by_tools(&path, here.end, 8).unwrap();
        assert_eq!(ended.heads, [b"part".to_vec()]);
        assert_eq!(m.line_heads(&path, here.end, 8).unwrap(), ended);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn writing_by_tools_replaces_the_file_with_its_mode() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tmp("write");
        let path = dir.join("hook.sh");
        let m = Machine::Local;
        m.write_by_tools(&path, b"#!/bin/sh\n", Some(0o755)).unwrap();
        m.write_by_tools(&path, b"#!/bin/sh\nexit 0\n", Some(0o755)).unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), b"#!/bin/sh\nexit 0\n");
        let mode = std::fs::metadata(&path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o755);
        assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 1);
        assert!(m.write_by_tools(&dir.join("no/such/dir/f"), b"x", None).is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn run_takes_a_folder_env_and_stdin() {
        let dir = tmp("run");
        let out = Machine::Local
            .run(Some(&dir), &[("NAME", "x")], &["sh", "-c", "pwd; echo $NAME; cat"], Some(b"in"))
            .unwrap();
        let said = String::from_utf8_lossy(&out.stdout);
        assert!(said.ends_with("\nx\nin"), "{said}");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
