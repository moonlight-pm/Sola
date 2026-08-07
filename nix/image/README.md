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
cargo make iso build         # stage + nix build installer ISO → var/images/sola.iso
cargo make iso run           # QEMU: ISO + blank target disk
```

Binaries always come from **`target/release`** (this tree), never `/opt/sola/bin`.
`vm` / `iso` commands do **not** invoke cargo.

**Qcow flow:** `vm install` → erase **vdb** → quit → `vm run` boots installed.  
**ISO flow:** `iso build` → `iso run` → erase blank disk → reboot to disk (`vm run`).

Flake outputs:

- `nixosConfigurations.sola-vm` / `packages.sola-vm-qcow2` — live installer qcow
- `nixosConfigurations.sola-iso` / `packages.sola-iso` — installer ISO
- `nixosConfigurations.sola-installed` — target system written by apply

Local products live under `var/images/` (gitignored).

### Plymouth splash (no image build)

```sh
nix/image/plymouth/preview.sh          # ~20s → /tmp/ply-preview/index.html + preview.gif
# optional: preview.sh 32 240          # fewer / smaller frames
```
