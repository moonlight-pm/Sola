//! Has an app's package moved? A running app runs the program it started
//! with: on Oath that is a store directory, which never changes, so an apply
//! that moves the app's package leaves it on the old program while its
//! command (`/bin/sola-mail`) now leads to the new one.

use std::path::PathBuf;

/// The program process `pid` runs.
pub fn running(pid: u32) -> Option<PathBuf> {
    let exe = std::fs::read_link(format!("/proc/{pid}/exe")).ok()?;
    let exe = exe.to_string_lossy();
    Some(PathBuf::from(exe.strip_suffix(" (deleted)").unwrap_or(&exe)))
}

/// The program `command` names now: its first word, through its links.
pub fn named(command: &str) -> Option<PathBuf> {
    let program = command.split_whitespace().next()?;
    std::fs::canonicalize(program).ok()
}

/// Process `pid` runs another program than `command` names now.
pub fn moved(command: &str, pid: u32) -> bool {
    match (running(pid), named(command)) {
        (Some(running), Some(named)) => running != named,
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_process_on_its_own_program_has_not_moved() {
        let me = std::process::id();
        let exe = std::env::current_exe().unwrap();
        assert!(!moved(exe.to_str().unwrap(), me));
        assert!(moved("/bin/sh", me), "another program");
        assert!(!moved("/no/such/program", me), "nothing to tell by");
    }
}
