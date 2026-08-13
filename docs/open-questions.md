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

## Open technical questions

### T1 — Agent pin UI surface

Pin data exists in overlay (`pinned`); bulk-delete respects pins; toggle UI was
removed. Is double-click rename + future context menu enough, or should pin
return to the sidebar row?

**Default until decided:** leave pins data-compatible; no new chrome without
product ask. See agent UI backlog.

### T2 — CEF default vs WPE

WPE is primary; CEF is parallel. When (if ever) flip defaults for specific
sites?

**Default until decided:** WPE remains default dispatcher path.

### D3 — sola-agent-terminal product forks (P2)

**Context:** Freeze is in; Grok hooks + tmux reattach smoked. These were left
interim so we did not invent policy. Do not treat the interims as locked.

**Ask:**

1. Display name / window title (`Workspaces` is interim)?  
2. Default worktree base — this desk’s `~/orca/workspaces/<Project>/`, sibling
   of main, or always ask?  
3. If `sat` runs and the app is down — fail, or launch the Wayland window?  
4. Claude in v1 — hook installer, or presence-only until Grok hooks are solid?

**Until decided:** use freeze **Interim** table only. Do not persist worktree
paths or `sat` auto-launch.

**Related:** `agent-terminal` capability;
[`specs/2026-08-13-sola-agent-terminal-design.md`](specs/2026-08-13-sola-agent-terminal-design.md).

---

## Decision log

| Date | ID | Decision | Where recorded |
|------|-----|----------|----------------|
| 2026-08-13 | agent-terminal | Promoted idea → freeze. Spawn sibling is v1; design law; not `sola-agent`. D3 forks still open. | freeze + CURRENT + D3 |
| 2026-08-13 | agent-terminal | **Grok is the first-class CLI** — implement and test Grok first. Claude remains D3 (presence-only until Grok hooks are solid). | freeze + CURRENT + design law |
| 2026-08-06 | dist | Distribution branch merged to master; qcow e2e OK; ISO e2e still open; interim TZ US/Mountain | freeze + plan + CURRENT |
| 2026-08-05 | — | Progress documentation practice adopted for Sola | CURRENT, progress-model, AGENTS |
| 2026-08-05 | dist | ISO primary; wizard = username + disk only; US EN + Mac keyboard fixed; hostname `sola`; no password; loginless → Sola; flower brand splash | [distribution-image freeze](specs/2026-08-05-distribution-image-design.md) |
| (earlier) | UI stack | Iced + sola-kit; WebView apocrypha | AGENTS, CURRENT locks |
| (earlier) | Browser | WPE primary, CEF parallel | AGENTS, architecture |
| (earlier) | Agent backend | Shared Grok leader only | AGENTS, CURRENT locks |
