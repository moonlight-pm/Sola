# Plan — Distribution installer (ISO-first)

**Status:** open  
**Freeze:** [../specs/2026-08-05-distribution-image-design.md](../specs/2026-08-05-distribution-image-design.md)  
**Focus:** root [`CURRENT.md`](../../CURRENT.md)

Product goal: **bootable ISO** → brand splash → kit wizard (username + disk) →
install → reboot → **loginless Sola desktop**.

The existing qcow harness stays for packaging/QEMU plumbing; do not invest in
getty polish.

## Phase 0 — Harness (done)

- [x] Artifact homes: `nix/image/`, `var/images/`, `cargo make vm`  
- [x] qcow2 build + QEMU boot to shell  
- [x] Progress docs for scaffold  

## Phase 1 — Installer app + policy wiring

- [x] `crates/sola-install`: iced + sola-kit wizard  
  - Welcome → Username → Disk → Progress → Done / Failed  
  - Dry-run when no install image or demo disk; real apply on live media  
- [x] Fixed policy reflected in UI copy: US EN, Mac keyboard, no password  
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
- [ ] ISO live env uses the same modules

## Phase 3 — Live ISO + disk install

- [x] Installer live profile (installer-session + install-tools)  
- [x] Whole-disk partition + install pipeline (ESP + root labels)  
- [x] QEMU harness: second virtio disk (`sola-install-target.qcow2` → vdb)  
- [x] QEMU dogfood: erase vdb → `SOLA_VM_BOOT=target` → loginless Sola  
- [x] `cargo make vm install` wipes previous target + boots live installer  
- [ ] Flake output e.g. `packages.sola-iso` / `cargo make iso build`  
- [ ] QEMU dogfood: ISO + blank target disk → install → reboot → Sola  

## Phase 4 — Polish + ship hygiene

- [x] Failed install screen (message + retry)  
- [ ] Shape 1 release tarball refresh (404 today)  
- [ ] Manual: operator install doc when ISO is dogfoodable (`docs/manual/`)  

## Commands (today)

```sh
cargo build --release             # you own the Rust build
cargo make vm build               # stage target/release → nix qcow2 (no cargo)
cargo make vm install             # wipe previous vdb + boot live installer
cargo make vm run                 # QEMU: live + existing/blank vdb
cargo make vm run --rebuild       # force disk-image rebuild
cargo make vm run --no-build      # never rebuild image (fail if missing)
SOLA_VM_BOOT=target cargo make vm run --no-build   # boot installed disk only
```

`vm run` / `vm build` never run cargo. Stage only from `target/release`
(not `/opt/sola/bin`). Image rebuild triggers: missing qcow, stale vs
`nix/image/*` or `target/release/sola-install`.

## Commands (target)

```sh
cargo make iso build
cargo make iso run          # QEMU: ISO + empty disk
```
