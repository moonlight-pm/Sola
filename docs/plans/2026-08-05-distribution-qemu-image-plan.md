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

- [x] `crates/sola-install`: iced + sola-kit wizard (dry-run apply)  
  - Welcome (flower) → Username → Disk confirm → Progress → Done  
  - Dogfood: `cargo make build sola-install` then run binary under Sola  
- [x] Fixed policy reflected in UI copy: US EN, Mac keyboard, no password  
- [ ] Timezone auto-detect helper (network; fallback documented)  
- [ ] Autologin + “start sola” session unit/script for installed system  
- [x] Apply backend stub (dry-run progress only — no disk writes)

## Phase 2 — Brand splash + quiet boot

- [x] Quiet kernel/params module (`nix/image/quiet-boot.nix`)  
- [x] Flower Plymouth theme (`nix/image/plymouth/`) — clockwise cyan gradient  
- [x] Spinner = flower petals only (no stock throbber; ~300 ms/step)  
- [x] Installer kiosk (`installer-session.nix` + cage + systemd)  
- [x] QEMU dogfood: flower splash visible + uniform clockwise walk; kiosk up  
- [ ] Same splash on installed system boot after real apply  
- [ ] ISO live env uses the same modules

## Phase 3 — Live ISO + disk install

- [ ] `nix/image` live configuration (installer session, not full desktop)  
- [ ] Flake output e.g. `packages.sola-iso` / `cargo make iso build`  
- [ ] Whole-disk partition + install pipeline (ESP + root)  
- [ ] QEMU dogfood: ISO + blank target disk → install → reboot → Sola  
- [ ] Optional: reuse wizard against qcow harness with second virtio disk  

## Phase 4 — Polish + ship hygiene

- [ ] Error states (no disk, install fail) without raw log walls  
- [ ] Shape 1 release tarball refresh (404 today)  
- [ ] Manual: operator install doc when ISO is dogfoodable (`docs/manual/`)  

## Commands (today)

```sh
cargo build --release             # you own the Rust build
cargo make vm build               # stage target/release → nix qcow2 (no cargo)
cargo make vm run                 # QEMU; may rebuild *image* if missing/stale
cargo make vm run --rebuild       # force disk-image rebuild
cargo make vm run --no-build      # never rebuild image (fail if missing)
```

`vm run` / `vm build` never run cargo. Stage only from `target/release`
(not `/opt/sola/bin`). Image rebuild triggers: missing qcow, stale vs
`nix/image/*` or `target/release/sola-install`.

## Commands (target)

```sh
cargo make iso build
cargo make iso run          # QEMU: ISO + empty disk
```
