# Plan — Distribution installer (ISO-first)

**Status:** open (partial — branch merged to master 2026-08-06)  
**Freeze:** [../specs/2026-08-05-distribution-image-design.md](../specs/2026-08-05-distribution-image-design.md)  
**Focus:** root [`CURRENT.md`](../../CURRENT.md)

Product goal: **bootable ISO** → brand splash → kit wizard (username + disk) →
install → reboot → **loginless Sola desktop**.

Qcow harness dogfoods splash + apply on master; remaining bar is **ISO e2e**.
Do not invest in getty polish.

## Phase 0 — Harness (done)

- [x] Artifact homes: `nix/image/`, `var/images/`, `cargo make vm`  
- [x] qcow2 build + QEMU boot to shell  
- [x] Progress docs for scaffold  

## Phase 1 — Installer app + policy wiring

- [x] `crates/sola-install`: iced + sola-kit wizard  
  - Welcome → Username → Disk → Progress → Done / Failed  
  - Dry-run when no install image or demo disk; real apply on live media  
- [x] Fixed policy reflected in UI copy: US EN, Mac keyboard, no password  
- [x] Timezone interim: US/Mountain (`America/Denver`) on installed system  
- [ ] Timezone auto-detect helper (network; fallback documented)  
- [x] Loginless desktop session on installed system (`installed-session.nix`)  
- [x] Apply backend: `sola-install-apply` (partition + nixos-install + user)

## Phase 2 — Brand splash + quiet boot

- [x] Quiet kernel/params module (`nix/image/quiet-boot.nix`)  
- [x] Flower Plymouth theme (`nix/image/plymouth/`) — clockwise cyan gradient  
- [x] Spinner = flower petals only (no stock throbber; ~300 ms/step)  
- [x] Installer kiosk (`installer-session.nix` + cage + systemd)  
- [x] QEMU dogfood: flower splash visible + uniform clockwise walk; kiosk up  
- [x] Same splash modules on installed system (`installed-system` + quiet-boot)  
- [x] ISO live env reuses the same modules (`live-common` + iso.nix)

## Phase 3 — Live ISO + disk install

- [x] Installer live profile (installer-session + install-tools)  
- [x] Whole-disk partition + install pipeline (ESP + root labels)  
- [x] QEMU harness: second virtio disk (`sola-install-target.qcow2` → vdb)  
- [x] QEMU dogfood: erase vdb → loginless Sola; `vm run` boots installed  
- [x] `cargo make vm install` wipes previous target + boots live installer  
- [x] Flake `nixosConfigurations.sola-iso` + `packages.sola-iso`  
- [x] Shared live modules (`live-common`, quiet-boot, installer-session, install-tools)  
- [x] `cargo make iso build` / `iso run` (QEMU: `-cdrom` + blank virtio disk)  
- [ ] QEMU dogfood **signed off:** ISO → erase disk → reboot target → Sola  


### ISO approach (agreed direction)

Reuse the **same** live stack as the qcow harness:

| Piece | Role |
|-------|------|
| `quiet-boot.nix` + Plymouth | Brand splash |
| `installer-session.nix` | cage + sola-install kiosk |
| `install-tools.nix` + `sola-installed` toplevel | Offline apply (already in store) |
| `iso-image.nix` (nixpkgs) | Produce `system.build.isoImage` |
| Stage via `SOLA_VM_STAGE` | Same patchelf’d `target/release` binaries |

**Not** a separate OOBE — product path is ISO-first. Qcow remains engineering harness.

**Dogfood command shape:**

```sh
cargo build --release
cargo make iso build          # stage + nix build ISO → var/images/sola.iso
cargo make iso run            # QEMU: ISO + blank disk; after install reboot disk
```

**Size note:** ISO embeds the full `sola-installed` closure (same as qcow installer). Expect multi‑GiB; fine for dogfood.

## Phase 4 — Polish + ship hygiene

- [x] Failed install screen (message + retry)  
- [ ] Shape 1 release tarball refresh (404 today)  
- [ ] Manual: operator install doc when ISO is dogfoodable (`docs/manual/`)  

## Commands (today)

```sh
cargo build --release             # you own the Rust build
cargo make vm build               # stage target/release → nix qcow2 (no cargo)
cargo make vm install             # wipe target + boot live installer
cargo make vm run                 # installed if present, else installer
cargo make iso build              # stage + ISO → var/images/sola.iso
cargo make iso run                # QEMU: ISO + blank target
```

`vm` / `iso` never run cargo. Stage only from `target/release`
(not `/opt/sola/bin`).
