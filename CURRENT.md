# CURRENT — living product focus

**Only session handoff.** Update when priority, next slice, or known runtime
state changes. Read after `AGENTS.md`. Full model:
[`docs/progress-model.md`](docs/progress-model.md). Capability maturity:
[`docs/capabilities.md`](docs/capabilities.md).

**Decisions agents must ask about:**  
[`docs/open-questions.md` § Decision points](docs/open-questions.md#decision-points-ask-human).
Do not invent product policy.

**As of:** 2026-08-07 (marketing site design in Paper; distribution still on master)

---

## Now

1. **Marketing site (sola.computer) — design in progress** — Paper file
   **sola.computer** ([open](https://app.paper.design/file/01KZF8TSPFDJZ4APR05E2ADXBJ)):
   desktop + mobile landing. Product truth in root [`PRODUCT.md`](PRODUCT.md).
   Audience: Linux/NixOS builders; graphite product UI extended; primary CTA
   **Download ISO** (assumed available); honest early-product framing.
   **Next:** iterate design with human, then implement site (stack TBD).
2. **Distribution follow-through (product when resumed)** — freeze + plan still
   open for remaining bars, not a separate branch:  
   - Freeze: [`docs/specs/2026-08-05-distribution-image-design.md`](docs/specs/2026-08-05-distribution-image-design.md)  
   - Plan: [`docs/plans/2026-08-05-distribution-qemu-image-plan.md`](docs/plans/2026-08-05-distribution-qemu-image-plan.md)  
   - **Done on master:** qcow harness, flower Plymouth, `sola-install` wizard,
     real disk apply, loginless desktop, `cargo make vm` / `iso build|run`.  
   - **Still open:** QEMU **ISO** e2e (boot ISO → erase → reboot → Sola);
     timezone auto-detect (interim **US/Mountain** / `America/Denver`);
     Shape 1 release tarball refresh (v0.1.1 URL 404); operator manual page
     when ISO is dogfoodable.  
3. **Progress docs** — keep this file + `docs/capabilities.md` honest; no
   second handoff.  
4. **Follow-ups (unordered backlog, not priority):**  
   - Optional per-app default sizes in `window_settings`; dogfood float chrome  
   - Permanent `/dev/input` ACL or udev for sola-kvm — **D2**  
   - Permission fan-out UX when TUI + sola-agent both attached — **D1**  
   - Worktree hygiene: `libei-portal` archive/cleanup  
   - sola-preview: zoom, image clipboard copy, `solactl --region`  
   - sola-mail: inline rich-text link hits (vs chips), multiline polish  
   - Clipboard follow-ups —
     [`docs/specs/2026-07-30-sola-kvm-clipboard-design.md`](docs/specs/2026-07-30-sola-kvm-clipboard-design.md)  
   - Optional: Mac warp path cost; true HID scroll if CG velocity gain not enough  

**Explicit holds:** none.

**Always allowed:** pure safety/doc fixes; tests; progress-doc maintenance;
warning cleanups; worktree hygiene the user asks for.

---

## Known dogfood state

| | **primary (local)** | **dist (QEMU)** |
|--|---------------------|-----------------|
| Role | Daily dogfood desktop | Installer / image engineering |
| Launch | Physical TTY → `/opt/sola/bin/sola` | `cargo make vm run` / `iso run` |
| Install root | `/opt/sola/bin/`, logs `/opt/sola/log/` | Guest image + `var/images/` products |
| Bus / UI | sticky `~/.config/sola/state.toml`; Iced + kit | Same stack inside guest when installed |
| Dist path | Shape 1 colleague module (`INSTALL.md`) | QEMU **vdb** install → loginless Sola **OK**; **ISO e2e pending** |
| Branch | **`master`** | Feature work in `.worktrees/` |

**Install policy:** agents never run `cargo make install` without explicit
permission for that install. User installs and smokes.

**Useful:**

```bash
cargo make build
RUST_LOG=debug /opt/sola/bin/sola 2>&1 | tee /opt/sola/log/sola.log

# Distribution (you own `cargo build --release` first)
cargo make vm build|install|run
cargo make iso build|run          # products under var/images/
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
| Dev install | Local `/opt/sola/bin/`; never install without explicit permission |
| Dist installer v1 | **ISO primary**; wizard = username + disk; US EN + Mac kbd; no password; loginless → Sola; flower splash; interim TZ US/Mountain |
| Dist stage | Image builds stage from **`target/release` only** (never `/opt/sola/bin`) |

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
| **distribution → master** | ISO scaffold, qcow e2e install, splash, loginless desktop |
| initial-window-state | Default-float + kit CSD on first-party apps |
| kvm Mac scroll velocity gain | CG pixel inject + rate ramp |
| progress-docs practice | CURRENT / capabilities / architecture spine |
| settings Applications list-detail | Compact master-detail panel |

---

## Pointers

- Capabilities: [`docs/capabilities.md`](docs/capabilities.md)  
- Architecture: [`docs/architecture.md`](docs/architecture.md)  
- Roadmap: [`docs/roadmap.md`](docs/roadmap.md)  
- Open questions: [`docs/open-questions.md`](docs/open-questions.md)  
- Dist freeze: [`docs/specs/2026-08-05-distribution-image-design.md`](docs/specs/2026-08-05-distribution-image-design.md)  
- Dist plan: [`docs/plans/2026-08-05-distribution-qemu-image-plan.md`](docs/plans/2026-08-05-distribution-qemu-image-plan.md)  
- Shape 1 ops: [`INSTALL.md`](INSTALL.md)  
- Product manual: [`docs/manual/`](docs/manual/) (fonts only for dist today)  
