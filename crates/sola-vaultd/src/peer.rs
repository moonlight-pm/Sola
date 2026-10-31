//! Who is on the other end of the socket (P3). `SO_PEERCRED` gives the
//! pid; `/proc/<pid>/exe` must be a Sola binary from an allowed place.
//! Vault, the browser and the shell may ask anything; any other Sola app
//! may only redeem a delivery made for it; `solactl` and everything else
//! nothing.

use std::path::{Component, Path, PathBuf};

use sola_vault_proto::Request;

/// Where Sola's binaries live: Oath's store and the NixOS install.
const ROOTS: &[&str] = &["/oath/store/pkg/", "/opt/sola/bin/"];
/// Extra roots for development (`target/release`), colon-separated.
pub const DEV_ROOTS_ENV: &str = "SOLA_VAULT_DEV_ROOTS";

/// The clients that may read and write the vault.
const FULL: &[&str] = &["sola-vault", "sola-browser", "sola-shell"];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    /// Vault, the browser, the shell.
    Full,
    /// Another Sola app: Hello, and Redeem of its own tickets.
    RedeemOnly,
}

/// A connected client.
#[derive(Debug, Clone)]
pub struct Peer {
    pub pid: u32,
    /// The executable's file name, for the log.
    pub exe: String,
    pub role: Option<Role>,
}

/// The allowed roots: the built-in ones and `SOLA_VAULT_DEV_ROOTS`.
pub fn roots() -> Vec<PathBuf> {
    let mut roots: Vec<PathBuf> = ROOTS.iter().map(PathBuf::from).collect();
    roots.extend(dev_roots());
    roots
}

pub fn dev_roots() -> Vec<PathBuf> {
    std::env::var(DEV_ROOTS_ENV)
        .unwrap_or_default()
        .split(':')
        .filter(|s| !s.is_empty())
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .collect()
}

/// Identify the process `pid` from its executable.
pub fn identify(pid: u32, roots: &[PathBuf]) -> Peer {
    match std::fs::read_link(format!("/proc/{pid}/exe")) {
        Ok(exe) => Peer {
            pid,
            exe: exe
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default(),
            role: role(&exe, roots),
        },
        Err(e) => {
            tracing::warn!(pid, error = %e, "vault: peer executable unreadable");
            Peer {
                pid,
                exe: String::new(),
                role: None,
            }
        }
    }
}

/// The role an executable at `exe` gets: none unless it sits under an
/// allowed root and is a Sola binary by name. A binary replaced while it
/// runs reads `<path> (deleted)`; its path still says where it came from.
pub fn role(exe: &Path, roots: &[PathBuf]) -> Option<Role> {
    let raw = exe.to_str()?;
    let exe = Path::new(raw.strip_suffix(" (deleted)").unwrap_or(raw));
    if !exe.is_absolute() || exe.components().any(|c| matches!(c, Component::ParentDir | Component::CurDir)) {
        return None;
    }
    if !roots.iter().any(|root| exe.starts_with(root)) {
        return None;
    }
    let name = exe.file_name()?.to_str()?;
    if FULL.contains(&name) {
        Some(Role::Full)
    } else if name.starts_with("sola-") && name != "solactl" {
        Some(Role::RedeemOnly)
    } else {
        None
    }
}

/// May a client in `role` ask `req`?
pub fn allows(role: Option<Role>, req: &Request) -> bool {
    match role {
        Some(Role::Full) => true,
        Some(Role::RedeemOnly) => matches!(req, Request::Hello { .. } | Request::Redeem { .. }),
        None => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn roots() -> Vec<PathBuf> {
        vec![
            PathBuf::from("/oath/store/pkg/"),
            PathBuf::from("/opt/sola/bin/"),
            PathBuf::from("/home/u/Sola/target/release"),
        ]
    }

    fn of(path: &str) -> Option<Role> {
        role(Path::new(path), &roots())
    }

    #[test]
    fn vault_browser_and_shell_are_full_clients() {
        assert_eq!(of("/oath/store/pkg/ab12-sola-vault/bin/sola-vault"), Some(Role::Full));
        assert_eq!(of("/opt/sola/bin/sola-browser"), Some(Role::Full));
        assert_eq!(of("/opt/sola/bin/sola-shell"), Some(Role::Full));
        assert_eq!(of("/home/u/Sola/target/release/sola-shell"), Some(Role::Full));
    }

    #[test]
    fn other_sola_apps_may_only_redeem() {
        assert_eq!(of("/opt/sola/bin/sola-terminal"), Some(Role::RedeemOnly));
        assert_eq!(of("/oath/store/pkg/x/bin/sola-projects"), Some(Role::RedeemOnly));
    }

    #[test]
    fn solactl_and_strangers_get_nothing() {
        assert_eq!(of("/opt/sola/bin/solactl"), None);
        assert_eq!(of("/opt/sola/bin/sola"), None);
        assert_eq!(of("/usr/bin/python3"), None);
        // A Sola name outside the allowed places.
        assert_eq!(of("/tmp/sola-vault"), None);
        assert_eq!(of("/home/u/sola-browser"), None);
        // No climbing out of a root.
        assert_eq!(of("/opt/sola/bin/../../../tmp/sola-vault"), None);
        assert_eq!(of("sola-vault"), None);
        // A root is a whole path component.
        assert_eq!(of("/opt/sola/binx/sola-vault"), None);
    }

    #[test]
    fn a_replaced_binary_keeps_its_place() {
        assert_eq!(of("/opt/sola/bin/sola-browser (deleted)"), Some(Role::Full));
        assert_eq!(of("/tmp/sola-browser (deleted)"), None);
    }

    #[test]
    fn redeem_only_clients_may_hello_and_redeem() {
        let hello = Request::Hello {
            version: 1,
            client: "t".into(),
        };
        let redeem = Request::Redeem { ticket: "t".into() };
        let get = Request::Get { id: "x".into() };
        assert!(allows(Some(Role::RedeemOnly), &hello));
        assert!(allows(Some(Role::RedeemOnly), &redeem));
        assert!(!allows(Some(Role::RedeemOnly), &get));
        assert!(!allows(Some(Role::RedeemOnly), &Request::Status));
        assert!(allows(Some(Role::Full), &get));
        assert!(!allows(None, &hello));
        assert!(!allows(None, &Request::Status));
    }
}
