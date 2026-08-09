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

### D3 — Browser: default link handler (P0 for browser polish)

**Context:** `sola_core::open_url` / shell route `http`/`https` to **Helium**.
`sola-browser` intentionally does **not** subscribe to `Topic::OpenUrl`
(`integration.rs`). Launcher still has an explicit Browser entry.

**Ask:**

1. Keep Helium as system default and sola-browser opt-in only?  
2. Or make sola-browser the default handler (subscribe OpenUrl + desktop MIME)?  
3. Or dual: sola-browser default, Helium escape hatch?

**Until decided:** do not change OpenUrl routing or MIME defaults.

**Related:** `browser` capability;
[`plans/2026-08-09-sola-browser-hardening.md`](plans/2026-08-09-sola-browser-hardening.md).

---

### D4 — Browser: dogfood MVP chrome scope (P0 for browser polish)

**Context:** Chrome is tabs + omnibox + nav + Edit menu only. Missing:
find-in-page, zoom, stop, downloads, bookmarks, history, session restore,
devtools, context menus, error pages.

**Ask:** For the next hardening slice, which features are **in scope** vs
explicitly later? (Pick a minimal dogfood bar.)

**Until decided:** do not invent bookmark/history storage or download UX;
prefer engine lifecycle / perf fixes that need no product policy.

**Related:** same hardening plan § P1.

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
| 2026-08-09 | browser | **CEF removed**; WPE-only single crate; full review → hardening plan; D3–D6 opened | plan, capabilities, CURRENT, open-questions |
| 2026-08-09 | browser | **CEF removed**; WPE-only single crate `sola-browser` (folded wpe + core); archive tag `pre-cef-removal` | AGENTS, architecture, CURRENT locks |
| 2026-08-06 | dist | Distribution branch merged to master; qcow e2e OK; ISO e2e still open; interim TZ US/Mountain | freeze + plan + CURRENT |
| 2026-08-05 | — | Progress documentation practice adopted for Sola | CURRENT, progress-model, AGENTS |
| 2026-08-05 | dist | ISO primary; wizard = username + disk only; US EN + Mac keyboard fixed; hostname `sola`; no password; loginless → Sola; flower brand splash | [distribution-image freeze](specs/2026-08-05-distribution-image-design.md) |
| (earlier) | UI stack | Iced + sola-kit; WebView apocrypha | AGENTS, CURRENT locks |
| (earlier) | Browser | WPE primary, CEF parallel (superseded 2026-08-09) | AGENTS, architecture |
| (earlier) | Agent backend | Shared Grok leader only | AGENTS, CURRENT locks |
