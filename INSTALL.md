# Installing Sola on NixOS

This installs **prebuilt Sola binaries** plus all required runtime
configuration via a NixOS module. You will not compile anything.

**From source** (clone + `cargo make install`): [`CONTRIBUTING.md`](CONTRIBUTING.md).
That path sets `services.sola.installRelease = false` so the module does
not fetch this tarball.

**Current gap:** the published tarball URL for **v0.1.1** 404s
(`nix/release.nix`). `nixos-rebuild` with `services.sola.enable = true`
and the default `installRelease = true` will fail at fetch until a
maintainer recuts the release (`cargo make publish`). Until then, use
the CONTRIBUTING path.

## Requirements

- NixOS (the flake is built against `nixos-unstable`; `nixos-25.05`+
  should also work).
- A user account with sudo for `nixos-rebuild`.
- x86_64. (No ARM build yet.)
- A working GPU stack — Mesa-only (Intel/AMD) or NVIDIA proprietary.
  See "GPU notes" below.
- SSH access to the private repo: your SSH key registered with GitHub
  (`ssh -T git@github.com` should print "Hi <username>!"), and your
  GitHub user added as a collaborator on `moonlight-pm/Sola`. Nix's
  `git+ssh://` flake fetch uses the same SSH auth as `git clone`.

## Setup

### 1. Add Sola as a flake input

If your system uses a flake (`/etc/nixos/flake.nix`):

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
        { services.sola.enable = true; }
      ];
    };
  };
}
```

> **Do not add `inputs.sola.inputs.nixpkgs.follows = "nixpkgs";`.**
> Sola pins its own nixpkgs (via `flake.lock`) to a revision where
> the patched `pkgs.river` (0.4.5, the zig rewrite) builds cleanly.
> Overriding it with your nixpkgs can land you on a revision where
> `pkgs.river` has been renamed or doesn't have the right version, and
> the build will fail with a `'river' has been renamed to/replaced by`
> throw. The closure-size cost of two nixpkgs revisions is small.

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

```sh
which sola              # /run/current-system/sw/bin/sola
which solactl           # /run/current-system/sw/bin/solactl
ls /opt/sola/bin/       # full app set
```

`sola` itself is a process manager that takes over the display the
moment it starts — it does **not** accept `--version` or `--help` and
has no safe "smoke test" invocation from inside another session. The
above PATH/listing checks confirm the install without launching it.

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

- When `installRelease = true` (default): installs the Sola binaries
  from the GitHub tarball (`sola`, `solactl`, `sola-shell`,
  `sola-river`, `sola-kit`, `sola-settings`, `sola-monitor`,
  `sola-browser`, `sola-terminal`, `sola-mail`, …) under the Nix
  store and links them onto `PATH`; activation symlinks
  `/opt/sola/{bin,share}` at that package (leaves a real directory
  alone). When `installRelease = false`, skip the tarball and create
  real `/opt/sola/{bin,share}` directories for `cargo make install`
  — see CONTRIBUTING.md.
- Installs a **patched River** (River 0.4.5 with carried patches —
  see `nix/patches/`).
- Configures `programs.nix-ld` with the native libraries Chromium's
  `libcef.so` loads at runtime.
- Adds GStreamer plugins + `glib-networking` + their session env
  vars (historical WebKitGTK helpers; iced apps do not need WebKitGTK).
- Adds `xdg-utils` and `desktop-file-utils` for default-browser
  routing; **wayland** and **xwayland** so iced binaries can dlopen
  `libwayland-client` and River can find Xwayland.
- Enables `hardware.graphics`.
- Enables **BlueZ** (`hardware.bluetooth`, power-on-boot, experimental
  for battery). The menubar Bluetooth chip talks to `org.bluez`; it
  hides when no adapter is present. Do **not** add Blueman — Sola owns
  that popover.
- The menubar **volume** chip talks to PipeWire (`pw-dump` / `wpctl`).
  The module does **not** enable PipeWire; the host already must (same
  as media keys). The chip hides if WirePlumber is missing.
- Installs **Inter** and **JetBrains Mono** (`fonts.packages`) so
  UI/mono roles resolve (see `docs/manual/distribution.md`).
- Creates `/opt/sola/log` as a writable directory. Binaries hardcode
  `/opt/sola/{bin,share,log}`.

The full configuration is in `nix/module.nix`.

## Updating

When a new release is published:

```sh
nix flake update sola
sudo nixos-rebuild switch --flake /etc/nixos
```

Releases are tagged `vX.Y.Z` on the Sola repo. To pin to a specific
release (instead of tracking `master`):

```nix
sola.url = "git+ssh://git@github.com/moonlight-pm/Sola?ref=v0.1.0";
```

Update by bumping the `ref=` value and re-running the two commands
above.

## Troubleshooting

- **`error: hash mismatch in fixed-output derivation`** — your local
  flake input is pinned to a release whose tarball changed on the
  server. Run `nix flake update sola` to refresh.
- **GitHub release tarball 404** (v0.1.1 as of 2026-08) — Shape 1
  prebuilt install is blocked until `cargo make publish`. Compile from
  source instead: CONTRIBUTING.md (`installRelease = false`).
- **`sola` exits immediately with a Wayland-related error** — you
  are probably in another desktop session. Sola needs a bare TTY.
- **A `sola-kit` app shows a blank window** — GPU initialization
  failed. See "GPU notes" above and check
  `/opt/sola/log/sola-kit.log`.
- **`error: 'river' has been renamed to/replaced by 'river-classic'`**
  during `nixos-rebuild` — your flake has
  `inputs.sola.inputs.nixpkgs.follows = "nixpkgs"`. Remove that line
  (see the warning under "Setup"), then re-run
  `nix flake update sola && sudo nixos-rebuild switch --flake /etc/nixos`.
  Sola's pinned nixpkgs has the right `pkgs.river`; following yours
  can land on a transient revision where it's missing.

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
