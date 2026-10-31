//! Machine power from the flower menu (reboot / poweroff).
//!
//! NixOS: logind via `loginctl` / `systemctl`. Sola has no polkit agent, so
//! the call is non-interactive (`sudo -n` as a last try).
//!
//! Oath (`/oath/INDEX.md`): catalog `host:local` power through `sudo oath`.
//! The menu click is the owner's confirm (`oath apply --confirm`).

use std::process::{Command, ExitStatus};

use sola_core::env::on_oath;

/// Flower-menu action id: reboot the machine (not Exit Sola).
pub const ACTION_RESTART_COMPUTER: &str = "restart-computer";
/// Flower-menu action id: power off the machine.
pub const ACTION_SHUT_DOWN: &str = "shut-down";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Reboot,
    PowerOff,
}

impl Kind {
    pub fn verb(self) -> &'static str {
        match self {
            Self::Reboot => "reboot",
            Self::PowerOff => "poweroff",
        }
    }

    /// Catalog `host:local.power` value (`halt`, not logind `poweroff`).
    fn oath_power(self) -> &'static str {
        match self {
            Self::Reboot => "reboot",
            Self::PowerOff => "halt",
        }
    }

    pub fn fail_toast(self) -> &'static str {
        match self {
            Self::Reboot => "Could not restart the computer",
            Self::PowerOff => "Could not shut down",
        }
    }
}

/// Request reboot or poweroff. Returns `Err` with a toast string if every
/// attempt failed. Does not emit `Topic::Shutdown` — the supervisor SIGTERMs
/// sola as the machine goes down, same graceful path as Exit Sola.
pub fn request(kind: Kind) -> Result<(), &'static str> {
    let verb = kind.verb();
    tracing::info!(verb, "machine power requested via menu");
    if on_oath() {
        return request_oath(kind);
    }
    request_logind(kind)
}

fn request_oath(kind: Kind) -> Result<(), &'static str> {
    let set = run(Command::new("/bin/sudo").args(oath_set_args(kind)));
    if !matches!(set, Ok(st) if st.success()) {
        tracing::warn!(
            ?set,
            power = kind.oath_power(),
            "oath set host power failed"
        );
        return Err(kind.fail_toast());
    }
    let apply = run(Command::new("/bin/sudo").args(oath_apply_args()));
    if !matches!(apply, Ok(st) if st.success()) {
        tracing::warn!(
            ?apply,
            power = kind.oath_power(),
            "oath apply host power failed"
        );
        return Err(kind.fail_toast());
    }
    Ok(())
}

fn request_logind(kind: Kind) -> Result<(), &'static str> {
    let verb = kind.verb();
    for mut cmd in logind_commands(verb) {
        let bin = cmd.get_program().to_string_lossy().into_owned();
        match cmd.status() {
            Ok(status) if status.success() => return Ok(()),
            Ok(status) => {
                tracing::warn!(bin, verb, %status, "power command failed")
            }
            Err(e) => tracing::warn!(bin, verb, %e, "power command exec failed"),
        }
    }
    Err(kind.fail_toast())
}

fn run(cmd: &mut Command) -> std::io::Result<ExitStatus> {
    let bin = cmd.get_program().to_string_lossy().into_owned();
    match cmd.status() {
        Ok(status) => {
            if !status.success() {
                tracing::warn!(bin, %status, "power command failed");
            }
            Ok(status)
        }
        Err(e) => {
            tracing::warn!(bin, %e, "power command exec failed");
            Err(e)
        }
    }
}

fn logind_commands(verb: &str) -> Vec<Command> {
    let bins = [
        "loginctl",
        "/run/current-system/sw/bin/loginctl",
        "systemctl",
        "/run/current-system/sw/bin/systemctl",
    ];
    let mut out: Vec<Command> = bins
        .iter()
        .map(|bin| {
            let mut c = Command::new(bin);
            c.arg(verb);
            c
        })
        .collect();
    let mut sudo = Command::new("sudo");
    sudo.args(["-n", "systemctl", verb]);
    out.push(sudo);
    out
}

fn oath_set_args(kind: Kind) -> Vec<&'static str> {
    match kind {
        Kind::Reboot => vec!["/bin/oath", "set", "host:local", "power=reboot"],
        Kind::PowerOff => vec!["/bin/oath", "set", "host:local", "power=halt"],
    }
}

fn oath_apply_args() -> &'static [&'static str] {
    &["/bin/oath", "apply", "--confirm", "host:local"]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn verbs_match_logind() {
        assert_eq!(Kind::Reboot.verb(), "reboot");
        assert_eq!(Kind::PowerOff.verb(), "poweroff");
        assert_eq!(Kind::Reboot.oath_power(), "reboot");
        assert_eq!(Kind::PowerOff.oath_power(), "halt");
    }

    #[test]
    fn command_list_tries_loginctl_then_systemctl() {
        let cmds = logind_commands("reboot");
        let bins: Vec<String> = cmds
            .iter()
            .map(|c| c.get_program().to_string_lossy().into_owned())
            .collect();
        assert_eq!(
            bins,
            [
                "loginctl",
                "/run/current-system/sw/bin/loginctl",
                "systemctl",
                "/run/current-system/sw/bin/systemctl",
                "sudo",
            ]
        );
        let sudo_args: Vec<String> = cmds
            .last()
            .unwrap()
            .get_args()
            .map(|a| a.to_string_lossy().into_owned())
            .collect();
        assert_eq!(sudo_args, ["-n", "systemctl", "reboot"]);
    }

    #[test]
    fn oath_set_uses_catalog_power() {
        assert_eq!(
            oath_set_args(Kind::Reboot),
            ["/bin/oath", "set", "host:local", "power=reboot"]
        );
        assert_eq!(
            oath_set_args(Kind::PowerOff),
            ["/bin/oath", "set", "host:local", "power=halt"]
        );
        assert_eq!(
            oath_apply_args(),
            &["/bin/oath", "apply", "--confirm", "host:local"]
        );
    }
}
