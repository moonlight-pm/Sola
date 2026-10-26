//! The packages Sola ships on Oath, and what each one carries.
//!
//! Sola owns this list: Oath's desk profile only names the packages and
//! runs their programs as `svc:sola-*`.

pub struct Package {
    /// Package name; the catalog id is `pkg:<name>`.
    pub name: &'static str,
    /// Binaries in `bin/`. Each is also the crate that builds it.
    pub bins: &'static [&'static str],
    /// Packs its programs build with, beyond the base set (their run-time
    /// needs come along); `plan:sola-build` takes every package's.
    pub build: &'static [&'static str],
    /// Packages this one loads or runs at run time, pinned at apply to
    /// the hash this machine has live (or the one just stored, for
    /// another package in this list).
    pub needs: &'static [&'static str],
    /// `share/`: Sola's icons, Lucide, Simple Icons, the McMojave cursors.
    pub share: bool,
    /// `cef` → `pkg:cef`'s CEF, which the CEF engine looks for beside
    /// its prefix.
    pub cef: bool,
}

impl Package {
    pub fn id(&self) -> String {
        format!("pkg:{}", self.name)
    }
}

/// The toolchain and GNU tools every Sola plan builds with: oath-make's
/// `BASE` without `patch`, which Sola's build never runs and the desk
/// profile does not ship.
pub const BASE: &[&str] = &[
    "gcc", "glibc", "bash", "coreutils", "sed", "grep", "gawk", "findutils", "diffutils",
    "tar", "gzip", "xz", "bzip2", "make",
];

/// In apply order: a package comes after the ones it needs.
pub const PACKAGES: &[Package] = &[
    Package {
        name: "sola",
        bins: &[
            "sola-bus",
            "sola-call",
            "sola-river",
            "sola-shell",
            "sola-session",
            "sola-terminal",
            "sola-projects",
            "sola-paint",
            "sola-calendar",
            "sola-settings",
            "solactl",
            "sola-oath",
            "sola-kvm",
        ],
        build: &["rust", "pkgconf", "libxkbcommon"],
        needs: &[
            "gcc",
            "wayland",
            "libxkbcommon",
            "mesa",
            "tmux",
            // wl-copy / wl-paste: Claude Code reads a pasted image with
            // them (the NixOS module ships wl-clipboard for the same).
            "wl-clipboard",
            "pipewire",
            "wireplumber",
            "fontconfig",
            "font-noto",
            "font-noto-cjk",
            "font-noto-emoji",
            "font-iosevka-term-slab",
        ],
        share: true,
        cef: false,
    },
    Package {
        name: "sola-browser",
        bins: &["sola-browser"],
        build: &["rust", "pkgconf", "cef"],
        needs: &["sola", "cef", "libpulse"],
        share: false,
        cef: true,
    },
    Package {
        name: "sola-design",
        bins: &["sola-design"],
        build: &["rust", "pkgconf", "cef"],
        needs: &["sola", "cef", "libpulse"],
        share: false,
        cef: true,
    },
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_package_comes_after_the_packages_it_needs() {
        for (i, pkg) in PACKAGES.iter().enumerate() {
            for need in pkg.needs {
                if let Some(j) = PACKAGES.iter().position(|p| p.name == *need) {
                    assert!(j < i, "{} needs {need}, listed later", pkg.name);
                }
            }
        }
    }
}
