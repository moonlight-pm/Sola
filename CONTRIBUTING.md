# Contributing to Sola

Dev-setup for compiling Sola from source on a **NixOS** box and iterating.
If you only want prebuilt binaries, see `INSTALL.md` — that path installs a
GitHub release tarball (currently **404** for v0.1.1; recut needed).

Sola is **NixOS-only**. Nix on another distro is not a supported host: every
workspace binary bakes RUNPATH to `/run/current-system/sw/lib` and
`/run/opengl-driver/lib`.

The binaries hardcode `/opt/sola/bin`, `/opt/sola/share`, and `/opt/sola/log`.
The CEF engine tree lives at `~/.cache/sola/cef-<version>/` (populated by
`cargo make install-cef`). `cargo make install` writes `/opt/sola/{bin,share}`
directly.

There is no GNU Makefile. `cargo make` is a Cargo alias for `sola-make`
(see `.cargo/config.toml`).

## Prerequisites

- **NixOS** x86_64 (`nixos-unstable` or `nixos-25.05`+).
- **Working GPU stack** — Mesa (Intel/AMD) or NVIDIA proprietary.
- **sudo** — first install creates `/opt/sola/` as root if needed.
- **Repo access** — SSH key on GitHub, collaborator on `moonlight-pm/Sola`.
- **Rust** — `rustc` **1.85+** / `cargo` (workspace edition 2024). rustup
  stable is fine. Skip the rustup snippet below if `rustc --version` already
  qualifies.

## Two layers

1. **Host (once)** — NixOS module: patched River 0.4.5, nix-ld for CEF,
   GPU, fonts, kvm udev. Does **not** have to install Sola binaries.
2. **Tree** — `cargo make install-cef` then `cargo make install` from a
   clone. This is what you re-run when you change code.

A clone plus `cargo make install` on a stock NixOS desktop, with no module,
will not produce a working session (no patched `river` on PATH, no nix-ld
set for `libcef.so`).

## 1. NixOS configuration

Add Sola as a flake input and enable the module **without** the release
tarball:

```nix
{
  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    sola.url = "git+ssh://git@github.com/moonlight-pm/Sola";
  };

  outputs = { self, nixpkgs, sola, ... }: {
    nixosConfigurations.<your-host> = nixpkgs.lib.nixosSystem {
      system = "x86_64-linux";
      modules = [
        ./configuration.nix
        sola.nixosModules.default
        {
          services.sola.enable = true;
          # Host runtime only. cargo make install owns /opt/sola.
          # Leave this true (INSTALL.md) only when the GitHub tarball exists.
          services.sola.installRelease = false;
        }
      ];
    };
  };
}
```

> **Do not add `inputs.sola.inputs.nixpkgs.follows = "nixpkgs"`.**
> Sola pins its own nixpkgs to a revision where `pkgs.river` (0.4.5, the
> zig rewrite) and the carried River/wlroots patches build cleanly. See
> `INSTALL.md` for the failure mode if you follow.

In `configuration.nix`, add **compile** tools the module does not ship
(the module already has `patchelf`, `wayland`, `libxkbcommon`, `xwayland`,
Inter, and JetBrains Mono):

```nix
environment.extraOutputsToInstall = [ "dev" "lib" ];
environment.sessionVariables.PKG_CONFIG_PATH =
  "/run/current-system/sw/lib/pkgconfig:/run/current-system/sw/share/pkgconfig";
environment.systemPackages = with pkgs; [
  pkg-config
  gcc
  git
  curl
];
```

`extraOutputsToInstall` + `PKG_CONFIG_PATH` is how smithay-client-toolkit
finds `xkbcommon.pc` on NixOS. Without it, the first iced crate build dies
in pkg-config.

If you do **not** already have a new enough `rustc`:

```nix
environment.systemPackages = with pkgs; [ rustup /* …plus the list above */ ];
```

```sh
rustup default stable
rustup component add rust-src   # optional; rust-analyzer into stdlib
```

Then:

```sh
sudo nixos-rebuild switch --flake /etc/nixos
```

Sanity checks after the rebuild:

```sh
river -version                  # 0.4.5 +xwayland
which river
ls /run/current-system/sw/share/nix-ld/lib | head
fc-list : family | grep -E '^Inter$|^JetBrains Mono$'
pkg-config --exists xkbcommon && echo xkbcommon-ok
```

`installRelease = false` creates real directories at `/opt/sola/{bin,share,log}`
and will replace leftover **symlinks** from a previous tarball install. It
does not delete a real directory that already has your builds.

If you previously enabled the module with `installRelease = true` (or the
old default) and `/opt/sola/bin` is still a store symlink after rebuild:

```sh
sudo rm -f /opt/sola/bin /opt/sola/share
sudo mkdir -p /opt/sola/bin /opt/sola/share /opt/sola/log
```

## 2. First-time build

```sh
git clone git@github.com:moonlight-pm/Sola
cd Sola
cargo make install-cef    # required: ~/.cache/sola/cef-<version>/  (~1.5GB, once)
cargo make install        # builds the workspace, copies to /opt/sola/bin
```

`cargo make install` already builds. You do not need a separate
`cargo make build` first.

**`install-cef` is not optional.** `sola-browser` and `sola-wrapper` `build.rs`
fail with “run `cargo make install-cef`” if `libcef.so` is missing. That
command downloads the public CEF tarball (~500MB compressed), extracts it,
runs `patchelf` so libcef’s RUNPATH hits nix-ld + `/run/opengl-driver/lib`,
and symlinks `Resources/*` next to `libcef.so`. Needs network, `curl`,
`tar`, and `patchelf`. That tarball has **no H.264/AAC** (AV1/VP9/Opus/MP3
only). Steam store DASH and typical MP4 need `scripts/cef-codecs/` (same
pin, hours). MPEG-LA if that `libcef.so` is redistributed.

The first `cargo make …` compiles `sola-make` itself; the first full
`install` is a long debug workspace build. `install` also runs
`cargo make assets sync` when icon/cursor packs are missing under
`/opt/sola/share/` (GitHub clones of Lucide, Simple Icons, McMojave).

Debug is the default. For release (smaller, slower to compile; strongly
preferred for sola-browser Bitwarden KDF):

```sh
cargo make install --release
# or one app:
cargo make install browser --release
```

## 3. Run it

Sola is a full desktop session — it owns the display and input. **Do not
run it from inside another desktop session.**

1. Log out of any graphical session.
2. Switch to a TTY (`Ctrl+Alt+F2` or similar).
3. Log in.
4. Launch the installed binary (not on `PATH` unless you add `/opt/sola/bin`):
   ```sh
   /opt/sola/bin/sola
   ```
   Debug logging to the log dir:
   ```sh
   RUST_LOG=debug /opt/sola/bin/sola 2>&1 | tee /opt/sola/log/sola.log
   ```
5. Quit via the menubar’s “Quit Sola”, or `pkill sola` from another TTY.

## 4. Day-to-day workflow

```sh
cargo make build                       # full workspace
cargo make build <crate>               # one crate (e.g. `cargo make build shell`)
cargo make install                     # install all rebuilt binaries
cargo make install <app>               # install one (short name: `shell` → sola-shell)
cargo make install <app> --watch       # watch + reinstall on change (frontend)
```

`install` writes `/opt/sola/bin`. Prefer a user-owned copy; falls back to
`sudo cp` when the tree is root-owned. The process manager watches that
directory and restarts a child whose binary changes — most apps do not
need a manual relaunch.

Replacing `sola` itself tears down the session (the installer asks). For
the process manager, the shell, or the compositor bridge, kill sola and
re-launch from the TTY.

## 5. Repository layout

Canonical map: `AGENTS.md` and `docs/architecture.md`. Headline pieces:

- `crates/sola/` — process manager (the binary you launch).
- `crates/sola-bus/` — IPC bus.
- `crates/sola-call/` — request/reply host (`solactl` talks here).
- `crates/sola-shell/` — menubar, launcher, switcher, zoning.
- `crates/sola-river/` — River ↔ bus bridge.
- `crates/sola-kit/` — Iced app kit + storybook.
- `crates/sola-*` — apps (browser, terminal, mail, workspaces, settings, …).
- `crates/sola-make/` — `cargo make …`.
- `crates/sola-assets/` — third-party icon/cursor pulls (fonts are **not**
  bundled; see `docs/manual/distribution.md`).
- `apocrypha/` — frozen WebView stack (reference only; not built).
- `docs/manual/` — operator docs for **shipped** behavior.
- `docs/specs/` — target freezes. `docs/plans/` — implementation checklists.
- `docs/vault/` — historical Obsidian notes; not living truth.

## 6. Debugging

- Logs: `/opt/sola/log/<process>.log` and `/opt/sola/log/sola.log`.
  `tail -F /opt/sola/log/sola.log` from a second TTY.
- `RUST_LOG=info,sola_kit=trace,cef=warn`.
- `solactl compositor windows` — running apps + window IDs.
- `solactl logs <app>` — tail one app.
- `solactl compositor screenshot` / `solactl session launch` talk to
  `sola-call` (not the bus). Owner down → fail.
- `solactl emit <Topic> '<json-payload>'` — bus poke from the CLI.
- River: `/opt/sola/log/river.log`.

CEF GPU init failure (`Unable to initialize SkSurface`): check
`__EGL_VENDOR_LIBRARY_DIRS` and `VK_ICD_FILENAMES`, and that
`hardware.graphics` populated `/run/opengl-driver/`. Deep dive:
`docs/vault/Distribution.md`.

### Common first-run failures

| Symptom | Likely cause |
|---|---|
| `error: CEF binary distribution not found` | Forgot `cargo make install-cef`. |
| `river not found in PATH` | Module not enabled / rebuild not switched. |
| `hash mismatch` / tarball 404 during `nixos-rebuild` | `installRelease` still true; v0.1.1 URL 404s. Set it false. |
| `'river' has been renamed to/replaced by 'river-classic'` | `inputs.sola.inputs.nixpkgs.follows = "nixpkgs"`. Remove it. |
| pkg-config / `xkbcommon` at compile | Missing `extraOutputsToInstall` or `PKG_CONFIG_PATH`. |
| Blank iced window, `Unable to initialize SkSurface` | GPU / nix-ld / CEF RUNPATH. See GPU notes in `INSTALL.md`. |
| sola exits immediately with a Wayland error | You launched from inside another desktop session. Use a bare TTY. |
| Ugly UI type | Inter / JetBrains Mono not in fontconfig; module should have installed them. |

## 7. Commit conventions

Match `git log --oneline -20`. Common prefixes: `feat`, `fix`, `refactor`,
`docs`, `test`, `chore`. Subject in imperative present (`add foo`, `fix bar`).
Body is the *why* and non-obvious gotchas.

Include a `Co-Authored-By` trailer when AI tools are part of the loop.

## 8. Cutting a release (maintainers)

`cargo make publish` bundles `/opt/sola/bin` + the CEF Release tree +
`/opt/sola/share`, pre-patches RUNPATHs, zstd-compresses, rewrites
`nix/release.nix`, commits, tags, pushes, and runs `gh release create`.
See `INSTALL.md` → “For maintainers”. That recut is what unblocks
`installRelease = true` / the colleague tarball path.

## 9. Getting help

- Issues: `https://github.com/moonlight-pm/Sola/issues`.
- Architecture: `docs/architecture.md`. Design freezes: `docs/specs/`.
- `docs/vault/` is history (WebView-era notes mixed in).
