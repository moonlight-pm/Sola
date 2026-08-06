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
cargo make vm install        # wipe previous vdb install + boot live installer
cargo make vm run            # QEMU: live installer + existing/blank vdb
cargo make vm run --rebuild  # force disk-image rebuild from current target/release
SOLA_VM_BOOT=target cargo make vm run --no-build   # boot installed target only
```

Binaries always come from **`target/release`** (this tree), never `/opt/sola/bin`.
`vm run` / `vm build` / `vm install` do **not** invoke cargo.

**Install dogfood:** `cargo make vm install` wipes `sola-install-target.qcow2`
and boots the live image. Wizard: erase **vdb** → apply → Reboot, then host-side
`SOLA_VM_BOOT=target cargo make vm run --no-build`.

Flake outputs:

- `nixosConfigurations.sola-vm` — the system config
- `packages.sola-vm-qcow2` — `config.system.build.image` (qcow2)

Local products live under `var/images/` (gitignored). Store paths are never
committed.
