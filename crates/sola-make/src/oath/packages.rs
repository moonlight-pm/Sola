//! The packages Sola ships on Oath, and what each one carries.
//!
//! Sola owns this list: Oath's desk profile only names the packages and
//! runs their programs as `svc:sola-*`.

pub struct Package {
    /// Package name; the catalog id is `pkg:<name>`.
    pub name: &'static str,
    /// Binaries in `bin/`. Each is also the crate that builds it.
    pub bins: &'static [&'static str],
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
            "sola-workspaces",
            "solactl",
            "sola-oath",
            "sola-kvm",
        ],
        needs: &[
            "gcc",
            "wayland",
            "libxkbcommon",
            "mesa",
            "tmux",
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
        needs: &["sola", "cef", "libpulse"],
        share: false,
        cef: true,
    },
    Package {
        name: "sola-design",
        bins: &["sola-design"],
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
