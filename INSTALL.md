# Installing Sola on NixOS

This installs **prebuilt Sola binaries** plus all required runtime
configuration via a NixOS module. You will not compile anything.

## Requirements

- NixOS (the flake is built against `nixos-unstable`; `nixos-25.05`+
  should also work).
- A user account with sudo for `nixos-rebuild`.
- x86_64. (No ARM build yet.)
- A working GPU stack — Mesa-only (Intel/AMD) or NVIDIA proprietary.
  See "GPU notes" below.

## Setup

### 1. Add Sola as a flake input

If your system uses a flake (`/etc/nixos/flake.nix`):

```nix
{
  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    sola = {
      url = "git+ssh://git@github.com/moonlight-pm/Sola";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };

  outputs = { self, nixpkgs, sola, ... }: {
    nixosConfigurations.<your-host> = nixpkgs.lib.nixosSystem {
      system = "x86_64-linux";
      modules = [
        ./configuration.nix
        sola.nixosModules.default
        { services.sola.enable = true; }
      ];
    };
  };
}
```

If your system uses the classic `/etc/nixos/configuration.nix` (no
flake), the simplest path is to add a flake — even a one-file
`/etc/nixos/flake.nix` that just wraps your existing
`configuration.nix` is enough.

### 2. Rebuild

```sh
sudo nixos-rebuild switch --flake /etc/nixos
```

The first time this runs, Nix will fetch the Sola release tarball
from GitHub Releases (a few hundred MB; takes a minute or two).

### 3. Verify

You should now have `sola` on your PATH:

```sh
which sola
sola --version
```

## Running Sola

Sola is a full desktop session — it owns the display, input, and
window management. **Do not run it from inside another desktop
session.** Launch it from a bare TTY:

1. Log out of any graphical session.
2. Switch to a TTY (Ctrl+Alt+F2 or similar).
3. Log in.
4. Run:
   ```sh
   sola
   ```
   Or, with debug logging:
   ```sh
   RUST_LOG=debug sola 2>&1 | tee /tmp/sola.log
   ```

Quit with the menu's "Quit Sola" entry, or `pkill sola` from another
TTY.

## GPU notes

Sola uses CEF (Chromium) for several apps, which needs a vendor EGL
ICD discoverable via `/run/opengl-driver/`:

- **Intel / AMD**: works out of the box once
  `hardware.graphics.enable = true` is set (the module does this).
- **NVIDIA proprietary**: configure `hardware.nvidia` in your
  configuration.nix as usual. The CEF GPU subprocess will use
  `/run/opengl-driver/share/vulkan/icd.d/nvidia_icd.x86_64.json`
  automatically.
- **NVIDIA Open**: should work the same way as Mesa.

If `sola-kit`-based apps (settings, monitor) come up blank or crash
on launch, this is usually the issue. Check
`/opt/sola/log/sola-kit.log` for `Unable to initialize SkSurface` or
similar.

## What the module installs

`services.sola.enable = true` does the following:

- Installs the Sola binaries (`sola`, `solactl`, `sola-shell`,
  `sola-river`, `sola-kit`, `sola-settings`, `sola-monitor`,
  `sola-browser`, `sola-terminal`, `sola-mail`, …) under the Nix
  store and links them onto `PATH`.
- Installs a **patched River** (River 0.4.5 with our carried Xwayland
  destroy-state fix — see `nix/patches/`).
- Configures `programs.nix-ld` with the 26 native libraries
  Chromium's `libcef.so` loads at runtime.
- Adds GStreamer plugins + `glib-networking` + their session env
  vars so the legacy WebKit-based apps (browser, terminal, shell,
  mail) can play media and use HTTPS.
- Adds `xdg-utils` and `desktop-file-utils` for default-browser
  routing.
- Enables `hardware.graphics`.

The full configuration is in `nix/module.nix`.

## Updating

When a new release is published:

```sh
nix flake update
sudo nixos-rebuild switch --flake /etc/nixos
```

## Troubleshooting

- **`error: hash mismatch in fixed-output derivation`** — your local
  flake input is pinned to a release whose tarball changed on the
  server. Run `nix flake update sola` to refresh.
- **`sola` exits immediately with a Wayland-related error** — you
  are probably in another desktop session. Sola needs a bare TTY.
- **A `sola-kit` app shows a blank window** — GPU initialization
  failed. See "GPU notes" above and check
  `/opt/sola/log/sola-kit.log`.
- **Steam games don't work / crash River** — only run Steam via
  `gamescope -- steam`. Direct `steam` hits River bugs the patch
  doesn't cover.

For deeper issues, see `docs/vault/Distribution.md` in the repo —
it documents every runtime requirement and its rationale.

## For maintainers: cutting a release

```sh
cargo make publish              # auto-bumps the patch of the latest vX.Y.Z tag
cargo make publish 0.2.0        # explicit version (e.g. for minor/major bump)
```

The command:
1. Refuses to run with a dirty working tree.
2. `cargo build --release` (with `strip = "debuginfo"` from root
   `Cargo.toml`, so binaries shrink ~70% while keeping function-level
   stack traces).
3. Stages `target/release/*` + the patched CEF Release tree from
   `~/.cache/sola/cef-<version>/Release/`.
4. Pre-patches the CEF-linking binaries' RUNPATH to `/opt/sola/cef`
   (so the bundle works outside Nix too — the derivation re-rpaths
   to the store path on install).
5. tar + zstd-19 compresses to `sola-<version>-linux-x86_64.tar.zst`.
6. Computes the SRI hash and rewrites `nix/release.nix`.
7. Commits, tags `v<version>`, pushes to `github`, runs
   `gh release create` with the tarball attached.

Requires `gh` authenticated (`gh auth status`) and `patchelf` + `zstd`
on PATH.
