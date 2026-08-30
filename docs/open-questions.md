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

### D3 — Which call-plane methods need a human confirm (P1)

**Context:** `sola-call` is desk-equivalent privilege (local-user `0600` socket, same as the bus). General agent control will want a confirm gate for some methods (close app, input, later Workspaces spawn). Cousin of **D1**.

**Ask:**

1. Which owners/methods require a prompt in v1+?  
2. Who owns the prompt (shell, a dedicated surface, the calling agent)?  
3. How does this interact with D1 multi-agent attach?

**Until decided:** do not invent confirm policy. Every live method is as privileged as the socket.

**Related:** [call-plane freeze](specs/2026-08-13-sola-call-plane-design.md); `call` capability.

---

## Open technical questions

### T1 — Agent pin UI surface

Pin data exists in overlay (`pinned`); bulk-delete respects pins; toggle UI was
removed. Is double-click rename + future context menu enough, or should pin
return to the sidebar row?

**Default until decided:** leave pins data-compatible; no new chrome without
product ask. See agent UI backlog.

### T2 — CEF default vs WPE — **closed (2026-08-11)**

WPE content-plane dogfood failed (`naturalethic/browser`). Product browser is
**CEF-only** in single crate `sola-browser`. Reopen only if a future Mesa/dma-buf
comparison warrants a second engine.

### D4 — sola-workspaces product forks (P2)

**Context:** Freeze is in; persist + spawn landed. Call plane owns fail-if-down
(was old D3.3). Remaining fork is Claude hooks.

**Ask:**

1. ~~Display name / window title~~ **decided 2026-08-14 / amended 2026-08-18:** crate / app id `sola-workspaces`; window **Workspaces**; owner `workspaces` (`solactl workspaces`); tmux `sola-ws` / `sws-`; config `~/.config/sola/workspaces/`.  
2. ~~Default worktree base~~ **decided 2026-08-13:** `<project-root>/.worktrees/<name>`.  
3. ~~If CLI runs and the app is down~~ **decided via call plane:** fail, do not launch.  
4. Claude in v1 — hook installer, or presence-only until Grok hooks are solid?

**Until decided:** use freeze **Interim** table for (4). Do not invent
Claude hook policy.

**Related:** `workspaces` capability;
[`specs/2026-08-13-sola-agent-terminal-design.md`](specs/2026-08-13-sola-agent-terminal-design.md).

---

### D5 — HTML/CSS kit vs iced (P2)

**Context:** HTML/CSS kit in worktree `kit-retarget`. Iced/`sola-kit`
remains the shipped kit. Idea:
[`ideas/2026-08-24-html-css-kit.md`](ideas/2026-08-24-html-css-kit.md).

**Answered 2026-08-25:** spike examples, sctk, JS punted, no canary overwrite.

**Amended 2026-08-29:**

1. Dead probes **removed** (`sola-blitz-spike`, `sola-html-spike`).  
2. Dual-kit era: HTML kit is a workspace member (wgpu 27). Storybook
   stays `sola-kit-spike`. First app twin is **`sola-settings-lab`**
   (`app_id` `sola-settings-lab`, title `Settings (lab)`).  
3. Install still skips `*-spike` / `*-lab` — run from `target/release/`.
   Never overwrite iced `sola-settings`.  
4. One bus `Topic::Theme` (`sola_core::theme::Theme`). HTML kit binds
   via `Theme::to_css` / CSS vars. No protocol version field.  
5. JS/DOM still punted. Window host remains sctk + calloop.

**Still open:** freeze / retarget iced. Do not replace `sola-kit` until
that is an explicit decision.

**Related:** idea above; `kit-retarget` worktree.

---

## Decision log

| Date | ID | Decision | Where recorded |
|------|-----|----------|----------------|
| 2026-08-29 | D5 amend | Drop blitz/html-spike. Dual-kit: workspace HTML kit + `sola-settings-lab`. Same `Topic::Theme`. Install skips `*-lab`. | CURRENT, idea, capabilities, architecture |
| 2026-08-25 | D5 | Spike examples (not canary install). First crate `sola-kit-spike`. Distinct `app_id` `sola-kit-spike` / title `Kit (spike)`. No install, no merge. sctk not winit. JS punted. | CURRENT, idea, capabilities, architecture |
| 2026-08-18 | workspaces | Per-project startup script after sibling spawn. Project menu + `project.startup`. Env: `PROJECT` / `WORKTREE` / `NAME`. | CURRENT, PRODUCT, CLI freeze, manual |
| 2026-08-18 | workspaces CLI | Face is `solactl workspaces` (owner renamed from `ws`). First-class. New verbs: `project.add`, `project.startup`, `workspace.select`, `workspace.set`, `workspace.exec`, `pane.wait`, `whoami`. Spawn `--branch` / `--base-branch` / `--title`. `--prompt-file`; richer list/spawn payloads; Grok-leaf targeting; parent from `$SOLA_PANE_ID`. Confirm still **D3**. | [CLI freeze](specs/2026-08-18-workspaces-cli-design.md), CURRENT, capabilities, manual/solactl |
| 2026-08-18 | D4.1 amend | Call owner is `workspaces` (`solactl workspaces …`). Tmux `sola-ws` / `sws-` unchanged. | CURRENT + freeze + PRODUCT |
| 2026-08-18 | workspaces | ⌘W closes the focused pane. Drop Project is menu-only. Kit splits (⌘⇧↓ / ⌘⇧→); leaf rows only after a split; last pane close keeps the workspace (Start new shell); a split leaf that exits retracts; hover does not spawn. Quiet `×N` only on a Grok leaf. | CURRENT, DESIGN, freeze header, capabilities |
| 2026-08-15 | Browser tab groups | In-strip folders (spaces later); groups at top, loose run at bottom; menu + drag join/leave; New group is menu-only; collapse keeps the page; empty dissolves; kit context menu | [freeze](specs/2026-08-15-sola-browser-tab-groups-design.md), CURRENT, capabilities |
| 2026-08-15 | Browser instance | One iced chrome via `chrome.sock`; second process hands off (does not reap live helpers). Helper death respawns + restores tabs. | CURRENT, capabilities, architecture, manual/sola-browser |
| 2026-08-15 | Browser passkey | `get()` intercept in every frame; same-site duplicate/retry coalesced (Gemini Exchange 2FA was failing the page before pick); `create()` vault confirm + persist (new login or attach) | CURRENT, capabilities, manual/sola-browser |
| 2026-08-15 | Kit storybook | Always update the matching storybook page in the same change; do not ask | `.grok/rules/kit-storybook-pages.md`, AGENTS |
| 2026-08-14 | D4.1 | Product is `sola-workspaces`. Owner `ws` (`solactl ws …`). Tmux `sola-ws` / `sws-`. Config `~/.config/sola/workspaces/`. | CURRENT + freeze + PRODUCT |
| 2026-08-14 | workspaces | No `sat` binary. Face is `solactl ws …` only. | CURRENT + freeze |
| 2026-08-14 | Browser downloads | Auto-save `~/Downloads`; toolbar icon with progress; click-to-open panel; persist completed in `shared/downloads.json`; cancel + open + remove-row (no Finder) | [freeze](specs/2026-08-14-sola-browser-downloads-design.md), CURRENT, capabilities, manual |
| 2026-08-14 | Call plane | Third plane `sola-call`; fail if owner down; `solactl` face is `compositor`/`session` (not `call 'sig'`); advertise for unknown apps; MCP later; confirm is **D3** | [freeze](specs/2026-08-13-sola-call-plane-design.md), CURRENT, architecture, capabilities |
| 2026-08-13 | workspaces | Promoted idea → freeze. Spawn sibling is v1; design law; not `sola-agent`. Product forks now **D4** (D3 taken by call plane). | freeze + CURRENT + D4 |
| 2026-08-13 | D4.2 | Worktrees live in `<project-root>/.worktrees/<name>`. Not `~/orca/workspaces/…`, not sibling-of-main. | freeze + CURRENT + PRODUCT |
| 2026-08-13 | workspaces | **Grok is the first-class CLI** — implement and test Grok first. Claude remains D4 (presence-only until Grok hooks are solid). | freeze + CURRENT + design law |
| 2026-08-13 | Unified sidebar | Terminal density Large; keep `Row` name + redefault to etch; browser divider via `SidebarPanel::resizable_with`; settings/mail/preview lose selection-teal | unified-sidebar freeze + plan, CURRENT, capabilities |
| 2026-08-13 | Browser OSR | IME + Shift+wheel + `<select>` PET_POPUP dogfooded; passkey **registration** deferred until needed | CURRENT, capabilities, manual |
| 2026-08-13 | Browser vault | Create login: save Bitwarden cipher first, then fill; always available on unlocked card; last username + generated password + bare apex URL | create-login freeze, CURRENT, capabilities, manual |
| 2026-08-12 | Browser persist | YouTube login survives full quit; ARGB→BGRA swizzle confirmed (no red wash) | CURRENT, capabilities |
| 2026-08-12 | Browser vault | Passkey **get** dogfooded (Google): intercept → picker → assert; clean web `clientDataJSON`; wire field `clientDataJSON` (not camelCase) | CURRENT, capabilities, manual/sola-browser |
| 2026-08-12 | Browser install | Prefer `cargo make install browser --release` for Bitwarden KDF; restore `--release` on install | sola-make, CURRENT, manual |
| 2026-08-12 | D8 / Browser | Profiles menubar (switch/create/rename/delete); switch re-exec; CEF under `profiles/<uuid>/cef/` | profiles freeze, CURRENT, capabilities, architecture, manual/sola-browser |
| 2026-08-12 | Browser windows | One iced chrome window (`sola-browser`). Instant profile switch via headless per-profile CEF helpers, not extra Wayland windows / unique app_ids. | CURRENT, capabilities, architecture, manual |
| 2026-08-11 | T2 / Browser | CEF-only single crate `sola-browser`; WPE multi-crate path retired after failed dogfood | CURRENT, architecture, capabilities, AGENTS |
| 2026-08-06 | dist | Distribution branch merged to master; qcow e2e OK; ISO e2e still open; interim TZ US/Mountain | freeze + plan + CURRENT |
| 2026-08-05 | — | Progress documentation practice adopted for Sola | CURRENT, progress-model, AGENTS |
| 2026-08-05 | dist | ISO primary; wizard = username + disk only; US EN + Mac keyboard fixed; hostname `sola`; no password; loginless → Sola; flower brand splash | [distribution-image freeze](specs/2026-08-05-distribution-image-design.md) |
| (earlier) | UI stack | Iced + sola-kit; WebView apocrypha | AGENTS, CURRENT locks |
| (earlier) | Browser | WPE primary, CEF parallel *(superseded 2026-08-11)* | — |
| (earlier) | Agent backend | Shared Grok leader only | AGENTS, CURRENT locks |
