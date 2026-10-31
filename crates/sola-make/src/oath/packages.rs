//! The packages Sola ships on Oath, and what each one carries.
//!
//! Sola owns this list: Oath's desk profile only names the packages and
//! runs their programs as `svc:sola-*`.
//!
//! `pkg:sola` is the desk core: the programs whose restart takes the
//! session down (bus, call, river, shell, session, kvm, lock), `solactl`,
//! the Oath catalog app, and `share/` (icons, cursors — `SOLA_SHARE_DIR`
//! and `XCURSOR_PATH` name `/oath/store/pkg/sola/share`). Each app is its
//! own `pkg:sola-<app>`, so an overlay that touches only an app moves only
//! it and restarts nothing else (the `oath-packages` skill; split
//! 2026-10-31). Apps need what they load, never the core: a need pins an
//! exact hash, and every core overlay would rewrite every app's needs.

pub struct Package {
    /// Package name; the catalog id is `pkg:<name>`.
    pub name: &'static str,
    /// Binaries in `bin/`. Each is also the crate that builds it, but
    /// for the few a crate ships beside its own ([`crate_of`]).
    pub bins: &'static [&'static str],
    /// Packs its programs build with, beyond the base set (their run-time
    /// needs come along); `plan:sola-build` takes every package's.
    pub build: &'static [&'static str],
    /// Packages this one loads or runs at run time beyond [`UI`] (when
    /// `ui`), pinned at apply to the hash this machine has live (or the
    /// one just stored, for another package in this list).
    pub needs: &'static [&'static str],
    /// An iced app: it needs [`UI`] too.
    pub ui: bool,
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

    /// Everything it needs at run time: [`UI`] for an app, then its own.
    pub fn runtime_needs(&self) -> Vec<&'static str> {
        let mut all: Vec<&'static str> = if self.ui { UI.to_vec() } else { Vec::new() };
        for n in self.needs {
            if !all.contains(n) {
                all.push(n);
            }
        }
        all
    }
}

/// What every iced program loads: the C runtime, Wayland, keys, GL, fonts.
pub const UI: &[&str] = &[
    "gcc",
    "wayland",
    "libxkbcommon",
    "mesa",
    "fontconfig",
    "font-noto",
    "font-noto-cjk",
    "font-noto-emoji",
    "font-iosevka-term-slab",
];

/// An app's own package: its programs, [`UI`] and `needs`.
const fn app(name: &'static str, bins: &'static [&'static str], needs: &'static [&'static str]) -> Package {
    Package {
        name,
        bins,
        build: &["rust", "pkgconf", "libxkbcommon"],
        needs,
        ui: true,
        share: false,
        cef: false,
    }
}

/// The crate that builds `bin`: its own name, but for a second binary
/// in another crate (`sola-botsd` is `sola-bots`'s daemon).
pub fn crate_of(bin: &str) -> &str {
    match bin {
        "sola-botsd" => "sola-bots",
        other => other,
    }
}

/// `cargo build -p` arguments for `bins`: each crate once.
pub fn build_crates<'a>(bins: impl IntoIterator<Item = &'a str>) -> Vec<&'a str> {
    let mut crates: Vec<&str> = Vec::new();
    for bin in bins {
        let krate = crate_of(bin);
        if !crates.contains(&krate) {
            crates.push(krate);
        }
    }
    crates
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
            "sola-kvm",
            "sola-lock",
            "solactl",
            "sola-oath",
        ],
        build: &["rust", "pkgconf", "libxkbcommon"],
        needs: &[
            "tmux",
            // wl-copy / wl-paste: the KVM's clipboard.
            "wl-clipboard",
            "pipewire",
            "wireplumber",
        ],
        ui: true,
        share: true,
        cef: false,
    },
    // wl-copy / wl-paste: Claude Code in a terminal reads a pasted image
    // with them.
    app("sola-terminal", &["sola-terminal"], &["tmux", "wl-clipboard"]),
    app("sola-projects", &["sola-projects"], &["tmux", "wl-clipboard"]),
    app("sola-paint", &["sola-paint"], &[]),
    app("sola-preview", &["sola-preview"], &[]),
    app("sola-scope", &["sola-scope"], &[]),
    app("sola-monitor", &["sola-monitor"], &[]),
    app("sola-calendar", &["sola-calendar"], &[]),
    app("sola-mail", &["sola-mail"], &[]),
    app("sola-settings", &["sola-settings"], &[]),
    app("sola-bots", &["sola-bots", "sola-botsd"], &[]),
    Package {
        name: "sola-browser",
        bins: &["sola-browser"],
        build: &["rust", "pkgconf", "cef"],
        needs: &["cef", "libpulse"],
        ui: true,
        share: false,
        cef: true,
    },
    Package {
        name: "sola-design",
        bins: &["sola-design"],
        build: &["rust", "pkgconf", "cef"],
        needs: &["cef", "libpulse"],
        ui: true,
        share: false,
        cef: true,
    },
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_second_binary_builds_from_its_crate_once() {
        assert_eq!(
            build_crates(["sola-bots", "sola-botsd", "sola-mail"]),
            ["sola-bots", "sola-mail"]
        );
    }

    #[test]
    fn every_program_ships_once() {
        let mut bins: Vec<&str> = PACKAGES.iter().flat_map(|p| p.bins.iter().copied()).collect();
        let n = bins.len();
        bins.sort();
        bins.dedup();
        assert_eq!(bins.len(), n, "a program in two packages");
    }

    #[test]
    fn apps_need_what_they_load_not_the_core() {
        for pkg in PACKAGES.iter().skip(1) {
            assert!(!pkg.runtime_needs().contains(&"sola"), "{} needs the core", pkg.name);
            assert!(pkg.runtime_needs().contains(&"wayland"), "{}", pkg.name);
        }
        let core = &PACKAGES[0];
        assert_eq!(core.name, "sola");
        assert!(core.share && core.bins.contains(&"sola-river"));
        assert!(!core.bins.contains(&"sola-mail"), "apps are out of the core");
    }

    #[test]
    fn a_package_comes_after_the_packages_it_needs() {
        for (i, pkg) in PACKAGES.iter().enumerate() {
            for need in pkg.runtime_needs().iter() {
                if let Some(j) = PACKAGES.iter().position(|p| p.name == *need) {
                    assert!(j < i, "{} needs {need}, listed later", pkg.name);
                }
            }
        }
    }
}
