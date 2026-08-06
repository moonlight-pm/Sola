# Roadmap

**Program horizon** — coarse phase status over months.  
Day-to-day maturity: [`capabilities.md`](capabilities.md).  
Session priority: root [`CURRENT.md`](../CURRENT.md).  
How layers fit: [`progress-model.md`](progress-model.md).

| Status | Meaning |
|--------|---------|
| **done** | Good enough to build on; polish ok |
| **partial** | Scaffold or subset shipped; important gaps remain |
| **active** | Current focus (`CURRENT.md`) |
| **next** | Queued after active |
| **planned** | Intended; not started |
| **unplanned** | Not scheduled; capture only |

Update phase status only when a **phase-level** flip happens. Prefer capability
rows for feature-level progress.

---

## Phase 0 — Process core

**Status: done**

- Multi-process supervisor (`sola`), bus, river bridge, session manager  
- Sticky bus state, restart resilience baseline  

**Remaining:** optional crash-policy polish (capability rows).

---

## Phase 1 — Iced shell + kit

**Status: done (v0)**

- Shell iced port (menubar, launcher, switcher)  
- sola-kit + theme protocol + storybook  
- Settings / monitor kit ports  

**Remaining:** Open Design parity across storybook pages; shell token adoption.

---

## Phase 2 — First-party apps (kit)

**Status: partial**

- Terminal iced, browser WPE/CEF, agent ACP, mail kit, preview, kvm  

**Remaining:** agent UI backlog; browser chrome completeness; mail polish;
kvm input ACL permanence; preview zoom/clipboard.

---

## Phase 3 — Visual system (macOS dark)

**Status: partial / next**

- Design language + graphite DS work  
- Screenshot / visual regression assets  

**Remaining:** execute look-and-feel roadmap phases with visual stops
([roadmap freeze](specs/2026-07-20-macos-look-and-feel-roadmap.md)).

---

## Phase 4 — Distribution & packaging

**Status: partial / active**

- Shape 1: NixOS module + release tarball (`INSTALL.md`, `services.sola`)  
- Engineering harness: qcow2 + `cargo make vm`  
- **Product aim:** installer ISO — flower splash, username+disk wizard,
  loginless Sola ([freeze](specs/2026-08-05-distribution-image-design.md))  

**Remaining:** real disk apply; ISO pipeline; Shape 1 release refresh.
Harness + flower splash + dry-run wizard exist. Fonts story operator-documented
under [manual/distribution](manual/distribution.md).

---

## Unplanned / parked

Capture under [`ideas/`](ideas/) until promoted. Do not treat as commitment.
