# Open questions

Unresolved **design forks**. Not the implementation backlog (that lives in
[`roadmap.md`](roadmap.md) / [`capabilities.md`](capabilities.md) / plans).

Priority tags: **P0** blocks current work · **P1** near-term · **P2** later.

**Agents:** If work depends on a row under
[Decision points (ask human)](#decision-points-ask-human), **stop and ask**.
Do not invent product policy.

Progress model: [`progress-model.md`](progress-model.md).

---

## Decision points (ask human)

### D1 — Permission fan-out when multiple agents attach (P1)

**Context:** Grok leader / sola-agent and TUI (or multiple sola-agent windows)
can both request permission. Ask-mode UX and which client owns the prompt is
unclear.

**Ask:**

1. Single global permission UI vs per-client strips?  
2. Who wins if both attach in ask mode?  
3. Should sola-agent suppress auto-approve when an external TUI is attached?

**Until decided:** do not invent multi-client permission policy; keep existing
single-client auto-approve modes; surface conflicts as errors if observed.

**Related:** `agent` capability; agent ACP freezes.

---

### D2 — sola-kvm permanent input ACL (P1)

**Context:** Input device access currently needs per-boot `setfacl` (or similar).
Permanent udev/ACL is a host-policy choice with security trade-offs.

**Ask:**

1. Prefer udev rule installed with Sola, documented manual udev, or group
   membership?  
2. Acceptable blast radius (all input nodes vs tagged devices)?

**Until decided:** keep documented operator workaround; no silent broad ACL
install from agent sessions.

**Related:** `kvm` capability; [`manual/sola-kvm-operator.md`](manual/sola-kvm-operator.md).

---

### D3 — Browser: default link handler — **decided 2026-08-09**

**Decision:** Keep **Helium** as the system `http`/`https` default until
sola-browser is good enough to become the product default. sola-browser
stays **opt-in** (launcher / explicit open); do **not** subscribe to
`Topic::OpenUrl` or flip MIME defaults without a later explicit ship call.

**Related:** `browser` capability;
[`plans/2026-08-09-sola-browser-hardening.md`](plans/2026-08-09-sola-browser-hardening.md).

---

### D4 — Browser: dogfood MVP chrome scope — **decided 2026-08-09**

**Decision:** Next product bar (beyond pure engine/lifecycle) includes:

| In scope | Notes |
|----------|--------|
| **Stop loading** | Button and/or Escape; wire existing `NavCmd::Stop` |
| **Downloads** | Real download UX (not just “works somehow”) |
| **History + session restore** | Survive restart; open recent / restore session |
| **Bitwarden integration (extension)** | Password manager as first-class; treat as product requirement |
| **High polish + reliability** | Bar is daily-driver quality, not spike demos |

**Explicitly not decided as in-scope by this answer:** find-in-page, zoom,
bookmarks UI (history ≠ bookmarks), context menus, error pages, DevTools —
still backlog unless later promoted.

**Implication:** Bitwarden-as-extension is **not free on WPE** (Chromium
extension APIs are not WebKit/WPE). Approach is open → **D7**.

**Related:** hardening plan; `browser` capability.

---

### D7 — Browser: Bitwarden / extension approach — **decided 2026-08-09**

**Decision:** **First-party Bitwarden UX inside sola-browser** (option 1).

| Do | Don’t |
|----|--------|
| Vault + unlock + autofill **in** sola-browser | Chrome/Firefox store package |
| Bitwarden **SDK/API** (or equivalent client lib) | Separate user-run system service / desktop bridge |
| Page fill via WebKitWebExtension and/or content inject | WebExtensions host (Epiphany-class) for now |
| Sola chrome UI for password UX | Revisit CEF solely for extensions (unless D7 reopened) |

**Quality bar (D4):** extension-class polish/reliability, not a demo popup.

**Architecture lock (2026-08-10):** in-process
`bitwarden/sdk-internal` `PasswordManagerClient` inside `sola-browser`
(`src/vault/` + worker thread); **official Bitwarden cloud** for login
(no self-host field yet); fill via WebKit `evaluate_javascript` (no
WebExtensions host). **Do not shape architecture around license** —
ignore until public distribution. Full freeze:
[`specs/2026-08-10-sola-browser-bitwarden-design.md`](specs/2026-08-10-sola-browser-bitwarden-design.md).

**Still deferred product detail:** account model (personal vs org),
biometric unlock, TOTP, passkeys, offline vault path, autofill default
(offer vs auto), shortcut chord — open when implementing.

**Related:** D4; hardening plan research §; engine stays WPE.

---

### D5 — Browser: middle-click behavior (P1)

**Context:** iced path drops Middle button (`button_to_wpe` → `None`).
WebKit `decide-policy` still has middle-click → background tab logic that
never fires from iced events. Cmd/Ctrl-click still works via modifiers.

**Ask:** Should middle-click open a background tab (restore event path), or
stay ignored (delete dead policy branch)?

**Until decided:** leave code as-is; document as known gap **B5**.

---

### D6 — Browser: omnibox search provider (P2)

**Context:** Non-URL omnibox queries go to **Kagi** only (`util::resolve_query`).

**Ask:** Keep Kagi hardcoded, make configurable in settings, or switch default?

**Until decided:** keep Kagi; no settings surface.

---

### D8 — Browser: profile model — **decided 2026-08-10**

**Decision:** Multi-profile-ready layout **now**; one active profile; switcher later.

| Locked | Detail |
|--------|--------|
| Identity | UUID + friendly name (default **Primary**) |
| WebKit paths | `~/.local/share/sola/browser/profiles/<uuid>/` + matching cache |
| Registry | `~/.local/share/sola/browser/profiles.json` |
| Shared data | `~/.local/share/sola/browser/shared/` (history, downloads, …) |
| Config | `~/.config/sola/browser/` (e.g. `vault.json`) — **not** flat `browser-*.json` |
| Per profile | WebKit data/cache, open **tabs** (tabs = bookmarks) |
| Shared | prefs, history, downloads, vault chrome / autofill |
| First run | Create Primary; **no migration**; delete old flat trees + dead config |
| Switcher UI | Later |

**Freeze:**
[`specs/2026-08-10-sola-browser-profiles-design.md`](specs/2026-08-10-sola-browser-profiles-design.md).

**Related:** D4 history+restore; hardening P1.3; `browser` capability.

### D9 — Browser: present architecture — **decided 2026-08-11**

**Question:** Headless + content-plane hybrid vs stock WPE Wayland present for
daily-driver scroll quality?

**Decision:** **Option A locked** — product present =
**`WPEDisplayWayland` / `WPEViewWayland`** for page pixels; iced remains chrome;
**river lockstep sibling** under the content hole for one visual window unit.
Content plane preferred path remains **implemented interim**; not the quality
endgame. Phases A0–A4 in freeze + plan (A0 dual-window quality gate first).

**Freeze / plan:**
[`specs/2026-08-11-sola-browser-stock-wayland-present-design.md`](specs/2026-08-11-sola-browser-stock-wayland-present-design.md),
[`plans/2026-08-11-sola-browser-stock-wayland-lockstep-plan.md`](plans/2026-08-11-sola-browser-stock-wayland-lockstep-plan.md).

**Related:** content-plane freeze §4.2 elevated; D3/D4; `browser` capability.

---

## Open technical questions

### T1 — Agent pin UI surface

Pin data exists in overlay (`pinned`); bulk-delete respects pins; toggle UI was
removed. Is double-click rename + future context menu enough, or should pin
return to the sidebar row?

**Default until decided:** leave pins data-compatible; no new chrome without
product ask. See agent UI backlog.

---

## Decision log

| Date | ID | Decision | Where recorded |
|------|-----|----------|----------------|
| 2026-08-11 | D9 | **Present:** Option A — stock WPE Wayland content + river lockstep under iced chrome; plane interim; A0–A4 plan | stock-wayland freeze + plan, CURRENT locks, open-questions D9 |
| 2026-08-10 | D8 | **Profiles:** UUID + Primary; `profiles/<id>/` data+cache; registry + `shared/`; config under `~/.config/sola/browser/`; tabs per profile; history/prefs/downloads/vault shared; no migration | profiles design freeze, CURRENT, open-questions D8 |
| 2026-08-10 | D7 arch | **In-process** `sdk-internal` `PasswordManagerClient` (`src/vault/` + async worker); inject fill; self-host in MVP; **license out of architecture scope** until public dist | bitwarden design freeze, CURRENT, D7 |
| 2026-08-09 | D7 | **First-party Bitwarden UX** in sola-browser (SDK/API + in-process autofill); no Chrome store package; no system service; no WebExtensions host for now | open-questions D7, plan, CURRENT |
| 2026-08-09 | D4 | Browser MVP bar: **stop loading**, **downloads**, **history+restore**, **Bitwarden (extension-class)**, high polish; find/zoom/bookmarks/devtools not auto-included | open-questions D4, plan, CURRENT |
| 2026-08-09 | D3 | **Helium remains system default** until sola-browser is good enough to take over; OpenUrl/MIME stay Helium; browser opt-in only | open-questions D3, architecture, plan |
| 2026-08-09 | browser | **CEF removed**; WPE-only single crate; full review → hardening plan; D3–D6 opened | plan, capabilities, CURRENT, open-questions |
| 2026-08-09 | browser | **CEF removed**; WPE-only single crate `sola-browser` (folded wpe + core); archive tag `pre-cef-removal` | AGENTS, architecture, CURRENT locks |
| 2026-08-06 | dist | Distribution branch merged to master; qcow e2e OK; ISO e2e still open; interim TZ US/Mountain | freeze + plan + CURRENT |
| 2026-08-05 | — | Progress documentation practice adopted for Sola | CURRENT, progress-model, AGENTS |
| 2026-08-05 | dist | ISO primary; wizard = username + disk only; US EN + Mac keyboard fixed; hostname `sola`; no password; loginless → Sola; flower brand splash | [distribution-image freeze](specs/2026-08-05-distribution-image-design.md) |
| (earlier) | UI stack | Iced + sola-kit; WebView apocrypha | AGENTS, CURRENT locks |
| (earlier) | Browser | WPE primary, CEF parallel (superseded 2026-08-09) | AGENTS, architecture |
| (earlier) | Agent backend | Shared Grok leader only | AGENTS, CURRENT locks |
