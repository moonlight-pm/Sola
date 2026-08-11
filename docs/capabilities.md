# Capabilities — as-built progress

**Purpose:** What exists vs what remains for each product capability.  
Target design: [`specs/`](specs/). Session priority: root
[`CURRENT.md`](../CURRENT.md). Product docs: [`manual/`](manual/) — **shipped
only**.

**Update this file in the same change** whenever status or gaps change.  
See [`progress-model.md`](progress-model.md).

**Status vocabulary:** `shipped` · `partial` · `spec’d` · `planned` · `idea`

**As of:** 2026-08-09 (FFM: title-only Windows skip + focus/composition dedup)

**Manual column:** `yes` = may document as fact · `partial` = limited honest
docs · `no` = do not present as product · `n/a` = engineering-only.

---

## Platform core

| ID | Capability | Status | Spec / plan | Dogfood | Gaps | Manual |
|----|------------|--------|-------------|---------|------|--------|
| supervisor | Process manager launches/restarts components | shipped | architecture | local TTY | Policy for crash loops / backoff polish | partial |
| bus | Unix-socket event bus + stickies | shipped | [persistent bus](specs/2026-04-24-persistent-bus-design.md) | local | Sticky surface still expanding; see bus freezes | partial |
| bus-reconnect | Apps survive bus restart | shipped | seamless restart freezes | local (menubar framed after restart) | Broader app-menu re-publish edge cases | no |
| river-bridge | sola-river ↔ River Wayland | shipped | [river design](specs/2026-04-16-sola-river-design.md) | local | Layer-shell / exclusive focus quirks; skip re-`focus_window` when already focused (FFM flash mitigation) | partial |
| session-mgr | sola-session spawn/close/reap | shipped | [session](specs/2026-04-17-sola-session-design.md) | local | Persistence depth varies by app | no |
| theme-bus | Topic::Theme palette + fonts | shipped | [sidebar/theme](specs/2026-05-07-sidebar-and-theme-protocol-design.md) | local | Full theme editor UX incomplete | partial |
| kit | sola-kit components + storybook | partial | [kit](specs/2026-04-30-sola-kit-design.md), graphite DS | storybook + apps | Storybook pages lag Open Design; not every control OD-parity | no |
| install-make | cargo make build/install to /opt/sola | shipped | — | local | Install requires human permission | yes |
| fonts-dist | System fonts only; distribution notes | shipped | [manual/distribution](manual/distribution.md) | local | Optional families must be installed by hand | yes |
| dist-shape1 | NixOS module + release tarball install | partial | [INSTALL.md](../INSTALL.md), [freeze](specs/2026-08-05-distribution-image-design.md) | colleagues historically | Published tarball URL **404** for v0.1.1; needs release refresh; not a fresh-machine path | partial |
| dist-vm-image | QEMU qcow harness (`cargo make vm`) | shipped | [freeze](specs/2026-08-05-distribution-image-design.md), [plan](plans/2026-08-05-distribution-qemu-image-plan.md) | QEMU install+boot target | Engineering only — not product media; stage always `target/release` | no |
| dist-installer | Flower splash + kit wizard + disk apply → loginless Sola | partial | [freeze](specs/2026-08-05-distribution-image-design.md), [plan](plans/2026-08-05-distribution-qemu-image-plan.md) | QEMU **vdb** e2e on master | **Gaps:** ISO e2e dogfood; TZ auto-detect (interim US/Mountain); polish; no `docs/manual/` ISO guide yet | no |
| dist-iso | Installer ISO build/run (`cargo make iso`) | partial | [freeze](specs/2026-08-05-distribution-image-design.md), [plan](plans/2026-08-05-distribution-qemu-image-plan.md) | build/run scaffold | Same live stack as qcow; multi‑GiB; **Gaps:** signed-off ISO→erase→reboot→Sola pass | no |

---

## Shell & windowing

| ID | Capability | Status | Spec / plan | Dogfood | Gaps | Manual |
|----|------------|--------|-------------|---------|------|--------|
| shell-menubar | Top menubar, menus, clock/stats | shipped | [shell iced](specs/2026-05-22-sola-shell-iced-port-design.md) | local | Visual polish vs macOS roadmap | no |
| shell-launcher | App launcher | shipped | shell design | local | Search ranking / recents polish | no |
| shell-switcher | MRU switcher | shipped | [switcher](specs/2026-04-09-switcher-design.md) | local | Switcher post-confirm FFM holdoff unmerged | no |
| shell-ffm | Focus-follows-mouse (dwell, no raise) | shipped | river + shell iced | local | Title-only Windows + Composition/chord dedup on `naturalethic/focus-flashing` (hygiene). Reported “Orca panel flash” reclassified to Orca/Grok pane UI, not shell | no |
| shell-zoning | Zone assignments | partial | zones + floating freezes | local | Opt-in snaps + restore; unassigned windows no longer force full frame | no |
| shell-float | Floating windows | partial | [floating](specs/2026-06-24-floating-windows-design.md) | local | **Default for unassigned windows** (app size + `WindowFloating`); Meta-drag shipped; kit CSD (`FloatState` + `floating_frame`) on monitor, settings, preview, mail, agent, terminal, kit storybook, browser | partial |
| shell-opening-toast | “Opening …” menubar toast | shipped | — | local | — | no |
| shell-custom | Shell chrome tokens | partial | [shell customization](specs/2026-06-06-shell-customization-design.md) | storybook Shell page | Not all chrome uses tokens | no |
| app-hidden | Composition hide for apps (`Topic::AppHidden`) + menubar restore chips | partial | — | code; install dogfood pending | **Gaps:** only app_id key (Steam id variants); no animation; switcher still lists hidden | partial |

---

## First-party apps

| ID | Capability | Status | Spec / plan | Dogfood | Gaps | Manual |
|----|------------|--------|-------------|---------|------|--------|
| arcade | sola-arcade Steam library + windowed gamescope nest | partial | [manual](manual/sola-arcade.md) | unit tests; library JSON cache + bg rescan; A–Z/Recent; ready-to-play filter; lazy viewport banners; Install on uninstalled; Stop-on-row; scroll preserve; nest steam exit; host label; zone/float + Cinema exit; `-S fit` | **Gaps:** some titles no host/crash; residual flicker; no `-e`; multi-store; never-played owned without local activity | yes |
| settings | Settings panel (theme, apps, mail cfg, …) | partial | [settings](specs/2026-04-19-sola-settings-design.md), apps list-detail | local | Applications UX still evolving; not all pages kit-parity | no |
| terminal | sola-terminal panes/tabs/tmux | partial | [terminal iced](specs/2026-06-03-sola-terminal-iced-port-plan.md) | local | Tab restore defers sticky `TerminalSession` until iced bus pump live (restart was empty UI with live tmux); pane UX depth; links polish | no |
| browser | Browser chrome + WPE engine (single crate) | partial | [hardening](plans/2026-08-09-sola-browser-hardening.md); [Bitwarden D7](specs/2026-08-10-sola-browser-bitwarden-design.md); [profiles D8](specs/2026-08-10-sola-browser-profiles-design.md); [content plane interim](specs/2026-08-11-sola-browser-content-plane-design.md); [**Option A stock Wayland**](specs/2026-08-11-sola-browser-stock-wayland-present-design.md); [lockstep plan](plans/2026-08-11-sola-browser-stock-wayland-lockstep-plan.md); [checkerboard](plans/2026-08-11-browser-checkerboard-deep-fix.md) | local; YT + `sola:scroll-stress`; NVIDIA | **Shipped subset:** multi-tab chrome, omnibox+Kagi, bus menus, float CSD, **interim content plane** (default), honest DPR, `sola:scroll-stress`, D8, Bitwarden, stop/history, copy/paste; **Wayland stock present spike** (`CONTENT=wayland`, dual-window). **Product target (D9/Option A):** stock WPE Wayland content + river lockstep under chrome hole. **Gaps:** A0 quality gate; A1–A4 lockstep cut over; daily-driver scroll; soft text; no profile switcher; cookie stickiness; vault session; downloads/history UI; NV12; B1/B4/B5; D3; no operator manual. CEF gone | no |
| agent | sola-agent ACP + Grok leader | partial | [acp runner](specs/2026-07-23-sola-agent-acp-runner-design.md), [ui backlog](specs/2026-07-23-sola-agent-ui-backlog.md) | local + leader | Pin UI missing; backlog A–I incomplete; history disk-only | no |
| mail | sola-mail kit client | partial | [mail kit](specs/2026-07-27-sola-mail-kit-design.md) | local IMAP | Rich-text link hits; multiline polish; no full offline | no |
| monitor | sola-monitor bus audit | partial | monitor kit port | local | UX depth | no |
| preview | sola-preview + selection capture | partial | [preview](specs/2026-08-04-sola-preview-and-selection-capture-design.md) | local | Zoom; image clipboard; solactl --region | no |
| kvm | sola-kvm Linux↔Mac | partial | [kvm](specs/2026-07-27-sola-kvm-design.md), clipboard | dual-host | Permanent input ACL; clipboard L2; warp cost; Mac scroll is CG velocity gain (not true HID accel) — tune on desk | partial |
| solactl | CLI helpers | partial | — | local | Region/screenshot flags incomplete | no |

---

## Cross-cutting / polish programs

| ID | Capability | Status | Spec / plan | Dogfood | Gaps | Manual |
|----|------------|--------|-------------|---------|------|--------|
| macos-look | macOS dark chrome density/materials | partial | [look roadmap](specs/2026-07-20-macos-look-and-feel-roadmap.md), [design language](manual/design-language.md) | local | Phase program incomplete | partial |
| design-language | Documented visual law | partial | [design-language](manual/design-language.md) | — | Not all apps comply | yes |
| screenshot | Capture + handoff to preview | partial | [screenshot plan](specs/2026-07-20-screenshot-capture-plan.md) | local | Region completeness; multi-output | no |
| doc-truth | Progress + manual match as-built | partial | [progress-model](progress-model.md) | — | Freeze headers not backfilled; vault stale | n/a |

---

## How to edit a row

1. Change **Status** only with evidence (code and/or dogfood).  
2. Keep **Gaps** non-empty when Status is `partial` or `spec’d`.  
3. Set **Manual** to `yes` only if `docs/manual/` may describe it as available.  
4. Point **Spec / plan** at freeze or plan.  
5. If this was the active slice, also update `CURRENT.md` **Now** / dogfood.  
6. Policy forks → [`open-questions.md`](open-questions.md#decision-points-ask-human).

---

## Recently notable (compact)

| Area | Notes |
|------|--------|
| **sola-browser** | 2026-08-11: **D9 Option A locked** (stock WPE Wayland + river lockstep); interim plane; A0–A4 plan; D8; Bitwarden |
| **sola-arcade** | 2026-08-08: banner list + nest; library cache + bg rescan; A–Z/Recent; ready-to-play filter; lazy viewport banners; Install on uninstalled; `--nested-steam` (no BPM); exit nested Steam on game quit; scroll preserve; Cinema exit; gamescope float 16:9 |
| **Distribution → master** | 2026-08-06: `sola-install`, Plymouth flower, qcow e2e loginless Sola, ISO scaffold |
| Float defaults | Unassigned windows default-float + kit CSD on first-party apps |
| Progress docs | 2026-08-05: CURRENT + capabilities + architecture spine |
| Bus restart | Menubar frame survives bus reconnect |
| Mail | Kit-native client on master |
