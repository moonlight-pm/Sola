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

**Status: planned**

- Beyond local `/opt/sola` install: packaging, fonts story productization,
  update channel for the desktop  

**Remaining:** not scheduled; see [manual/distribution](manual/distribution.md)
for current operator truth only.

---

## Unplanned / parked

Capture under [`ideas/`](ideas/) until promoted. Do not treat as commitment.
