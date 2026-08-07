# CURRENT — living product focus

**Only session handoff.** Update when priority, next slice, or known runtime
state changes. Read after `AGENTS.md`. Full model:
[`docs/progress-model.md`](docs/progress-model.md). Capability maturity:
[`docs/capabilities.md`](docs/capabilities.md).

**Decisions agents must ask about:**  
[`docs/open-questions.md` § Decision points](docs/open-questions.md#decision-points-ask-human).
Do not invent product policy.

**As of:** 2026-08-06 (distribution — QEMU install → loginless Sola dogfood OK)

---

## Now

1. **Distribution installer (active)** — branch `naturalethic/distribution`.  
   - **Product:** ISO → flower splash → wizard (username + disk) → install →
     reboot → **loginless Sola** (US English, Mac keyboard, hostname `sola`,
     no password, timezone auto).  
   - Freeze: [`docs/specs/2026-08-05-distribution-image-design.md`](docs/specs/2026-08-05-distribution-image-design.md)  
   - Plan: [`docs/plans/2026-08-05-distribution-qemu-image-plan.md`](docs/plans/2026-08-05-distribution-qemu-image-plan.md)  
   - **Harness:** `cargo make vm install` (wipe + installer);
     `cargo make vm run` boots **installed** if present, else installer.
     Stage from **`target/release` only** (no cargo inside vm).  
   - **Dogfood OK:** splash → wizard → erase vdb → reboot target → loginless
     Sola (`runuser` session; username prefill `sola`).  
   - **Next:** polish (installer UX, splash handoff, errors); then ISO path.  
2. **Progress docs** — keep this file + `docs/capabilities.md` honest when
   shipping; do not reintroduce a second handoff.  
3. **Follow-ups (unordered backlog, not priority):**  
   - Optional per-app default sizes in `window_settings`; dogfood float chrome  
   - Permanent `/dev/input` ACL or udev for sola-kvm (avoid per-boot setfacl)  
   - Permission fan-out UX when TUI + sola-agent both attached (ask mode) — **D1**  
   - Remaining worktree hygiene: `libei-portal` archive/cleanup  
   - sola-preview: zoom, image clipboard copy, `solactl --region`  
   - sola-mail: inline rich-text link hits (vs chips), multiline polish  
   - Clipboard follow-ups (native pasteboard, images, larger caps) —
     [`docs/specs/2026-07-30-sola-kvm-clipboard-design.md`](docs/specs/2026-07-30-sola-kvm-clipboard-design.md)  
   - Optional: further Mac warp path cost for sola-kvm  
   - Optional: true HID scroll path (virtual HID) if CG velocity gain not enough  
   - Shape 1 release tarball refresh (published v0.1.1 URL currently 404)

**Explicit holds:** none.

**Always allowed:** pure safety/doc fixes; tests; progress-doc maintenance;
warning cleanups; worktree hygiene the user asks for.

---

## Known dogfood state

| | **primary (local)** |
|--|---------------------|
| Role | Daily dogfood desktop |
| Launch | Physical TTY → `/opt/sola/bin/sola` (no display manager) |
| Install root | `/opt/sola/bin/`, logs `/opt/sola/log/` |
| Bus state | `~/.config/sola/state.toml` (sticky topics) |
| Compositor | River via `sola-river` |
| UI stack | Iced 0.14 + `sola-kit` (not WebView) |
| Grok agent | Shared leader (`grok-leader.service` / `~/.grok/leader.sock`) |
| Branch | `naturalethic/distribution` (dist image work); else `master` + `.worktrees/` |

**Install policy:** agents never run `cargo make install` without explicit
permission for that install. User installs and smokes.

**Useful:**

```bash
cargo make build
RUST_LOG=debug /opt/sola/bin/sola 2>&1 | tee /opt/sola/log/sola.log
tail -100 /opt/sola/log/sola.log
```

---

## Locked models (do not re-litigate)

| Topic | Rule |
|-------|------|
| UI stack | **Iced + sola-kit** only for new apps; WebView host is apocrypha |
| Compositor | **River** external; **sola-river** is the bus ↔ Wayland bridge |
| IPC | **Sola Bus** (Unix socket events) + Wayland for surfaces/input |
| Process model | Multi-process; each app independently restartable |
| Theme | Bus `Topic::Theme` + kit semantic tokens/fonts; shell chrome tokens |
| Browser engines | **WPE primary**, CEF parallel; no `accelerated_osr` crate feature |
| Agent backend | Attach to **shared Grok leader** — do not spawn private turn-loop agents |
| Worktrees | Feature code only under `.worktrees/`; approval = merge + cleanup |
| Install | Local `/opt/sola/bin/`; never install without explicit permission |
| Dist installer v1 | ISO primary; wizard = username + disk; US EN + Mac kbd; no password; loginless → Sola; flower splash |

---

## Session start checklist

1. [`AGENTS.md`](AGENTS.md)  
2. **This file** (Now + dogfood + locks)  
3. [`docs/capabilities.md`](docs/capabilities.md) — rows for the slice  
4. [`docs/open-questions.md`](docs/open-questions.md) — any D*?  
5. One freeze/plan for the domain if building  
6. [`docs/architecture.md`](docs/architecture.md) only if the map is unclear  

**End of slice:** [`.grok/skills/sola-progress-docs/SKILL.md`](.grok/skills/sola-progress-docs/SKILL.md).  
**No** one-off handoff files.

---

## Recently completed (compact)

Full history is git. Keep this list short (last few merges only).

| Slice | Note |
|-------|------|
| dist real apply | QEMU vdb install → loginless Sola; `vm install`; sola-desktop |
| dist image + splash | `sola-install`, `cargo make vm`, Plymouth clockwise cyan flower |
| initial-window-state | Default-float + kit CSD on monitor/settings/preview/mail/agent/terminal/kit/browser |
| kvm Mac scroll velocity gain | CG pixel inject + rate ramp (`scroll_accel`); dogfooded on ember |
| progress-docs practice | Spine: CURRENT, capabilities, architecture, roadmap, open-questions |
| build warning cleanup | Dead code / unused surface trimmed across agent, mail, terminal |
| bus-restart-output-geometry | Menubar stays framed across bus restart |
| sola-mail kit | IMAP/SMTP worker, three-pane UI, Helium open, IDLE refresh |
| preview-enhancements | Header + Copy path |

---

## Pointers

- Capabilities: [`docs/capabilities.md`](docs/capabilities.md)  
- Architecture: [`docs/architecture.md`](docs/architecture.md)  
- Roadmap: [`docs/roadmap.md`](docs/roadmap.md)  
- Open questions: [`docs/open-questions.md`](docs/open-questions.md)  
- Specs: [`docs/specs/`](docs/specs/)  
- Plans: [`docs/plans/`](docs/plans/)  
- Product manual: [`docs/manual/`](docs/manual/)  
