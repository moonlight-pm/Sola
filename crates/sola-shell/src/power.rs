//! Machine power from the flower menu (reboot / poweroff).
//!
//! Asks logind via `loginctl` / `systemctl`. Sola has no polkit agent, so
//! the call is non-interactive (`sudo -n` as a last try). A local seat
//! session is usually already allowed; the NixOS module also grants wheel
//! reboot/power-off so a missing agent cannot hang the shell.

use std::process::Command;

/// Flower-menu action id: reboot the machine (not Restart Shell).
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

    pub fn fail_toast(self) -> &'static str {
        match self {
            Self::Reboot => "Could not restart the computer",
            Self::PowerOff => "Could not shut down",
        }
    }
}

/// Request reboot or poweroff. Returns `Err` with a toast string if every
/// attempt failed. Does not emit `Topic::Shutdown` — systemd will SIGTERM
/// sola as part of the reboot, which is the same graceful path as Quit Sola.
pub fn request(kind: Kind) -> Result<(), &'static str> {
    let verb = kind.verb();
    tracing::info!(verb, "machine power requested via menu");
    for mut cmd in commands(verb) {
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

fn commands(verb: &str) -> Vec<Command> {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn verbs_match_logind() {
        assert_eq!(Kind::Reboot.verb(), "reboot");
        assert_eq!(Kind::PowerOff.verb(), "poweroff");
    }

    #[test]
    fn command_list_tries_loginctl_then_systemctl() {
        let cmds = commands("reboot");
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
}
