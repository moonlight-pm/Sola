**Date:** 2026-08-05  
**Status:** target (freeze)  
**Implementation:** partial  
**Dogfood:** QEMU — splash → wizard → erase vdb → `vm run` → loginless Sola  
**Gaps:**
- No ISO output yet (harness is qcow + second disk)  
- Timezone auto-detect not done (fixed America/Denver for now)  
- Installer/desktop polish  
- Shape 1 release tarball URL 404  
**As-built:** [../capabilities.md](../capabilities.md) · [../architecture.md](../architecture.md)

# Distribution images — target design

## Product experience (locked)

```text
Power on (installer media)
  → [Firmware: OEM/OVMF — outside product control]
  → Quiet Linux boot: dark field + five-petal flower + activity animation
       (no kernel/systemd/getty text spam)
  → Graceful handoff → installer UI (kit, kiosk)
  → Username + disk
  → Apply (partition, install, create user, autologin)
  → Reboot (same quiet branded splash)
  → Straight into Sola as that user (loginless)
```

### Boot silence (locked)

- **No** diagnostic wall of text on the **graphical** path (no dmesg /
  systemd / getty essays over the splash). Engineering logs may still
  stream on serial (host terminal with QEMU mon:stdio).
- **Plymouth** from early initrd, with **simpledrm** +  
  `plymouth.ignore-serial-consoles` so a serial console does **not**
  suppress the graphical splash.
- **Boot mark:** the five-petal flower is an **alpha mask**; paint is a soft
  **cyan ripple expanding from the hub** (brand accent on graphite teal),
  looping ≈ 2 s. Theme: `sola` (`nix/image/plymouth/`).
- **Firmware (OVMF/OEM)** still owns the first moments before the kernel;
  we do not reflash arbitrary board ROMs. Goal: short firmware flash →
  brand splash for OS load → installer.
- Transition into the installer (or Sola) should feel continuous: splash
  quits as the graphical session takes the display — not a flash of TTY.

### Username lifecycle (locked)

| Phase | Who is logged in? | Username? |
|-------|-------------------|-----------|
| Live installer media | Disposable install seat (system account) | **Not** the end user — wizard runs without personal login |
| Wizard step | — | User **picks** the account name for the installed system |
| After apply + reboot | That account, **autologin** | Yes — required so Sola has a home/user; no password gate |

So: yes, a username is required before Sola on the **installed** machine —
and that is exactly what the wizard collects. You do **not** need a
username to *start* the installer itself.

Post-install is **loginless** for now: getty/greeter are not the product
path; the session starts Sola as the chosen user.

### Configuration (v1 — locked)

| Topic | Policy |
|-------|--------|
| Language | **US English only** (no picker) |
| Keyboard | **Mac layout only** (`us` + Mac variant / xkb as used on dogfood) — no picker |
| Timezone | **Interim: US/Mountain** (`America/Denver`) fixed until auto-detect lands; target remains auto-detect (network/IP) with silent fallback — no picker in v1 |
| User | **Username only** — no display name, **no password** in installer |
| Login | **Autologin** that user; they may set a password later on their own |
| Hostname | Default **`sola`** — not in wizard; changeable later |
| Disk | **Yes** — whole-disk install path (erase target and install); advanced dual-boot later |

### Post-install session (locked)

- **Straight to desktop** — no greeter, no password gate for v1  
- User runs as the username chosen at install  

### Branding

- Mark: five-petal flower (`crates/sola-assets/icons/sola/flower.svg`)  
- Early boot: Plymouth spinner + flower logo (see `nix/image/quiet-boot.nix`)  
- Installer chrome: graphite / design-language aligned, flower present  
- Raster logo derivation: `nix/image/plymouth/` (from the same SVG)  

---

## Shapes

| Shape | Audience | Mechanism | Status |
|-------|----------|-----------|--------|
| **1 — Colleague install** | Existing NixOS host | Tarball + `services.sola.enable` | Partial (release refresh needed) |
| **2 — Engineering harness** | Devs | Preinstalled qcow2 + `cargo make vm` | Scaffold works; **not** the product boot story |
| **3 — Installer media (product)** | Fresh machine | **ISO** (primary aim): live env → wizard → disk install | Target |

**ISO is the product goal.** A temporary first-boot OOBE on the qcow harness
is allowed only to dogfood splash + wizard + apply logic before the live ISO
path is wired — not a shipping mode.

---

## Installer wizard (v1 screens)

Keep it short and polished:

1. **Welcome** — flower + short line; Continue  
2. **Username** — single field; validate Unix username rules  
3. **Disk** — list disks; confirm “Erase *disk* and install Sola”  
4. **Installing…** — progress (no log dump by default)  
5. **Done** — Reboot  

No language, keyboard, timezone, password, hostname, or “full name” steps.

### Apply pipeline (conceptual)

On confirm:

1. Partition target (simple whole-disk: ESP + root; expand later if needed)  
2. Install NixOS system closure + Sola package/module  
3. Write user (no password / empty + autologin), hostname `sola`  
4. Locale `en_US.UTF-8`, Mac keyboard, timezone from detection  
5. Enable Sola session autostart for that user  
6. Reboot into installed system → Sola  

Exact disk layout and nixos-install vs custom image apply are implementation
details; they must match `services.sola` runtime requirements.

---

## Artifact layout (non-doc)

| Path | Role |
|------|------|
| `nix/module.nix` | Runtime module (installed system + live env deps) |
| `nix/sola.nix` | Package from release tarball |
| `nix/image/` | Image / ISO Nix expressions, Plymouth theme, live config |
| `nix/image/sola-from-stage.nix` | Local stage package for harness builds |
| `crates/sola-install` (planned) | Kit-native installer UI + apply orchestration |
| `var/images/` | Local products only (gitignored) |
| `cargo make vm …` / later `cargo make iso …` | xtask — not ad-hoc `scripts/` |

---

## Engineering harness (current qcow)

Exists to prove packaging + QEMU. **Do not polish getty/issue as product.**
Evolve toward:

- Optional: same installer binary in a “install to this disk” test mode inside
  QEMU (virtual second disk), or  
- Prefer: build ISO early and QEMU-boot the ISO with a blank target disk  

---

## Operator success criteria (product v1)

1. `cargo make iso build` (name TBD) produces a bootable ISO  
2. QEMU: boot ISO, complete wizard (username + erase disk), reboot  
3. Installed system starts **Sola desktop** as the chosen user without login  
4. Keyboard is Mac-US; language US English; hostname `sola`  
5. Brand splash shows the five-petal mark before the wizard  

### Non-goals (v1)

- Multi-language / multi-layout  
- Password or full-name collection  
- Greeter / multi-user login UX  
- Dual-boot / partial-disk advanced partitioning  
- Public update channel  
- Cross-distro (non-NixOS) media  

---

## Decision record

| Date | Decision |
|------|----------|
| 2026-08-05 | ISO is primary product path; OOBE-on-qcow only as optional harness |
| 2026-08-05 | Wizard: username + disk only; US English + Mac keyboard fixed |
| 2026-08-05 | Timezone auto-detect; hostname default `sola` |
| 2026-08-05 | No password; autologin; straight to desktop (loginless) |
| 2026-08-05 | Brand: five-petal flower through splash + installer |
| 2026-08-06 | Splash animation: clockwise cyan shade gradient on petals (flower is spinner) |
| 2026-08-06 | Wizard username prefill `sola` (selected for replace-on-type) |
| 2026-08-06 | QEMU e2e: vdb apply + `vm run` boots installed when present |
| 2026-08-06 | Timezone interim fixed to US/Mountain (`America/Denver`) until auto-detect |
