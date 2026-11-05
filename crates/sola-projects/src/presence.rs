//! Process-tree presence. Tells us *who* is in the pane, not state.
//!
//! Grok is preferred when several agents share a tree. OpenCode is
//! presence-only; Grok, Codex, and Claude also get hooks.

use sola_terminal::tmux;
use std::collections::{HashMap, VecDeque};
use std::fs;

use crate::machine::Machine;

/// Known agent binaries, Grok first.
pub const AGENTS: &[&str] = &["grok", "claude", "codex", "opencode"];

/// Who is live in the pane. `Unknown` means the walk failed — do not
/// treat that as “back to shell” or a live agent will flicker idle.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Presence {
    Unknown,
    Shell,
    Agent(&'static str),
}

/// Who is in each of `sessions` on `m`, from one look at its panes and its
/// process table however many there are. A session with no answer is not in
/// the result.
pub fn scan(m: &Machine, sessions: &[String]) -> HashMap<String, Presence> {
    match m {
        Machine::Local => scan_local(sessions),
        Machine::Remote(_) => scan_remote(m, sessions),
    }
}

fn scan_local(sessions: &[String]) -> HashMap<String, Presence> {
    let Some(panes) = tmux::pane_pids() else {
        return HashMap::new();
    };
    let table = ProcessTable::read();
    sessions
        .iter()
        .filter_map(|session| {
            let pid = *panes.get(session)?;
            let who = scan_tree(pid, |p| table.agent(p), |p| table.children(p));
            Some((session.clone(), who.map_or(Presence::Shell, Presence::Agent)))
        })
        .collect()
}

/// The agent in the process tree under `root` (Grok first when several).
fn scan_tree(
    root: i32,
    agent_of: impl Fn(i32) -> Option<&'static str>,
    children: impl Fn(i32) -> Vec<i32>,
) -> Option<&'static str> {
    let mut found = Vec::new();
    let mut q = VecDeque::from([root]);
    let mut seen = std::collections::HashSet::new();
    while let Some(pid) = q.pop_front() {
        if !seen.insert(pid) {
            continue;
        }
        if let Some(name) = agent_of(pid) {
            if !found.contains(&name) {
                found.push(name);
            }
        }
        for child in children(pid) {
            q.push_back(child);
        }
    }
    if found.contains(&"grok") {
        return Some("grok");
    }
    found.into_iter().next()
}

/// [`scan`] on another computer: its panes' pids and its process table, by
/// one script over ssh.
pub fn scan_remote(m: &Machine, sessions: &[String]) -> HashMap<String, Presence> {
    let pick = sola_terminal::remote::pick("\"=$s\"");
    let script = format!(
        "for s in \"$@\"; do {pick} p=$(tmux -L \"$L\" display-message -p -t \"=$s:\" \"#{{pane_pid}}\" 2>/dev/null) \
         && echo \"$s $p\"; done; echo --; ps -eo pid=,ppid=,comm=,args="
    );
    let mut argv = vec!["sh", "-c", script.as_str(), "sh"];
    argv.extend(sessions.iter().map(String::as_str));
    let out = match m.run(None, &[], &argv, None) {
        Ok(o) if o.status.success() => o.stdout,
        _ => return HashMap::new(),
    };
    parse_remote_scan(&String::from_utf8_lossy(&out))
}

/// `<session> <pid>` lines, `--`, then `ps -eo pid=,ppid=,comm=,args=`.
fn parse_remote_scan(text: &str) -> HashMap<String, Presence> {
    let (panes, table) = text.split_once("\n--\n").unwrap_or((text, ""));
    let mut parent: HashMap<i32, Vec<i32>> = HashMap::new();
    let mut agent: HashMap<i32, &'static str> = HashMap::new();
    for line in table.lines() {
        let mut f = line.split_whitespace();
        let (Some(pid), Some(ppid), Some(comm)) = (f.next(), f.next(), f.next()) else {
            continue;
        };
        let (Ok(pid), Ok(ppid)) = (pid.parse::<i32>(), ppid.parse::<i32>()) else {
            continue;
        };
        parent.entry(ppid).or_default().push(pid);
        let named = agent_from_name(comm).or_else(|| f.next().and_then(agent_from_name));
        if let Some(a) = named {
            agent.insert(pid, a);
        }
    }
    panes
        .lines()
        .filter_map(|line| {
            let (session, pid) = line.split_once(' ')?;
            let pid: i32 = pid.trim().parse().ok()?;
            let who = scan_tree(
                pid,
                |p| agent.get(&p).copied(),
                |p| parent.get(&p).cloned().unwrap_or_default(),
            );
            Some((session.to_string(), who.map_or(Presence::Shell, Presence::Agent)))
        })
        .collect()
}

pub fn agent_from_name(name: &str) -> Option<&'static str> {
    let base = name.rsplit('/').next().unwrap_or(name);
    if base == "grok" || base.starts_with("grok-") {
        return Some("grok");
    }
    for &agent in &AGENTS[1..] {
        if base == agent || base.starts_with(&format!("{agent}-")) {
            return Some(agent);
        }
    }
    None
}

/// One read of `/proc`: each process's children, and the agents known by
/// name. A walk reads this, not `/proc`: finding a process's children there
/// costs a read of every process here, far too much per process walked.
#[derive(Default)]
struct ProcessTable {
    children: HashMap<i32, Vec<i32>>,
    agents: HashMap<i32, &'static str>,
}

impl ProcessTable {
    fn read() -> Self {
        let mut table = Self::default();
        let Ok(entries) = fs::read_dir("/proc") else {
            return table;
        };
        for entry in entries.flatten() {
            let Some(pid) = entry.file_name().to_str().and_then(|n| n.parse().ok()) else {
                continue;
            };
            // Gone already.
            let Ok(stat) = fs::read(entry.path().join("stat")) else {
                continue;
            };
            if let Some((name, parent)) = parse_stat(&String::from_utf8_lossy(&stat)) {
                table.children.entry(parent).or_default().push(pid);
                if let Some(agent) = agent_from_name(name) {
                    table.agents.insert(pid, agent);
                }
            }
        }
        table
    }

    fn children(&self, pid: i32) -> Vec<i32> {
        self.children.get(&pid).cloned().unwrap_or_default()
    }

    /// By its name, else by its binary (read here: only for what is walked).
    fn agent(&self, pid: i32) -> Option<&'static str> {
        self.agents.get(&pid).copied().or_else(|| {
            let exe = fs::read_link(format!("/proc/{pid}/exe")).ok()?;
            agent_from_name(&exe.to_string_lossy())
        })
    }
}

/// `(name, parent)` from a `/proc/<pid>/stat` line. A name can hold spaces
/// and parentheses, so it ends at the last `)`.
fn parse_stat(line: &str) -> Option<(&str, i32)> {
    let name = line.get(line.find('(')? + 1..line.rfind(')')?)?;
    let parent = line.get(line.rfind(')')? + 1..)?.split_whitespace().nth(1)?;
    Some((name, parent.parse().ok()?))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_remote_scan_finds_the_agent_under_each_pane() {
        let text = "sws-a 100\nsws-b 200\n--\n\
            100 1 zsh -zsh\n\
            101 100 node /usr/bin/claude --resume x\n\
            200 1 zsh -zsh\n\
            201 200 vim vim notes\n\
            300 1 grok grok\n";
        let found = parse_remote_scan(text);
        assert_eq!(found["sws-a"], Presence::Agent("claude"));
        assert_eq!(found["sws-b"], Presence::Shell);
        assert!(!found.contains_key("sws-c"));
    }

    #[test]
    fn a_stat_line_gives_the_name_and_the_parent() {
        assert_eq!(parse_stat("12 (claude) S 34 12 12 0"), Some(("claude", 34)));
        assert_eq!(parse_stat("12 (a b) (c)) R 34 12"), Some(("a b) (c)", 34)));
        assert_eq!(parse_stat("12 (claude) S"), None);
        assert_eq!(parse_stat("not a stat line"), None);
    }

    #[test]
    fn the_table_finds_a_child_and_knows_it_by_name() {
        use std::os::unix::process::CommandExt;
        let dir = std::env::temp_dir().join(format!("sola-presence-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let link = dir.join("claude");
        let sleep = std::path::PathBuf::from("/bin/sleep").canonicalize().unwrap();
        let _ = std::fs::remove_file(&link);
        std::os::unix::fs::symlink(&sleep, &link).unwrap();
        let mut child = std::process::Command::new(&link).arg0("sleep").arg("30").spawn().unwrap();
        let pid = child.id() as i32;

        let table = ProcessTable::read();
        let walked = scan_tree(pid, |p| table.agent(p), |p| table.children(p));
        let mine = table.children(std::process::id() as i32);

        let _ = child.kill();
        let _ = child.wait();
        let _ = std::fs::remove_dir_all(&dir);
        assert!(mine.contains(&pid));
        assert_eq!(walked, Some("claude"));
    }

    #[test]
    fn a_remote_scan_runs_here() {
        // No such tmux sessions: the table still comes, the panes do not.
        let found = scan_remote(&Machine::Local, &["sws-no-such-session".into()]);
        assert!(found.is_empty());
    }

    #[test]
    fn grok_is_first_and_matches_prefix() {
        assert_eq!(agent_from_name("grok"), Some("grok"));
        assert_eq!(agent_from_name("/opt/sola/bin/grok"), Some("grok"));
        assert_eq!(agent_from_name("grok-1.0.3-linux-x86_64"), Some("grok"));
        assert_eq!(agent_from_name("claude"), Some("claude"));
        assert_eq!(agent_from_name("codex"), Some("codex"));
        assert_eq!(agent_from_name("codex-0.50.0"), Some("codex"));
        assert_eq!(agent_from_name("bash"), None);
        assert_eq!(AGENTS[0], "grok");
    }
}
