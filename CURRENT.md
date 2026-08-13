# CURRENT — living product focus

**Only session handoff.** Update when priority, next slice, or known runtime
state changes. Read after `AGENTS.md`. Full model:
[`docs/progress-model.md`](docs/progress-model.md). Capability maturity:
[`docs/capabilities.md`](docs/capabilities.md).

**Decisions agents must ask about:**  
[`docs/open-questions.md` § Decision points](docs/open-questions.md#decision-points-ask-human).
Do not invent product policy.

**As of:** 2026-08-13 (instant tab close; omnibox load line)

---

## Now

1. **Browser (CEF) — remaining engine polish**  
   - Branch: **`naturalethic/cef-browser`** (this worktree).  
   - Single crate `sola-browser`: iced chrome + CEF CPU OSR.  
   - **Dogfood (local):** one iced window; Profiles switch instant
     (menubar + kit identity select in the full-width chrome bar;
     visited tabs/profiles paint instantly from parked last-frames;
     uncached tabs blank immediately);
     omnibox unfocuses on submit (typed → resolved, no blank flash);
     thin accent load line in the omnibox; back/forward/stop from CEF;
     tab close is instant (strip does not flash the row back);
     YouTube signed in after full quit; colors OK after ARGB→BGRA
     swizzle; Bitwarden unlock (**`--release`**); fill after unlock;
     passkey get on Google.  
   - **Profile model:** `app_id` is always `sola-browser` (one switcher
     entry). Each profile is headless `sola-browser --engine` with its
     own CEF cookie root. Chrome-bar select is the quick switcher.  
   - Manual: `docs/manual/sola-browser.md`.  
   - **Not shipped:** passkey **registration**; remaining OSR quirks
     (caret/scroll/IME); first-visit session tabs.  
   - **Next:** remaining OSR (caret/scroll/IME) and passkey registration.  
2. **sola-arcade / windowed gamescope** — **partial, dogfoodable** (on master)  
   - Backlog: Portal-class nest fails; residual flicker; title contrast;
     never-played owned without API.  
3. **Distribution follow-through (when resumed)** — ISO e2e, TZ, tarball.  
4. **Follow-ups (unordered backlog):** float chrome, D1/D2, preview, mail,
   kvm clipboard, switcher FFM holdoff (`naturalethic/switcher-ffm-holdoff`
   unmerged), etc.

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
| Branch | **`naturalethic/cef-browser`** (browser); master for other daily | Feature work in worktrees / Orca workspaces |
| Browser | One chrome window + per-profile `--engine` helpers; instant Profiles switch (menubar + chrome-bar select); visited tabs/profiles paint from parked last-frames (miss blanks); omnibox load line + no submit blank-flash; instant tab close (no strip bounce); YouTube persists after quit; Bitwarden unlock/fill; passkey **get** (Google) | — |
| Arcade | Banner list + nest dogfooded (Core Keeper, PEAK); cache + ready-to-play filter + lazy banners; nest Steam exits on game quit; some titles still flaky | — |
| Nest paint | wayland+`-b`+`-S fit`; **no `-e`**; `--nested-steam` (no BPM) | — |

**Install policy:** agents never run `cargo make install` without explicit
permission for that install. User installs and smokes.

**Useful:**

```bash
cargo make build
cargo make install browser shell   # only with your OK
RUST_LOG=debug /opt/sola/bin/sola 2>&1 | tee /opt/sola/log/sola.log
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
| Browser | **CEF** in single `sola-browser` crate; no `accelerated_osr`; WPE path retired |
| Agent backend | Attach to **shared Grok leader** — do not spawn private turn-loop agents |
| Gamescope host | Windowed only (`-W`/`-H`, never host `-f`); product path is Arcade nest |
