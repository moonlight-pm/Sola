# `nix/image/` — install-media / VM image sources

Non-doc distribution artifacts for **bootable images** (Goal A: preinstalled
qcow2 for QEMU). Shape 1 colleague install (`nix/module.nix` + release tarball)
stays next door under `nix/`.

| File | Role |
|------|------|
| `configuration.nix` | NixOS appliance profile (qemu-guest, user, fonts, disk-image) |
| `sola-from-stage.nix` | Package Sola from a `cargo make vm build` stage tree |
| `README.md` | This map |

## Build / run (via xtask, not ad-hoc scripts)

```sh
cargo build --release        # you own the Rust build
cargo make vm build          # stage target/release → nix qcow2 (no cargo)
cargo make vm install        # wipe target + boot live installer
cargo make vm run            # installed system if present, else installer
```

Binaries always come from **`target/release`** (this tree), never `/opt/sola/bin`.
`vm run` / `vm build` / `vm install` do **not** invoke cargo.

**Flow:** `vm install` → wizard erases **vdb** → finish install → quit QEMU →
`vm run` boots the installed disk automatically.

Flake outputs:

- `nixosConfigurations.sola-vm` — the system config
- `packages.sola-vm-qcow2` — `config.system.build.image` (qcow2)

Local products live under `var/images/` (gitignored). Store paths are never
committed.
