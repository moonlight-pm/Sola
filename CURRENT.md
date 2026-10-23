# CURRENT — living product focus

**Only session handoff.** Update when priority, next slice, or known runtime
state changes. Read after `AGENTS.md`. Full model:
[`docs/progress-model.md`](docs/progress-model.md). Capability maturity:
[`docs/capabilities.md`](docs/capabilities.md).

**Decisions agents must ask about:**  
[`docs/open-questions.md` § Decision points](docs/open-questions.md#decision-points-ask-human).
Do not invent product policy.

**As of:** 2026-10-23 (Next's grey base in the kit on `next`, committed `ba33fda9`, applied (`pkg:sola` eeb66d85): the slate palette's blue tint is gone from the seed, the kit, the apps' own colors and `design/tokens.css`; the shell moved the saved slate theme to the greys. Porting the remaining apps to a window per document — one app at a time — is paused before it started). 2026-10-23 (Next slice 5 committed `388df10c`, applied and smoked by injection: desktop workspaces in the shell, a browser profile per workspace with several profiles live at once). 2026-10-23 (Next slice 6 committed `ec059189`, applied and smoked by injection: titlebars with each app's menus on every kit window, a 10 px gap grid, the bar an island, the menubar without app menus). 2026-10-22 (Next on `next`: Projects is a window per project with its tabs on a top bar; the shell bar marks a project working or waiting on a decision; every Sola app on canto renders on Vulkan; Oath pins Sola packs published from `next`). 2026-10-21 (Next tab bar on `next`: spec frozen; slices 1–2 applied; the browser has a window per page, applied and smoked by injection after a blank-page fix, groups and the strip gone (4g); Workspaces renamed **Projects** (a workspace is a tab), applied; `next` merges only when all of Next is done on canto). 2026-10-19 (site: **sola.moonlight.pm live** — project page + `/manual` rendered from `docs/manual` (11 chapters on the system — architecture, process manager, bus, call plane, compositor, session, shell, theme, a complete solactl reference — and an applications list; app notes stay in the repo); source moved to `site/` from Thoxa; `sola.computer` parked; INSTALL / CONTRIBUTING say the repo is public and fetch over HTTPS). 2026-10-19 (mail rework **on master** (merged from `sola-mail`) — per-account sessions, typed message ids, compose fixes; **installed** `mail` release 2026-10-19 (through `40db4486` — refused recipient named, failures logged; 3 accounts connected)). 2026-10-19 (spotify: Web API budget — session reads for playlists/library/albums/top tracks, page freshness, slow remote poll + Connect push hints, incremental liked sync, 429 probe/cooldown fixes; review + UI/player fixes; follow-up — sign-in no longer waits on `/me`, build warnings cleared; **on master**, **installed** `spotify` release; cold start opened straight into the app, no `/me`). 2026-10-18 (D-Bus deprecated as a requirement — no new D-Bus dependencies; future target is Sola's own replacement on Sola Bus; inventory in architecture, shape open as T3). 2026-10-18 (shell menubar: Lamzu mouse battery chip — udev rule applied on the host; **installed** `shell` release with the 10 s retry while the mouse sleeps; chip seen at 40%). 2026-10-15 (design: boards are never `was_hidden` — Chromium evicted the frames of all but 5 hidden boards and those stayed white for good; boards nobody looks at now paint at 1 fps, shown — **installed** `design`; Joshua confirmed every board renders). 2026-10-14 (design slice 8 on master: annotations — `[[section]]` / `[[note]]` / `[[link]]` / `[[flow]]` in `canvas.toml`, page exits `data-design-to`, arrows routed through the gutters, click-to-follow, tours; board titles fixed; board frames as patches, paced visible-only wakes, tiny-on-screen boards `Slow`, `--disable-gpu-compositing` for Design's helper (GPU 84% → ~30% at fit) — **installed** `design`+`solactl`; the human looked on a scratch copy of Wicket foundation and approved; 105 tests). 2026-10-14 (visual direction: canvas `design/next/` ("Next") is the canonical foundation path forward — base is dark grey; layout picks A–H open). 2026-10-14 (design memory budget, slice 7: boards fit in free memory (15% kept free), idle boards discarded with their picture kept and reopened on a look or call, agents refused with the reason, `canvas.close`, `memory` in `status` — **installed** `design`; `status` memory + `canvas.close` smoked; discard under real pressure unsmoked; 81 tests). 2026-10-13 (host memory safety net: zram + earlyoom in `configuration.nix` after ten agents' Design canvases filled 125G and froze the box — Design memory budget decided, parked as [idea](docs/ideas/2026-10-13-design-memory-budget.md)). 2026-10-13 (design board pace: boards nobody looks at sleep — off-screen canvases unless an agent is working there, boards out of view; in view 60 fps with focus, 1 fps without; work runs at full rate. Idle Design had held ~3.5 cores on 7 spinner boards — **installed** `design`, smoked). 2026-10-13 (design share viewer on phones: pages inert to touch, one finger pans, pinch only zooms, no hint — **installed** `design`, real phone unsmoked). 2026-10-13 (design: `auto` boards follow their page's height for the whole load (the page reports changes; 1 / 3 / 8 s re-measures gone) — **installed** `design`, smoked). 2026-10-12 (design: rail drag-reorder — canvases within their project, projects as blocks, order kept in `projects.json`; `project.remove --path` on the call plane (solactl + MCP) — **installed** `design` release, drag smoked). 2026-10-12 (workspaces: **root tabs** — New workspace **No worktree** box / `workspace.spawn --root`; every root tab works on master, worktree only when asked — **installed** `workspaces`+`solactl` release; CLI smoked, card unsmoked). 2026-10-12 (design: **sola-design** on master through slice 6 — multi-agent canvases, live share at `design.moonlight.pm` (viewer handles like Design) — installed). 2026-10-12 (calendar reminders **smoked** — `shell`+`calendar` release). 2026-10-11 (mail: focusing the window re-lists the open folder immediately; a working ring beside the count while that fetch runs — **installed** `mail` release, unsmoked). 2026-10-08 (workspaces: Claude is first-class — `--agent claude`, hooks merged into `~/.claude/settings.json`, rail `×N` from `compact_boundary`, reboot `claude --resume` — **installed** `workspaces` release, unsmoked. Hooks landed in `~/.claude/settings.json` (permissions, theme, plugins kept). OpenCode stays presence-only). 2026-10-08 (switcher: a pointer already on an icon does not steal the preselected app until it moves — **installed** `shell` release, unsmoked). 2026-10-08 (location bar: Enter loads the typed URL; a visit-history row loads only after that row is highlighted or clicked — the first match is not taken. The bar keeps the submitted URL until the engine leaves the previous page. **Installed** `browser` release, unsmoked). 2026-10-03 (workspaces: the wheel scrolls the terminal buffer — **installed** `workspaces` release, unsmoked. Rollout working/done installed. RimWorld `codex` stayed up across the restart). 2026-09-30 (Bitwarden current-page Autofill: security-state stays 1 on decrypt so checksum-less login URIs still match; blob encrypt only while a personal cipher is written — **installed** `browser` release, unsmoked). 2026-09-28 (`solactl browser fill --file` + read-back `value`/`matched`; `get` / `links`; `wait --control` until a named control exists — **on master**, not installed). 2026-09-24 (`solactl browser fill`/`type` paste into ProseMirror/contenteditable with newlines; `key --chord` inserts once; a wedged chrome times out with recovery copy — **on master**, not installed). 2026-09-23 (NUL in a page title no longer aborts chrome — **installed** `browser` release, unsmoked). 2026-09-23 (workspaces: `workspace.rm` / hover × drop the tab immediately; `tmux kill-session` and `git worktree remove` run off the iced thread so the rail does not freeze — **on master**, included in the 2026-10-08 `workspaces` release). 2026-09-21 (workspaces rail `×N` is the loudest Grok *or* Codex session — Codex counts `type: compacted` in `~/.codex/sessions/**/rollout-*-{id}.jsonl`; Grok still uses session-dir segments/checkpoints. Bots write-fence escape **installed** `bots` release; existing sessions adopt AGENTS.md on the next send). 2026-09-17 (workspaces rail: hover-pencil / double-click is `--title` (label defaults to worktree slug, no `slug ·` prefix); rename keys stay off the PTY; drag-reorder tabs inside a project; drag-reorder project groups; kit `reorder_within_sections` + `Event::Edit` — **installed** `kit`+`workspaces` release, unsmoked). 2026-09-16 (Bots worktree: daemon HTTP + iced + iOS SolaBot). 2026-09-14: OSR HTML5 kanban drag **dogfood good** — unique last_frame + damage continuity + overlay ring; remaining CPU-OSR mitigations parked. 2026-09-12 (vault PUT Bitwarden-Client-Version `2026.8.0` + blob encrypt — **installed** `browser` release. Profile-switch tab leak **installed**. 2026-09-11: workspaces: `pane.send` / `pane.read` / `workspace.exec` use tmux `=session:` pane target — split tabs no longer `send failed`; `solactl browser` **installed** `browser`+`solactl` release; Tertius smoke: background open + `wait --load` + YAML refs + background click-nav to IANA + httpbin fill/radio/checkbox; seat stayed on DHH. vault item **Edit** — **installed** `browser` release, unsmoked; 2026-09-10: workspaces: TUI selection follow during drag + page scroll (Grok/Codex CUP rewrite); Ctrl-D on one split leaf no longer kills the sibling tmux session (exact `=session` target); Codex first-class — `--agent grok|codex`, hooks merged into `~/.codex/hooks.json`, rail rollup; **installed** `workspaces` release; Claude still presence-only; volume device pick: 2s helper timeout so hung pw-dump cannot block set-default — **installed** `shell` release; mail: letter Date + local `HH:MM`; junk is `x` — **installed** `mail` release; number pad digits in the page — OSR `VK_NUMPAD*` not Home/End — **installed** `browser` release, unsmoked; terminal `open` / `xdg-open` on a PDF uses sola-browser, not Chrome — **installed** `browser` release; workspaces persist Grok `session_id` and `grok -r` after reboot, unsmoked; volume chip `pw-dump` JSON strip **installed** `shell` release, unsmoked; sola-calendar **installed** `bus`+`settings`+`calendar` release, unsmoked; 2026-09-04: terminal first tab / no-source pane starts in `$HOME`; kvm Linux listen Super+Tab confirm: virtual-keyboard **key before modifiers**; kernel auto-repeat not injected; sola-spotify: clicks re-activate after Connect inactive; session teardown waits before reconnect; `/me` 429 no longer 2s-polls; artist/album name links; album Save/Follow + artist Follow — **installed** `spotify` release; mail multi-IMAP canonical boxes; send identities **installed** `mail`+`settings` release; browser: vault stay-logged-in + Log out, HTTP auth persist, helper Shutdown-flush, Google passkey document-start intercept, ⌘⇧R hard reload — **installed** `browser` release, desk smoke pending; mail attachments send/receive; kvm Linux listen + CLIP1 text/`image/png`; workspaces `workspace.set --name` moves `.worktrees/<slug>` + `--branch`; self-cleanup: `workspace.rm --worktree` + gone-path reap; grid selection follows scroll; idle omnibox query-strip + Kagi lucky Enter + page ⌘V **smoked**; switcher count marks + grouped notify pile, no cap 20 — **installed** `kit`+`shell` release; shell menubar: pixel-graph stats + 12-band spectrum **raster to a nearest-neighbor image** (not iced canvas 1×1; GLES2 / software GL); notify pile bell + count, Super+Shift+4 freeze keeps open panels, app-menu X from layout — **installed** `shell` release; Super+K live Frame is **card-sized**; DevTools docked panel **smoked**; JS dialogs kit modal; sola-spotify **on master** **installed** `spotify` release; number pad digits **smoked**; screenshot chords → clipboard + promised PNG; `cargo make` defaults to release; Arcade watch / singleton / refuse-live-Steam; Slack huddle camera **smoked**; mail move-rules; GPU idle → [`PERFORMANCE.md`](PERFORMANCE.md))
---

## Now

**Next — what follows (Joshua, 2026-10-23).** Port the Sola apps that still keep their own list of documents to a window per document (the shell's bar lists them), **one app at a time** — paused 2026-10-23 before picking the first. Candidates: Paint (its image list), Design (the project → canvas rail), Bots (the bot rail), the wrapper's popups (they replace the view). The navigation sidebars of Mail, Calendar, Spotify, Settings and Monitor are not tab lists. Also open: the islands (board 03), a workspace overview, slice 6's leftovers.

**Next slice 5 — desktop workspaces, a browser profile each (started 2026-10-23; applied, smoked, committed `388df10c` on `next`).** Joshua took board 04's proposals (switcher pills next to the flower with names and marks, named workspaces kept, Super+Tab across all, a window on one workspace), each workspace picks a browser profile (shared allowed), a page sent to a workspace with another profile reopens there under it, N6 dropped (projects not tied to workspaces). Spec § Desktop workspaces, a browser profile each; plan Phase 5 (5a–5e done). Shell: `stacks/` (a desk per workspace), `app/workspaces.rs`, `menubar/pills.rs`, `menu/workspace.rs`; Super+1…9 / Super+Ctrl+1…9 / Super+Ctrl+(Shift+)Tab; `Topic::Workspaces`, `Topic::WindowWorkspace`; Design's Actual Size is ⌘⇧0. Browser: the router routes by page and keeps every profile's helper live (front only while a page is on screen or active), `app/pages.rs` opens pages under the showing workspace's profile and reopens a page in place when its workspace's profile differs, sessions per profile. Tests: shell 219, browser 287, bus 110, kit 166, design 105. Applied by Joshua 2026-10-23 and smoked by injection: pills and their menu, new / rename / delete a workspace, Super+1 / Super+2, Super+Ctrl+2 sends a page and it reopens under the target's profile, Profiles → New Profile sets it for the workspace (the page reopens in place), Delete Profile reopens its pages under the first profile left, helpers idle with pages on a hidden workspace. Restart Shell first reopened pages back and forth (the shell spoke before its layout was back); fixed and applied (`pkg:sola` 96b78a20, `pkg:sola-browser` 335396fc): a full session restart restored both workspaces and both profiles' pages, nothing reopened. Two follow-ups went in with slice 5 and were applied with the grey base (`pkg:sola-browser` f97781ba): `profiles.json` forgets deleted workspaces (seen: the deleted workspace's entry is gone), and the Delete Profile dialog says the pages reopen under the first profile left (unsmoked; needs a second profile). Known: sola-wrapper's `edit_menu_has_super_chords` test has failed since 2026-09-03 (the Edit menu grew to seven items).

**Next slice 6 — titlebars with menus, gaps, the bar as an island (started 2026-10-22, applied 2026-10-23, committed `ec059189`).** Joshua took boards 01–02's PROPOSED picks as drawn (one 38 px row, unfocused menus dimmed, gap 10, 12 px corners, lit rim, black ground), kit-drawn dropdowns, a compositor hairline for foreign windows, and the 28 px band kept (islands later). Spec § Titlebars, gaps, the bar as an island; plan Phase 6. Built: kit `window_frame` (+ `island`, `menu::publish` / `choose`), every app on it (Design and Install new), the shell's gap grid and island bar, the menubar without app menus, `Topic::WindowChrome` (shell → sola-river `set_tiled` + rim; apps drop the frame in Cinema). **Applied** by Joshua (`pkg:sola` 28a139da, `pkg:sola-browser` f4e3fcd7, `pkg:sola-design` 06be463b); smoked by injection (corners and gaps at 5×, menu open / hover-switch / Esc, Window → Float and → Right from the titlebar, Cinema). Joshua: "looks great" → committed. **Leftovers:** menus folding into `···` on a narrow tile, zone glyph + stack count on the titlebar, real window titles from apps, the Window menu's unbound snap zones, the shell's leftover `is_system` menu state.

**Next — one tab list (the bar), branch `next` on canto, started 2026-10-21; spec frozen, slices 1–2 done, the browser (slice 4) applied.** Joshua picked the Next direction of boards 05–06: a shell-owned bar docked on the left lists every window by zone; a tiled zone shows one tab; in the end state each page, shell and agent session is its own window and the apps drop their tab lists (the desks idea's panel model). Answered 2026-10-21: spec first (N1); **two zones, Left / Right 50 / 50 on canto** (1920 × 1080; configurable if it goes back to novus) (N2); a new window joins the focused zone (N3); docked, 240 px (N4); the bar first under today's menubar, titlebars with menus as each app converts, gaps / islands after (N8). N6 (workspace bound to a project) and N7 (drop browser groups) accepted for now; N5 (the word "workspace") open. Spec [`docs/specs/2026-10-21-next-tab-bar-design.md`](docs/specs/2026-10-21-next-tab-bar-design.md), plan [`docs/plans/2026-10-21-next-tab-bar-plan.md`](docs/plans/2026-10-21-next-tab-bar-plan.md). Slice 1 is shell-only (per-zone stacks, the bar window, `TabLayout`, sola-river forwards `parent`); apps unchanged. **Frozen 2026-10-21** (Joshua: "Proceed"). **Slice 1 applied on canto 2026-10-21 19:22Z** (`pkg:sola` 46b6aa8e, from `c3eebd77`): the bar window, per-zone stacks (`crates/sola-shell/src/stacks.rs`), `Topic::TabLayout` + `Topic::WindowParent`, zone keys and Super+\`; 200 shell tests pass. Seen: the bar draws (Left · 4, Right · 0, Floating · 0), the shown tab fills the content area, hidden Design / Terminal idle at ~0.7% CPU. A bug seen on the first apply (with no saved layout the shell took its own first save as the saved layout, leaving focus on a tab behind another) is fixed in `b91a8ca8`, **applied 2026-10-21 20:02Z** (`pkg:sola` a3236c19). Smoked after that apply: the full restart restored the Left zone's saved order (browser, Terminal, Design, Workspaces) although the windows arrived in another order, with Workspaces shown and focused. Smoked by injection 2026-10-21 (Joshua away from the desk): a bar row click shows that tab and focuses it (menubar follows); Super+KP_6 moved Terminal to Right (840 / 840 split, Left fell to its next tab, focus followed); Super+\` and Super+Shift+\` stepped Left's stack both ways; Super+KP_* floated Design inset from its tile (740×952, Floating · 1, Left fell to Workspaces) and Super+KP_4 tiled it back; KP_0 Fullscreen covered the content area and toggled back; KP_Enter Cinema covered the output and toggled back. Keys went in as bus `Chord` events (`solactl emit`), so River's key path for \` and the numpad is not exercised (solactl's key injector has no \` or keypad names). After `82c43f6a` (`solactl compositor input press / release / drag`, \` and keypad key names; applied 2026-10-21, `pkg:sola` 9a581685): the restart restored the split (Workspaces in Left, Terminal in Right); a row **drag** moved Design into Right below Terminal (shown, focused); Super+\`, Super+Shift+\` and Super+KP_4 **through rivulet** step and move as with bus chords. Not smoked: a dialog riding its parent (nothing open makes one). **Slice 2 built 2026-10-21 (`9630e82d`), not applied:** windows as tabs — `Topic::Tab` / `Topic::TabWindow`, the shell pairs tabs to windows by process and open order (`crates/sola-shell/src/pairing.rs`), the saved layout keys tabs by tab key, kit `tabs::TabWindows`; **Terminal has a window per tab** (no tab strip, ⌘T opens a tab window in the focused zone, closing a window closes its tab, one process). **Applied 2026-10-21** (`pkg:sola` 984819fd): ⌘T, splits, pane close, closing a tab window, Super+\` between Terminal tabs, and a second launch handing off to the running Terminal all smoked by injection. **Restart Shell swapped two Terminal tabs' pairings** (tabs replayed one at a time while every window was up); fixed in `46fa76a7` by announcing each tab again with its window id (named tabs pair first), **applied 2026-10-21** (`pkg:sola` 5eb94ecb): a fresh start paired both Terminal tabs in restore order, and Restart Shell kept both pairings, Right's order and its shown tab. Slice 2 is done but for a dialog smoke. Joshua 2026-10-21: **the browser is next** (slice 4: a window per page, no tab strip or groups) — **built 2026-10-21 (`5f8c89d8`), applied** (`pkg:sola-browser` bab0cc35): the engine addresses pages (`Cmd::Target`, `Cmd::ShowPage`, a page's own size), a frame slot and texture per page window, the window you are in is the active page with the full chrome and the others show their page under a read-only bar, ⌘-click opens a page window behind (`Tab.background`), the shell reports windows on and off screen (`TabWindow.shown`) so pages behind rest, an app can ask for its tab shown (`Topic::ShowTab`), and the shell no longer raises the browser on `OpenUrl`. Smoked by injection: restore, ⌘T, `solactl browser goto`, a background open joining its zone behind, ⌘W, focus following, find. A page opened behind stayed blank when shown and DevTools blanked the page and pane (a page window stopped asking for redraws after its first frame) — fixed in `e6c0c8e2`, **applied 2026-10-21** (`pkg:sola-browser` cb88b909): both paint, a restart restores the pages, Design renders. A `target=_blank` popup opened as its own window but sometimes stayed black (CEF can paint a stale size last while a new window is sized twice) — fixed in `f035991e`, **applied** (`pkg:sola-browser` 2e1f63db): 6 of 6 popups painted. Not smoked: ⌘-click (no injector holds ⌘ over a click), vault, downloads, sola-wrapper. **4g done, applied 2026-10-21** (`pkg:sola-browser` 7c2541c2): tab groups, the dead strip view, the favicon cache, `group.*` and `tab.move` are gone (old `session.json` files still load); `tab.focus` / `--select` show the page's window — smoked with ⌘W / ⌘⇧T; **N5 decided — the agent app becomes Projects** (`sola-projects`) before OS-workspace code, which reopens the Workspaces names lock at that time. **The Projects rename is done and applied (2026-10-21, `pkg:sola` 4bee9e3c):** `sola-workspaces` is **Projects** (`sola-projects`, owner `projects`, `solactl projects`); the unit inside a project is a **tab** (`tab.spawn` / `tab.rm` / `--tab`; Joshua: tab, clean break, no alias); config moved to `~/.config/sola/projects/`, session restore relaunched it under the new id, live tmux sessions carried over (runtime names `sola-ws` / `sws-` / `SOLA_WS_*` unchanged). Joshua updates his own `~/.agents/skills` (`solactl-workspaces`, `cleanup`). **Then (Joshua): a window per project** — the rail took too much room; each project is a window (a shell tab named for the project) with its tabs on a top bar; closing a window keeps the project (Project → Open). Built and applied 2026-10-21 (`f6ef9cf8`); smoked by injection (switch, `tab.select`, close / reopen). Gaps closed the same day: status marks draw (kit `status_mark` is SVG now — canvas meshes landed in the wrong place on wgpu's GL backend, which canto used because Oath's Mesa had no Vulkan driver), drag reorder, wheel scrolling on overflow, reveal on select; chip × and the + card smoked. **canto renders every Sola app on Vulkan since 2026-10-22** (RADV on rivulet's card `0000:06:00.0`; Oath ships RADV, the loader and Mesa's device-select layer, and `pkg:cef` no longer bundles its own loader; no Sola env change). The old canvas mark draws in place on Vulkan and misdraws with `WGPU_BACKEND=gl`, so marks stay SVG for any GL fallback. Not looked at: Design boards up close on Vulkan. **Activity in the shell's bar (2026-10-22, applied):** a project window's row shows its project's mark in the icon's place — diamond if any tab is waiting on a decision, ring if any is working, icon otherwise (new sticky `Topic::TabActivity`, kit `TabWindows::set_activity`). Smoked on canto: the Sola row spun while this tab worked, turned to the diamond when a scratch tab waited (stand-in `claude` process + a permission-prompt hook), and back to the ring when it closed. A spinning ring costs the shell ~8% of a core on canto (12.2% vs 4.2% idle) — open question for Joshua: keep the spin, or a still ring in the bar. canto's compositor is **rivulet** (Oath's C port of River 0.4.5, same protocols): any River feature Sola starts using must work there (checked: `parent`, `identifier`, `use_ssd`, `get_decoration_above`), and Oath can add compositor features River lacks. **`next` merges to master only when all of Next is done and tested on canto** — this bar, window-per-tab apps, titlebar menus, islands, gaps.

**Oath without a fork — branch `next` on canto (Oath), started 2026-10-20.** Joshua's second machine (canto, Mac Pro, the Oath distro) runs Sola from Oath's `oath-sola` fork; `next` is this machine's main branch and absorbs that fork so Oath can drop it, while NixOS keeps working for the eventual merge to master. Decided 2026-10-20: `sola-oath` (the Oath catalog app) lives here; building and publishing Sola's Oath packages moves into Sola and uses the `oath` CLI (Oath stays as unaware of Sola as it can, beyond the desk profile); the dev loop on canto is repackage + `oath apply`; the Oath side is briefed through `solactl projects tab.exec -t ws-main-2` (Oath's root tab). **Done on `next`:** `sola_core::paths` (env override → platform default → `/opt/sola`), the fork's generic fixes, power through `oath`, `sola-oath`; whole workspace builds on Oath; unit tests pass. Oath delivered its side (output 923bc0cef779): `oath store`, services restart when their package moves (live after canto's reboot), images take a pack pinned by hash. `cargo make oath stage|apply` is on `next`. **canto runs `next` (2026-10-20 22:02Z, first `cargo make oath apply`, Joshua's go-ahead):** `pkg:sola` `461aa9c8b04f`, `pkg:sola-browser` `c6505655281a`, `pkg:sola-design` `5ec4b77e63a6` (built from 824b6ec4); PID 1 restarted the six `svc:sola-*`, the session restored browser, Design, terminal and Workspaces, tmux shells kept. Pre-existing noise, same as the fork build: the shell's Bluetooth poll warns every 5 s (no system bus on Oath), the terminal's `tmux new-session -d failed` at start. **Published 2026-10-20** (`cargo make oath publish` from 2ed8fe24 on canto, Joshua's go-ahead; origin key copied from novus to `~/.config/oath/origin.env`): plan-built `pkg:sola` `58089ce24f88`, `pkg:sola-browser` `69f6f57df457`, `pkg:sola-design` `d9a6c2f0ca38`, with their plans, `pkg:sola-src` `43839b791b82` and `pkg:sola-crates` `e866fa3de67b` on `store.oath.moonlight.pm`; a fetch from the public origin re-hashes to the pin. canto keeps its dev pins. **Oath dropped the fork (2026-10-20, Oath 06cd14b..4f289a8, R28):** desk.json pins those three published packs (fetched and checked by Oath's `cargo make build`; output 4ec147de8bb5 passes 55 QEMU cases), `pkgs/sola*`, `forks/sola` and `cargo_vendor` are gone, `SOLA_LOG_DIR` left the svc env (Sola's Oath default is the same dir). **Release flow:** `cargo make oath publish` from a committed tree, then send the Oath tab the printed desk.json entries; it swaps the pins. A new `pkg:cef` (or mesa, wayland, …) makes Oath warn that Sola needs republishing. Publish compiles once (`plan:sola-build`, then three short copy plans); `--local` builds without uploading. **Published from `next` 2026-10-22** (Joshua's go-ahead each time; master has no `cargo make oath publish`): from `c53b18e4` against Oath's RADV Mesa, then from `c6cda58b` against the `pkg:cef` without its own Vulkan loader. **Oath pins** `pkg:sola` `6fb7b2bb`, `pkg:sola-browser` `60704d4b`, `pkg:sola-design` `31a35a61` (Oath master `aea1daf`; the desk image fetched them and passes 56 QEMU cases), so Oath's image now carries Next. **canto** ran those published packs (generations 234–237), then went back to dev packs for the bar's activity marks (generation 238, from `2c4b95a4`'s code). `moonlight-pm/oath-sola` is archived on GitHub (Joshua, 2026-10-20).

**Security — the bots HTTP token is public (found 2026-10-19).** `sola-botsd` runs in every session on `0.0.0.0:27419`; its bearer token is a constant in public source and the desk firewall is off, so the LAN can drive yolo Grok bots. Not reachable from outside today. Decision [D7](docs/open-questions.md#d7--bots-http-api-exposure-p0) (rotate + move the secret, bind address, firewall, managed or opt-in). Other findings from writing the manual are in the Gaps of the `supervisor`, `bus`, `call`, `river-bridge`, `session-mgr`, `theme-bus`, `kit`, `fonts-dist`, `solactl`, `doc-truth`, and `site` rows of [capabilities](docs/capabilities.md).

**Visual foundation — Next (`design/next/`), started 2026-10-14; samples, nothing decided.** Joshua named Next the canonical foundation path forward, building on the Islands exploration (his favorite of `design/islands`, `solar`, `instrument`, `agent-desk`). Three moves: app menus move from the shell bar into each app's titlebar; every window wears a titlebar (kit apps draw it, foreign apps get one from sola-river via `use_ssd` + `get_decoration_above` when they accept server-side decorations); one 10 px gap grid (Omarchy-style), so the top row holds only island bubbles. Boards: cover, desk, 01 menus on the window, 02 gaps and the ground, 03 the top row, 04 workspaces (OS sense: a whole desk of windows per workspace, switcher bubble beside the flower, overview grows from it — idea [desks](docs/ideas/2026-10-12-desks.md) part A only; its panel model stays parked), 05 the master sidebar, two boards (Joshua's alternative: a left tree of workspace → Left / Center / Right / Floating zones → each zone's window stack; docked, rail, or summoned from 04's bubble), 06 one tab list, two boards (Joshua's direction from 05: the bar is the only tab strip — apps drop their own lists, zones hold mixed tabs (pages, shells, agent sessions), the titlebar follows the shown tab; the workspace switcher is back in the top row and the bar lists only that workspace. This is the desks idea's panel model). Joshua is leaning to rename `sola-workspaces` → `sola-projects` to free the word — not decided; it reopens the **Workspaces names** lock. **Base decided (Joshua, 2026-10-14): dark grey**, not slate — each slate token's L* in neutral grey, surfaces and lines 20% darker, text unchanged; accent and status colors unchanged. **In the kit since 2026-10-23 (Joshua asked; on `next`, applied `pkg:sola` eeb66d85, committed):** `Palette::seed`, kit `hex::*` and shell defaults, `CHROME_SURFACE` and the other kit constants, the shell's stats chips, Calendar, Settings, Spotify's and Settings' sign-in pages and Design's share viewer use the greys; `design/tokens.css` carries them (next.css no longer overrides); the shell swaps a saved theme's slate values for their greys and saves it (`Theme::retire_slate`, colors picked by hand stay) — on canto it did so once at the first start and every sampled chrome pixel is neutral (bar `#141414`, titlebar `#151515`, terminal `#0b0b0b`, menubar `#050505`). The choosable accent waits for the later **sola-kit overhaul** (Joshua). Picks A–H (row layout, unfocused menus, browser, gap, corners, focus, ground, top-row arrangement) are listed on the cover with proposals; ask Joshua, do not pick. `design/foundation/` stays the as-built authority until Next's picks land in the kit. "Graphite" stays the name of Sola's UI style (Joshua, 2026-10-14). 2026-10-21: the windowing model of boards 05–06 is being built first, on `next` (the item above); the visual picks A–H stay open.

**sola-design — on master through slice 8 (annotations, exits, flows; after slice 7, the memory budget); all installed.** Slice 8: agents annotate canvases (sections, numbered notes, board links, flows as tours; exits in pages with `data-design-to`), arrows run through the gutters and never across a page, clicking an exit or a board link glides the camera there; only agents edit designs (no human notes or feedback). Board frames cross the frame socket as patches of the tiles that changed, and Design's helper composites in software. HTML artboards on a canvas, edited by agents — freeze [`docs/specs/2026-10-12-sola-design-design.md`](docs/specs/2026-10-12-sola-design-design.md), plan [`docs/plans/2026-10-12-sola-design-plan.md`](docs/plans/2026-10-12-sola-design-plan.md), manual [`docs/manual/sola-design.md`](docs/manual/sola-design.md). Canvases are `<project>/design/<canvas>/canvas.toml` + HTML, served over loopback http (project root as `/`, optional canvas `root`, D5). Sola's own visual authority is the canvas `design/foundation/` (tokens in `design/tokens.css`). Slice 6: **multi-agent canvases** — agents' `open` / `canvas.new` stay off the human's screen, page calls take `--canvas` (MCP: each agent's own), rail marks agent activity; **live share** (D6) — every canvas has a rotatable link `https://design.moonlight.pm/<token>/` served by Design through the Cloudflare Tunnel `sola-design` (Design runs `cloudflared`). Share links load on the LAN too: hub's Wicket v1 now resolves domains it keeps only for mail (`moonlight.pm`) upstream (hub `6b47539`). **Next:** human smoke (two agents at once, a link on a phone); add `cloudflared` to `configuration.nix` (now a user GC root in `~/.local/state/sola/`). Share viewer: a right-drag that starts on an artboard now pans (the page hook cancelled `pointerdown`, which also suppressed the `mousedown` that started the pan; pan now runs on pointer events) — **installed** `design`, unsmoked. Rail rows drag (canvases within their project, project headers as blocks; order saved in `projects.json`) and `project.remove --path` forgets a project from solactl / MCP (`design_project_remove`) — **installed** `design`; `project.remove` smoked (solactl + MCP, folder already deleted), drag smoked by Joshua. `auto` boards follow their page's height while it is open (late content at 12 s grew its board; a `vh` page stops after a few steps per load) — **installed** `design`, smoked. **Board pace** (**installed** `design`, smoked by Joshua): boards nobody looks at sleep (`Cmd::BoardPace` Live / Slow / Asleep) — off-screen canvases and boards out of view paint at 1 fps, boards in view run 60 fps with focus and 1 fps without; loading, settling, and agent calls wake a board at full rate. **Fix 2026-10-15 (installed `design`; Joshua confirmed every board renders):** asleep boards were `was_hidden`; Chromium evicts the frames of all but 5 hidden views, and an evicted board never got BeginFrames again — it stayed a white box (21 of 26 boards on Wicket `foundation`; DevTools: those pages visible and loaded, rAF never fires). Boards are never hidden now; Asleep is 1 fps — after the install all 36 live boards (Wicket `foundation` + `next` asleep off screen) were `visible` with rAF firing within 1 s. Pages stay shared between Joshua and agents by choice. Share links: no password or expiry by choice (Joshua, 2026-10-13) — `share.rotate` cuts a link off; they work only while Design runs. **Memory budget — slice 7 installed 2026-10-14.** After ten agents' canvases filled all 125G and froze the box (host net since: zram + earlyoom), Design keeps 15% of RAM free and fits its boards in the rest: short on memory it discards idle boards (tab closed, picture kept; reopen on a look or an agent's call), and refuses an agent — with the numbers, what is open and why, and the way out — only when the boards in use do not fit. New `canvas.close`; `status` reports `discarded` and `memory`. Smoked: `status` memory; `canvas.close` of a 10-board canvas (its ~2.9G came back within ~20 s — the CEF GPU process returns shared memory late, so discards are carried 20 s). **Next:** see a discard under real pressure (picture kept, reopens on pan / `screenshot`) and an agent read a refusal. Parked: renderer-death recovery, cheaper boards, cgroup fence — [idea](docs/ideas/2026-10-13-design-memory-budget.md).

**Mouse battery in the menubar — installed 2026-10-18 (`shell` release); chip seen (40%, 16:42).** The kernel has no battery for the Lamzu 4K receiver (`3554:f510`), so the shell asks the mouse over hidraw (vendor report 8, command 4): mouse glyph + percent, left of Bluetooth; amber ≤20%, red ≤10%, charging glyph; dims after 10 min without an answer. Verified by hand and through the Rust path (45%, discharging). A **sleeping mouse does not answer** (battery and firmware queries both silent for minutes, as root too), so the shell asks every 10 s while silent and every 60 s after an answer (the first install asked only once a minute and the chip waited). Host: the udev rule (group `input`, 0660) is applied — `configuration.nix` + `nixos-rebuild switch`, after pinning the river 0.4.5 override's `zigDeps` hash (unpinned `nixpkgs-unstable` moved river to 0.5.0 / `zig.fetchDeps`); the switch also moved zed-editor 1.22 → 1.24.2 and electron 44.5 → 44.7. **Next:** check what a switched-off mouse does (silence → dim, or a bogus 0%?); watch amber / red / charging when they come.

**Spotify — Web API budget, on master, installed `spotify` release 2026-10-19, partly smoked.** The shared client id (`d420a117…`, shared with ncspot / spotify-player / Omarchy) answered 429 to the very first requests at startup (log 2026-10-19 10:28), so the app now spends it sparingly. With local playback up, playlists, the playlist library (rootlist), album pages, and an artist's popular tracks come over the librespot session (ported from [Spotifast](https://github.com/crmne/spotifast), MIT; Web API fallback). Pages are reused while fresh and clicking the current page refreshes; liked state comes from an incrementally synced `liked.json`; another device's playback is polled every 15 s (60 s with Connect cluster pushes, 4 s after a command) instead of every 2 s plus a `contains`; devices load when the picker opens. Transport: cooldown after the permit, one probe after a 429, quota cooldown, background slot cap, player commands fail fast while cooling and never block the worker; volume/seek no longer re-activate this computer. UI/player fixes from the review: Load more duplicate/replace bugs, Albums append, sliders send on release (debounced for wheel/keys), Home/Search 64px row art, 1 s tick only for a remote clock, art dedupe/retry/LRU + disk prune, atomic settings/art writes off the UI thread, quit unregisters the Connect device. Personal Spotify app: declined. **Follow-up (installed 17:04):** after the first install the app sat on *Signing in…* for over a minute because it waited for `/me`, which the shared app kept answering 429 (5 s / 25 s Retry-After at each 30 s window edge, the whole time). Sign-in now opens at once on the saved token and last run's profile (`profile.json`); the profile comes over the session (id, country, plan, name from the profile view), and `/me` only runs without one. Dead code removed (personal-grant and legacy-token code, unused Web API calls, `NetActivity`, MPRIS `Seeked`); patched `iced_winit` uses `try_recv`. Seen after the follow-up install: no `/me` request, local playback ready 1 s after the restart, a playlist loaded 3 s after. Still 429ing every ~30 s while the shared quota is saturated: Home's category playlists (`/browse/categories/…/playlists`) and the remote `/me/player` poll (it kept polling after a local load), and the 15-min liked sync retries four times per round — next: parse Connect cluster pushes instead of polling `/me/player`, and stop background reads retrying on 429. Smoke still open: cold start (no 429 burst; playlist opens via the session), Back/Forward without requests, play on the phone and pause from here, volume drag on a remote device.

**Workspaces root tabs — installed 2026-10-12 (`workspaces`+`solactl` release); CLI smoked, card unsmoked.** The New workspace card has a **No worktree — work on master in the root** box (`workspace.spawn --root`). The tab opens in the project root (`kind: root`): hover × closes it (tmux only — never `git worktree remove`), the pencil sets the title, the startup script is skipped, `--worktree` / `--name` / `--branch` are refused. AGENTS.md (repo + `~/AGENTS.md`) and the workspaces / cleanup / sync skills: **every** root tab — the main tab included — works on master and opens a worktree only when asked. CLI smoke: `spawn --root` landed `kind: root` at the project root; `--branch`, `set --name`, and `rm --worktree` refused; plain `rm` dropped the tab and its tmux. Next: tick the box on the card, check the copy, × and pencil.

**Mail — rework on master, installed 2026-10-19; desk smoke open.** Per-account sessions, search hits that act in their own mailbox, Forward, Save Draft, mark unread / flag, compose Tab order + address suggestions + per-address check, a refused recipient named in the toast, failures in the log (item 9). List dates, account tags, decoded subjects and Forward seen on the desk; freeze checklist items 11–17 still to run. Focus check (installed 2026-10-11): switching to the Mail window re-lists the open folder and mailbox counts immediately (the same silent fetch as Get New Mail and the 45s poll). A working ring sits beside the message count while that fetch is in flight (hover: Checking mail). The action toast stays for moves and undo. IDLE still pushes while Mail is in the background. A search in progress is left alone.

**Claude in Workspaces — installed 2026-10-08, unsmoked.** `--agent claude` on spawn and exec. Hooks merged into `~/.claude/settings.json` and left permissions, theme, plugins, and other hooks in place. The app execv-restarted at 16:28. The rail mark includes Claude. Quiet `×N` counts `compact_boundary` in `~/.claude/projects/<encoded-cwd>/<session>.jsonl`. A new tmux after reboot runs `claude --resume <id>` when that transcript is still on disk. OpenCode stays presence-only. Next: smoke a Claude pane (working / waiting / done, `×N` after a compact).

**Switcher — still pointer installed 2026-10-08, unsmoked.** Super+Tab keeps the previous app highlighted until the pointer moves a few pixels.

**Codex in Workspaces — buffer wheel installed 2026-10-03, unsmoked.** The wheel scrolls the terminal buffer. Codex writes the transcript with a scroll region; tmux forwards that scroll-up onto the outer alternate screen, and that grid keeps scrollback. Mouse mode stays one SGR report. Rollout `task_started` / `task_complete` is installed. RimWorld `ws-main-11` `codex` stayed up across the 19:12 restart. Lines already discarded before this install stay gone. Next: wheel the transcript after new output.

0. **Bots** (on master) — named informational LLM sessions on this
   machine; iced dialog + native iPhone; not a PTY, not coding.
   Freeze
   [`docs/specs/2026-09-15-sola-bots-design.md`](docs/specs/2026-09-15-sola-bots-design.md).
   **Installed** `bots`+`solactl`+`browser`. HTTP
   `https://bot.sola.computer` → desk `:27419`. iOS **SolaBot** on Ember
   (`~/Workspace/SolaBot`); default scheme **SolaBot-Release** (Run =
   Release). Phone **and** iced Bots use HTTP SSE `GET /events`
   (not sola-call poll). List swipe is Mail-style `swipeActions`.
   ACP no longer re-sends a turn after disconnect. Chat markdown /
   `verse` fences. **Installed** `bots` release 2026-09-21 (write-fence
   escape in the foundation prompt; existing sessions adopt AGENTS.md
   on the next send). Write fence default is the bot home; Joshua can
   order an escape. Seat steal never lifts. **Next:** session-roll TBD.
1. **Window menu + Super+K** — kit Window menu (zones, float, hide, cycle);
   shell injects it when an app omits it; Super+K shortcuts overlay
   (Omarchy chord). Freeze
   [`docs/specs/2026-08-31-window-menu-and-shortcuts-design.md`](docs/specs/2026-08-31-window-menu-and-shortcuts-design.md).
   Paint Crop moved to **⌘⇧K**. Terminal publishes the kit Window menu
   (after Edit). **Installed** `kit shell paint terminal` (debug, 2026-08-31).
   Super+K live swapchain is the cheatsheet card + shadow pad (not the
   full usable output) — same pattern as menus; leftover pad dismisses,
   desk clicks pass through. Launcher / switcher / selection stay
   full-output. **Installed** `shell` release 2026-09-02. **Next:**
   desk-smoke Window menu + Super+K (and this card Frame). Gaps: no
   zone checkmarks; chords not remappable.
2. **Screenshots + image clipboard** — Super+Shift+3/4/5 advertise `image/png` at the chord (paste waits on the pipe), Fastest-encode in the background, toast **Screenshot copied**. No file, no Preview. Slack ⌘V **smoked**. `solactl compositor screenshot` still writes a PNG. Freeze [`docs/specs/2026-09-01-image-clipboard-design.md`](docs/specs/2026-09-01-image-clipboard-design.md). **Installed** `kit`+`shell` **release** 2026-09-01. Super+Shift+4 copies the live scene **before** dismissing menubar panels (open notifications stay in the freeze) — **installed** `shell` release 2026-09-02. Gaps: occluded / Super+H-hidden `--app`; multi-output.
3. **Browser** — editing the location bar and pressing Enter loads the
   typed URL. A visit-history row is used only after ↓ / `Ctrl+N` or a
   click highlights it; the first match is not taken. Until the engine
   leaves the previous page, the bar keeps the URL just submitted (a
   stale snapshot was painting the old address back). **Installed**
   `browser` release 2026-10-08, unsmoked. Same-site Enter (a different
   path on the current host loads here) is in that same release.
   A page title with an interior NUL no longer aborts
   chrome. The Washington Digital Archives download tab
   (`magick-tg-P98v8XUbFBC6uv9Jd58pTwIuN1fnM` plus a trailing NUL) was
   killing `xdg_toplevel.set_title` on launch. Control characters are
   stripped from the window title, CEF title updates, and `session.json`
   restore. **Installed** `browser` release 2026-09-23, unsmoked.
   Codecs CEF (H.264/AAC `canPlayType` `probably`) +
   `--autoplay-policy=no-user-gesture-required`; unified Bitwarden panel;
   **⌘⌥G** new group (name focused+selected; hover pencil); **pocket
   color** (edit-mode chip + compact picker, black/semibold on light fills).
   Tab-strip **favicons** (16px slot; globe until PNG; empty on blank) and
   `window.open` → **focused tab** (Cloudways DB/SSH) **smoked** 2026-09-01
   (`browser` debug). `target=_blank` focuses; ⌘-click stays background.
   **`alert` / `confirm` / `prompt`** (and leave-page) are a kit modal over
   the page — CEF OSR has no native JS dialogs. Wrapper uses the same overlay.
   Confirm Continue no longer panics the helper (`RefCell` held across CEF
   re-entry). Omnibox is **idle text** until click / `⌘L` (no query string
   while idle; clipped so a long path cannot cover vault/downloads). Typing
   searches visit history (top 5 tray; ↓ / `Ctrl+N` or a click highlights
   a row — the first match is not taken; Enter = that row, otherwise the
   typed URL or Kagi **lucky**, or **switch to an open tab** for a URL;
   Shift+Enter = Kagi results). Idle-bar ⌘V pastes into the **page**. All three
   **smoked** (`browser` release). **Find**
   `⌘F` / next `⌘G` / previous `⌘⇧G`. **⌘⇧R** hard-reloads (bypass HTTP
   cache for this load; Shift-click Reload). New Group is **`⌘⌥G`**. Developer
   Tools docks under the page (**smoked**; brief black flash on open/close).
   **Installed** `browser` (release, 2026-09-04).
   Freeze
   [`docs/specs/2026-08-28-sola-browser-vault-panel-design.md`](docs/specs/2026-08-28-sola-browser-vault-panel-design.md),
   item edit
   [`docs/specs/2026-09-11-sola-browser-vault-item-edit-design.md`](docs/specs/2026-09-11-sola-browser-vault-item-edit-design.md),
   and tab groups
   [`docs/specs/2026-08-15-sola-browser-tab-groups-design.md`](docs/specs/2026-08-15-sola-browser-tab-groups-design.md).
   Pocket color was `kit` + `browser --release` 2026-08-31.
   Vault stays unlocked across restarts; **Log out** on the card wipes
   the session. HTTP Basic/Digest **Sign in** is persisted per profile.
   Install/re-exec **Shutdown**-flushes helpers (cookies) instead of
   SIGTERM; a dead helper no longer restores closed tabs.
   Google “Verifying it’s you” passkey: intercept is **document-start**
   (V8 eval) and advertises a platform authenticator so Bitwarden’s
   picker opens instead of Chromium’s OSR-invisible sheet.
   Vault item **Edit** on the record (all shown types; org when writable;
   passkeys stay; no delete) — **installed** `browser` release 2026-09-11;
   PUT uses Bitwarden-Client-Version `2026.8.0` + blob encrypt (**installed**
   2026-09-12). That install left security-state at 2, so decrypt dropped
   login URIs with no checksum and the current-page Autofill list stayed
   empty (search by name still found the item). Reads stay at 1; blob
   encrypt is held only across the write. **Installed** `browser` release
   2026-09-30, unsmoked. A Save made while the list was empty may have written the
   item back without its websites — those need the URI added again.
   Profile switch does not copy the previous helper’s tabs
   (**installed** 2026-09-12).
   HTML5 OSR drag (Shortcut kanban tickets = pragmatic-dnd Native;
   replica `crates/sola-browser/assets/kanban-dnd.html`) **dogfood
   good** 2026-09-14: unique last_frame, overlay 3-slot ring, skipped-frame
   dirty absorb, GPU uploads overlay dirty. Remaining CPU-OSR cost
   parked
   [`docs/ideas/2026-09-14-osr-cpu-drag-mitigations.md`](docs/ideas/2026-09-14-osr-cpu-drag-mitigations.md)
   (ghost shader quad, overlay restore-ghost, `read_frame` recycle,
   wgpu pad check, credits — not `accelerated_osr`, not a 16 ms timer).
   **Next:** vault item edit Save; Google passkey picker; Find + JS
   dialogs after the Continue fix; ⌘⇧R hard reload; vault stay-logged-in;
   HTTP auth persist; group recolor + Steam store trailer.
   Gaps: Google passkey picker unsmoked; DevTools open/close flash; Find unsmoked; confirm-after-fix unsmoked; SVG-only favicons stay globe; vault desk smoke (Edit Save after client-version fix); notify no sound / actions / D-Bus.
4. **Shell Bluetooth + volume** (on master) —
   Bluetooth freeze
   [`docs/specs/2026-08-29-shell-bluetooth-menubar-design.md`](docs/specs/2026-08-29-shell-bluetooth-menubar-design.md);
   volume freeze
   [`docs/specs/2026-08-29-shell-audio-menubar-design.md`](docs/specs/2026-08-29-shell-audio-menubar-design.md).
   Nearby inquiry omits 6-pair hex addresses (`AA-BB-…`). Volume chip **right of
   Bluetooth** (`Panel::Audio`): 12-band LED spectrum is the chip (no speaker
   glyph; click the bars). FFT of the monitor; phosphor stack; ~3× the CPU
   graph; phrase gap on both sides. Popover slider + mute + output/input
   device pick (`pw-dump` / `wpctl` /
   `pw-cat`). Media keys still `solactl media`. Chip highlight tracks
   `menu_open`. Notifications pile: bell + count, no Clear; grouped by app.
   CPU/GPU/MEM/RX/TX are pixel graphs. App-menu dropdowns use
   laid-out label X. **Installed** `shell` release 2026-09-02. WH-CH520 paired on the desk.
   Volume chip no longer hides when `pw-dump` is not JSON (MIDI Through
   unnamed SPA arrays) — **installed** `shell` release 2026-09-08 (unsmoked).
   Spectrum **desk-smoked** on Spotify (presence-band ceiling, peak-hold
   autoscale, pink tilt, punch gate). Phrase gap on both sides of the bars;
   shell re-exec keeps app z-order (**installed** `river`+`shell` release
   2026-09-02). Notify pile is grouped by app (no cap 20; ≤4 list as rows
   so the bell matches; 5+ collapse; same-tag replace). Super+Tab shows a
   count mark (unseen live + pile; Mail unread). A pointer already resting
   on an icon does not take that selection until it moves, so releasing
   Super still raises the previous app — **installed** `shell` release
   2026-10-08, unsmoked.
   Opening the pile or
   focusing the app clears the mark without draining the pile. **Installed**
   `kit`+`shell` release 2026-09-03. Flower **Restart Computer** / **Shut Down**
   (logind; no confirm; not in Super+K) — **installed** `shell` release
   2026-09-03. **Next:** desk-smoke grouped pile +
   switcher marks + those power items;
   volume keys vs chip, sink/source switch, dashed-MAC nearby filter.
5. **GPU idle** — living track:
   [`PERFORMANCE.md`](PERFORMANCE.md)
   (architecture regression table + capabilities row `gpu-idle`).
   Desk sample 2026-08-25: **mean 17.7%** at P8 / 21 W (was ~30–40%).
   Opaque-region **not fully smoked** (session apps did not re-exec).
   **Next:** restart those windows and re-measure, then River NVIDIA knobs.
6. **kit Morph2 sidebar** — frozen. sola-browser tab strip: groups and
   loose tabs **intermix** (no groups-on-top). **Installed** `kit` + `browser`.
7. **sola-terminal** — **partial** (on **master**; `terminal-polish` merged).
   Grid selection is kit neon `accent` (`#3dd6f5` @ 55%), not the graphite
   `selection` atom. Wash is glued to buffer cells and re-anchors when a
   TUI CUP-rewrites the live grid (`sel_follow`). Workspaces PTYs share
   the palette and the follow. **Installed** `terminal` (2026-09-02);
   Window menu **installed** 2026-08-31. First tab / no-source pane
   starts in `$HOME`.
8. **sola-monitor** — **partial** (on **master**; `monitor-polish` merged).
   Bus + Call inspector on kit chrome (left plane rail, `list_item` log,
   inspector well, last-known stickies / live owners). Call traffic via
   `Role::Observer` + `Wire::Trace` (not RPC on the bus). Kit JSON
   highlighter. **Installed** `call` + `monitor` (debug, 2026-08-21).
   Desk smoke pending. GPU menubar ranking (SM % + VRAM) also lands.
9. **sola-mail** — **partial** (**rework on master** 2026-10-19, merged
   from `sola-mail`; **installed** `mail` release 2026-10-19 (through `40db4486` — refused recipient named, failures logged; 3 accounts connected)).
   **This slice (rework):** coordinator worker + per-account read / write
   / IDLE sessions (3 logins, was 4 + keepalive); Settings edits reconnect
   only accounts whose IMAP login changed (rules-only edits keep every
   session; IDLE stops by shutting its socket — no more up-to-30 s stall
   per account); failed connects retry 15 s → 5 min + **Retry Now**.
   Bugs fixed: typing in Search no longer fires `d` / `a` / `x`; search
   hits open / mark / move in **their own** mailbox (was the sidebar's —
   could trash the wrong letter); list subjects / names RFC 2047-decoded;
   letter time honours the sender's offset; IMAP panic filter installed
   once (the per-call global hook swap could silence the log hook);
   refresh merges by date (cross-account UIDs broke the load-more tail);
   `UID EXPUNGE` with UIDPLUS; Gmail Sent not appended twice; sent mail
   has a Message-ID; replies send `<…>` In-Reply-To + References.
   New: **Forward** (with files), Reply-To, reply-all keeps To / Cc,
   **Save Draft** + Edit draft, discard confirm (Esc / Cancel / clicking
   another row), erase confirm, mark read/unread (⌘⇧U), flag (⌘⇧L),
   Move to Inbox button, relative list dates, account tags, Sent / Drafts
   show recipients, Sending… state. UI split into list / reader / compose
   / moves / sidebar / chrome / keys. Kit `prose` reuses shaped
   paragraphs. Earlier slices: Mail.app list, snapshot boot + connect
   card, multi-IMAP canonical boxes, attachments, send identities,
   focus re-list + Checking mail ring.
   HTML `<hN>` now render as prose headings (were `### ` text; Forward
   quoted them).
   Compose follow-up (desk report: tiny `to@` / `cc@` fields, no Tab,
   clicks seemed ignored): real fields + focus well, Tab order, address
   suggestions, per-address check on Send, centred discard card, toolbar
   live while composing, slow update / view logging (no stall found in
   the log; the UI thread was idle — likely the greyed toolbar + the
   bottom-of-pane discard bar).
   Send failures: a recipient the server refuses at RCPT is named
   (`<addr> was refused by the mail server — 5.1.2 Mailbox does not
   exist.`); send / open / flag / draft failures are logged at warn.
   (Desk report 2026-10-19: `terry@hotsprings.mt` has no mailbox on
   niarada.co's Stalwart — not a Sola bug.)
   **Next:** desk-smoke the freeze
   checklist items 11–17 (list dates / tags / decoded subjects and Forward
   seen working).
   **Install:** standing OK to `install mail` after each finished round
   (focus check + checking ring **installed** `mail` release 2026-10-11,
   unsmoked). Self-restarts.
10. **Project site (sola.moonlight.pm)** — source in [`site/`](site/)
   (moved from Thoxa's `workloads/sola` 2026-10-19; a Thoxa program, built
   with `--build-context thoxa=../Thoxa`). Wicket aulos workload `sola`
   (`wl-f2835fc5634e`, image `sola-landing`) now routes
   **https://sola.moonlight.pm/** (verified 200, LAN and public).
   `sola.computer` **parked**: route dropped, apex A record deleted in
   Cloudflare (`*.sola.computer` kept for `bot.sola.computer`). **Live
   2026-10-19:** a plain project page in the Thoxa/Oath shape (Design,
   Applications, Architecture, Status, Building; bar = Manual · Source) and
   **`/manual`** — `docs/manual` rendered as one numbered page (order in
   [`docs/manual/README.md`](docs/manual/README.md)). **Reworked
   2026-10-19, live:** tighter type and spacing on the
   Thoxa/Oath frame, the foundation's desk drawing on the home page, and a
   manual about the system — Introduction, Architecture, Process manager,
   The bus, The call plane, Compositor, Session manager, Shell, Theme and
   toolkit, a complete solactl reference, and an Applications list (app
   chapters stay in the repo as notes). Deploy is `registry push` + `workload apply` (re-pulls
   `:latest`); `workload roll` sits in `pending` on aulos. Messaging: [`docs/marketing/PRODUCT.md`](docs/marketing/PRODUCT.md).
   **Gaps:** no install image to link; `site/web/desk.png` is re-rendered
   by hand when the foundation desk changes. Writing the chapters found
   code bugs and a security hole; see the security item under **Now**.
   Root [`PRODUCT.md`](PRODUCT.md) remains the **desktop** product truth —
   do not overwrite it with site messaging.
11. **sola-workspaces** — **partial** (on master); **renamed Projects on `next`** (`sola-projects`, `solactl projects`, a workspace is a tab: `tab.spawn` / `tab.rm` / `--tab`) — the rest of this entry uses master's names  
   **Freeze:** [`docs/specs/2026-08-13-sola-agent-terminal-design.md`](docs/specs/2026-08-13-sola-agent-terminal-design.md)  
   **Call plane:** [`docs/specs/2026-08-13-sola-call-plane-design.md`](docs/specs/2026-08-13-sola-call-plane-design.md)  
   **Product:** [`crates/sola-projects/PRODUCT.md`](crates/sola-projects/PRODUCT.md) (on master `crates/sola-workspaces/`)  
   **CLI freeze:** [`docs/specs/2026-08-18-workspaces-cli-design.md`](docs/specs/2026-08-18-workspaces-cli-design.md)  
   **Next:** desk-smoke `solactl workspaces` and grok `-r` after reboot. Polish:
   project recolor. Rail rename / reorder **installed** `kit`+`workspaces`
   release (unsmoked): hover pencil / double-click is `workspace.set --title`
   (rail is that label, or the slug when empty). Drag tabs stay in the
   project; drag group headers; `--before` / `project.reorder`. CLI
   `workspace.set --name` moves `.worktrees/<name>` (id stays);
   `--branch` is `git branch -m`.
   **Installed** `workspaces` release 2026-09-10 (TUI selection follow; split-leaf Ctrl-D; Codex first-class).
   **This slice:** rail `×N` for Codex (rollout `type: compacted`; Grok
   path unchanged) — **installed** `workspaces` release 2026-09-21 (unsmoked). 2026-09-17: rail label is `--title` or the worktree slug; pencil /
   double-click rename; drag-reorder tabs in a project and project groups
   (`workspace.set --before`, `project.reorder`). Rename typing does not
   echo into the PTY. **Installed** `kit`+`workspaces` release 2026-09-17
   (unsmoked). 2026-09-11: pane/window tmux `-t` is `=session:` so split
   tabs `pane.send` / `pane.read` / `workspace.exec` (`pane.read --pane
   ws-sc-17973` after a split).  
   **Do not invent:** call-plane **D3** confirm. D4.4 is closed:
   Claude is first-class (`--agent claude`, hooks merged into
   `~/.claude/settings.json`, rail rollup, `×N` from `compact_boundary`,
   reboot `claude --resume`). **Installed** `workspaces` release 2026-10-08, unsmoked.
   Codex is first-class (`--agent codex`, hooks merged into
   `~/.codex/hooks.json`, rail rollup). First Codex session may need
   `/hooks` to trust the Sola status hook. OpenCode stays
   presence-only.  
   **Install:** standing OK to `install workspaces` after each finished
   round. Ask for any other target.  
   **Now:** persist + spawn + done toast. Crate/app id `sola-workspaces`.
   Methods on sola-call owner `workspaces` (`solactl workspaces ps` / `workspace.spawn` /
   `workspace.set` / `workspace.exec` / `pane.wait` / `whoami` / …). Control plane is
   first-class: verb changes update `calls.rs` + dispatch + tests +
   `docs/manual/solactl.md` together.
   Config `~/.config/sola/workspaces/` (migrates `agent-terminal/`). Tmux
   `sola-ws` / `sws-`. App installed and dogfooded (rail, splits,
   drop-project, dead-pane, `×N`). `solactl workspaces` implemented (richer
   payloads, `--prompt-file`, `project.add`, `workspace.select` /
   `workspace.exec`, `pane.wait`, `whoami`; Grok-leaf targeting, Codex
   leaf when `--agent codex`; parent from `$SOLA_PANE_ID`) — **desk smoke pending**. CLI
   `workspace.spawn` is background unless `--select` (UI + / ⌘T still
   jump). Skill `sola-workspaces-cli`: “review/work ticket”, “create
   worktree”, “tell that grok” → spawn/exec, never steal the rail.
   Per-project
   startup script after sibling spawn (**Project → Startup Script…** /
   `project.startup`). Rail: Add project expands `~`;
   groups stack at the top; no grok/agent label on the row. Sibling
   hover close is the kit ×; root has no row close (Drop Project is menu-only).
   Launcher builtin **Workspaces** is in shell (`lucide/folders`).
   Shortcuts: ⌘T spawn, ⌘N new project, ⌘⇧↓ split down, ⌘⇧→ split
   right, ⌘W close pane. A workspace is one rail row even when split;
   the mark rolls up every Grok or Codex pane (waiting / needs-attention beats
   working beats done beats idle). Last pane close keeps the workspace.
   Dead last pane shows **Start new shell**; a split leaf that exits
   retracts. Quiet `×N` on the workspace row is the loudest Grok or
   Codex session in the tab (Grok: segments / checkpoints; `signals.json`
   can stay 0. Codex: rollout jsonl `type: compacted`). Switching a split attaches every leaf; hover does not spawn. Restart binds tmux by `SOLA_WS_PATH` / cwd
   — leftover sessions from a deleted workspace are quarantined, not
   attached to the next tab. A live Grok pane’s session id is persisted;
   after a reboot (tmux gone) the tab execs `grok -r <id>` when that
   session is still on disk (`/exit` to a shell does not resume). Working ring spins (kit mark uses ms
   phase, not `as_secs_f32`). Rail marks reclaim on Grok
   `SessionStart` / `UserPromptSubmit` after `/new` or `grok -r`
   (was frozen on the old session). `StopCancelled` → done.
   Super-chord no longer latches LOGO: ⌘T / ⌘N / ⌘V used to swallow
   every later key until quit (River eats Super-up; the union was
   written back into `keyboard_mods`). Exiting Grok back to the shell
   idles the rail mark (grey disc; SessionEnd used to leave done).
   `workspace.rm` replies before it kills tmux (a pane closing itself
   no longer hangs `solactl` / leaves the working ring). `--worktree`
   also `git worktree remove`s after the tab closes — do not
   `git worktree remove` from inside the pane first (cwd dies; the tab
   used to stick with a working spinner). A gone `.worktrees/<name>`
   path reaps the tab. Hover × / plain `workspace.rm` still leave the
   checkout. `pane.list` / `whoami` include `session_id`. `pane.send` /
   `workspace.exec --prompt` bracketed-paste into the tmux session then
   Enter (raw PTY dump was clipping the composer and not submitting
   until a later wheel/focus). Desk cards (unfocused): title is
   `{project} · {tab}`, body is `grok is done` / `codex is done` /
   `needs attention`.
   Grid selection follows TUI CUP-rewrites (Grok/Codex) while dragging
   and after a page of wheel — remaining lines stay highlighted when
   the top of the range scrolls off. Ctrl-D on a split leaf
   retracts that pane only — tmux session `-t` is exact `=session` so
   `sws-ws-foo` cannot prefix-kill `sws-ws-foo-p`. Pane/window commands
   use `=session:` (installed 2026-09-11).  
12. **sola-paint** — default MIME / `solactl open` dest; crop / rotate /
   flip / save; left tabs; kit `FilePicker`; **single-instance** (second
   spawn hands off); **zoom/pan**. Crop is **⌘⇧K** (⌘K is Super+K overlay).
   Screenshots stay on **preview**.
   Stage cache + off-thread decode; tabs persist via `PaintSession`.
   Reinstall `paint` to dogfood. Gaps: no clipboard image.
13. **Call plane** — on **master**. Host + `solactl compositor` / `session` +
   kit helper + `Role::Observer`. Workspaces registers owner `workspaces`
   (desk smoke pending). **D3** (confirm gates) is open. Catalog sticky
   on the bus still later (monitor observes the call socket instead).  
14. **sola-arcade / windowed gamescope** — **partial, dogfoodable** (on master)  
   Per-title nest: **Fit to window** or a locked resolution (default **1080p**).
   Fit follows the gamescope host frame on zone/float (nested mode-control +
   focused window at 0,0). Keep the game **fullscreen** for Fit. Nest passes
   `--cursor-scale-height` (desktop-sized host pointer; Factorio was huge).
   A–Z / Recent sort persists (`arcade-prefs.json`).  
   **This slice (2026-09-14):** nested-steam treats Steam **install-script**
   reapers (`AppId=<id> Install=1`, `iscriptevaluator`) as prepare, not the
   game — Palworld-class Proton titles no longer “crash” when VC redist
   exits. **Next:** desk-smoke Palworld from Arcade (quit desktop Steam
   first; first Play may still show Steam’s launch-option picker in the
   nest: Play Palworld vs DirectX 12).  
   **Install:** standing OK to `install arcade` after each finished round.  
   - Fit rezone dogfooded (Factorio, fullscreen on).  
   - Backlog: Portal-class nest fails; residual flicker; title contrast;
     never-played owned without API; VDF/decode bounds (see
     [`docs/ideas/2026-09-01-omakade-arcade.md`](docs/ideas/2026-09-01-omakade-arcade.md)).
15. **Distribution follow-through** (when resumed)** — ISO e2e, TZ, tarball.
16. **sola-calendar** — first pass **on master**. Kit month / week / day, local **On This Computer**. Accounts are **Settings → Calendar** only (`Topic::CalendarConfig`): multiple Google, iCloud, CalDAV, and ICS/webcal URLs. Google uses the shipped Desktop OAuth client + secret. Month: date at top-start, full weekday names, events fill the cell (no “+N more” while they fit); sidebar is a kit split (calendars + Today/Tomorrow quickview; no Accounts list); inspector always open on a split (no Close); event editor is a caption-row form (title, when, calendar disc, location, notes); HTML/URL locations are clickable; aliases and colour picker (original name only while editing); hide from sidebar also deactivates (re-show / label / colour in Settings). Freeze [`docs/specs/2026-09-08-sola-calendar-design.md`](docs/specs/2026-09-08-sola-calendar-design.md). **Installed** `bus`+`settings`+`calendar` release 2026-09-08 (unsmoked). Reminders: inspector **Remind me**, several quick sets, local `reminders.json`, desk card stays until dismissed (`sola-persist:`). **Smoked** `shell`+`calendar` release 2026-10-12. Gaps: no invites; no provider alarm sync; Calendar must be running; iCloud/CalDAV series edit; URL feeds read-only.
17. **Follow-ups (unordered backlog):** Host `video` extraGroup (uaccess already RW on `/dev/video*`); CEF PipeWire camera needs `rtc_use_pipewire` (build has `use_sysroot=false`). IPC contract handshake
   ([idea](docs/ideas/2026-08-30-ipc-contract-compat.md) — mixed worktree
   installs; not Now); create-card; vault item delete;
   float chrome, D2, preview, kvm clipboard, switcher FFM holdoff
   (`naturalethic/switcher-ffm-holdoff` unmerged); spotify podcasts / tray /
   playlist reorder/delete. Browser: org-vault fill
   **desk smoke**; hover × follows the pointer after close; scheme-less
   localhost / loopback is `http://`. Agent tab/group/page control
   (`solactl browser`, same strip, Agent pocket by skill —
   [freeze](docs/specs/2026-09-11-sola-browser-agent-control-design.md);
   implemented, desk-smoked on Tertius; CSS-pixel click/hover +
   `key --chord` + `SOLA_BOT=1` guards **in code**, not installed).
   Passkey **create** smoked. Page
   menu DevTools / Inspect Element. HTML5 OSR drag: host ghost is
   dirty-rect only (not a full-page clone); incomplete CPU paints no
   longer leave hover-reveal holes; pointer-up outside the page /
   Escape / leave-window cancel — **installed** `browser` release, desk smoke pending.
   Tab-group pockets
   **installed**: flush members, hairline rim, header drag moves the
   pocket, title drop is invalid, hole matches etch row height. Loose
   is a real section (no box). Extra opens only on the well under the
   ghost. **⌘G** new group **installed** 2026-08-29. Pocket color (edit-mode swatch) **installed** `kit`+`browser --release` 2026-08-31.

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
| Bus / UI | sticky `~/.config/sola/state.yaml`; Iced + kit | Same stack inside guest when installed |
| Dist path | Shape 1 colleague module (`INSTALL.md`); tarball v0.1.1 **404**. From-source: `CONTRIBUTING.md` (`installRelease = false`) | QEMU **vdb** install → loginless Sola **OK**; **ISO e2e pending** |
| Branch | **master** (workspaces + wrapper + **scope** + **spotify** first pass). Feature work in worktrees | Feature work in worktrees / Orca workspaces |
| Scope | **On master.** Pixel loupe: live follow, zoom 65×65…3×3, hex copy, remembered float. Desktop pointer visible; grid omits the sprite (patched wlroots + River). Installed `scope`+`river`+`shell`; compositor `/opt/sola/bin/river`. | — |
| Shell | Volume chip no longer hides on invalid `pw-dump` JSON — **installed** `shell` release 2026-09-08 (unsmoked). Flower **Restart Computer** / **Shut Down** (logind; **installed** `shell` release 2026-09-03, unsmoked). Number pad types digits (NumLock on by default) **installed** `river` release 2026-09-01; **smoked**. Super+Shift+4 freeze-then-crop (no dim) + `--app` toplevel capture (no raise) **installed** `river`+`shell` debug 2026-08-31 (`sola-arcade` smoke). Super+Shift+3/4/5 → clipboard (promised `image/png`, Fastest encode) **installed** `kit`+`shell` **release** 2026-09-01. Super+Shift+4 keeps menubar panels in the freeze (2026-09-02; **installed** `shell` release). Volume chip no longer stays filled after dismiss; notify pile is bell + count, grouped by app (no cap 20, no Clear; ≤4 list as rows; 5+ collapse; same-tag replace) — **installed** `kit`+`shell` release 2026-09-03. Super+Tab count mark (unseen live+pile; Mail unread; bell/focus acks). Volume chip 12-band LED spectrum (default-sink FFT; own phrase; punch gate) **installed** `river`+`shell` release 2026-09-02. Shell re-exec keeps app z-order. Right-cluster phrases (extras · spectrum · percents · rates · clock) **installed** `shell` release 2026-09-02. Stat chips are btop-style pixel graphs (numbers in the dropdown; 2px optical lift) **installed** `shell` release 2026-09-02. App-menu dropdowns measure label X (were drifting right) **installed** `shell` release 2026-09-02. Preview **Copy** **installed** `kit`+`preview`+`browser`+`wrapper` debug 2026-09-01. Super+H hide. Dead-pid prune; menubar remapped. Window menu + Super+K overlay **installed** `kit`+`shell`+`paint`+`terminal` debug 2026-08-31. Super+K live Frame is card-sized (**installed** `shell` release 2026-09-02). Float CSD rounded-face clip **installed** `kit` release 2026-09-02 (storybook; other float apps need their own install). | — |
| Paint | Installed first-pass; singleton + zoom/pan need `install paint`. Shell hotkeys copy screenshots to the clipboard (not Preview). | — |
| Browser | 2026-10-08: location bar Enter loads the typed URL; a history row loads only after it is highlighted or clicked (the first match is not taken). The bar keeps that URL until the engine leaves the previous page. Same-site Enter (different path loads in this tab). **Installed** `browser` release, unsmoked. 2026-09-30: current-page Bitwarden Autofill matches checksum-less login URIs again (security-state 1 on decrypt; blob encrypt only during the personal-cipher write) — **installed** `browser` release, unsmoked. Items saved while the list was empty may have lost their websites on the server. 2026-09-23: a page title ending in NUL (WA Digital Archives download, `magick-tg-…`) no longer aborts `set_title` — **installed** `browser` release, unsmoked. One chrome window + per-profile `--engine` helpers; instant Profiles switch; parked last-frames; omnibox load line; **copy URL** (left of field; committed page URL; check flash — **installed**); outside open **raises** the window (**installed** browser+shell); scheme-less localhost / loopback uses http; instant tab close (hover × opaque chip; follows pointer after close); **drag-reorder tabs** + width-aware titles (dogfooded); **tab groups** (kit inset pocket, flush members, hairline rim; header drag moves the pocket; title drop ignored; **⌘⌥G** new group on a loose tab, name focused+selected; hover pencil to rename; edit-mode color swatch + check; luminance ink; selected lift + 1px lip on pockets **and** loose tabs — persist hex, default well; **installed** `kit`+`browser --release` 2026-08-31; strip has no right-click); **⌘V once** (focused-frame JS, not all-frames); **⌘-click** dogfooded (IMDb): Super+drag bindings **removed** (CSD titlebar still moves floats); JS href → chrome background tab **below current** (same group); tab-strip favicons **smoked** 2026-09-01 (`browser` debug; SVG-only sites stay globe); `window.open` / `NEW_POPUP` → focused tab beside opener **smoked** (Cloudways DB/SSH); ⌘T / xdg-open / `solactl open` append **loose at the bottom**. Super+Tab untouched. **page context menu** (kit; cancels empty CEF OSR strip); **hold back/forward** for session history; YouTube persists after quit; Bitwarden **unified vault** (one toolbar icon — lock / key / shield when this page has TOTP / fingerprint on passkey; search + type chips + item records; **Create login** via **+**; fill/cards/identities/TOTP/passkeys decrypt **org vaults** too — **installed** `browser` --release 2026-08-28, desk smoke next; create still personal; item **Edit** on the record — **installed** `browser` release 2026-09-11, unsmoked); **downloads** (auto-save `~/Downloads`; toolbar icon + progress; flat panel; persist `shared/downloads.json`; dogfooded); unlock lifts the vault icon, accent = open panel; page ⌘C/⌘V + triple-click; passkey **get** (Google + **Gemini Exchange 2FA**; all-frames intercept; same-site coalesce — dogfooded); passkey **create** (vault confirm; new login or attach — **smoked**); OSR IME + Shift+wheel + `<select>`; **default web open** via sola-browser (`sola-browser.desktop` claims http(s)+HTML+XHTML+PDF+about+unknown; no Helium fallback). Relative HTML/PDF `open` is absolute `file://` (not `https://apocrypha/…`) — **installed** `browser`+`solactl` 2026-09-02. This desk: `xdg-mime` for those types → `sola-browser.desktop`; **single iced chrome** (`chrome.sock` handoff; second process does not reap helpers); tab strip no phantom `↓ N` chip. **⌘⇧T** reopens the most recently closed tab (stack of 25, per profile; **installed** `browser` 2026-08-28). **Notifications:** KenHerbert Allow → displayed + top-right desk card (wrap fix installed 2026-08-27). Steam store autoplay: codecs CEF + `--autoplay-policy` **installed** 2026-08-30 (`canPlayType` AAC/H.264 `probably`; reload trailer). JS `alert`/`confirm`/`prompt` kit modal; idle omnibox omits query strings; visit-history tray; Enter = Kagi lucky / Shift+Enter = SERP; idle-bar ⌘V → page **smoked**; Find `⌘F`/`⌘G`; DevTools docked **smoked** 2026-09-03. **⌘⇧R** hard reload (CEF ignore-cache; unsmoked). Vault stays unlocked across restarts; **Log out** wipes session (unsmoked). HTTP Basic/Digest Sign in persists per profile (unsmoked). Google passkey intercept is **document-start** V8 eval + platform-authenticator advertisement (unsmoked). Helper start **Shutdown**-flushes cookies; a dead helper restores chrome’s tab list (no closed-tab resurrect). **Installed** `browser` release 2026-09-04. New Group is `⌘⌥G`. Number pad digits in the page (OSR `VK_NUMPAD*`) **installed** `browser` release 2026-09-05, unsmoked. **`solactl browser`** smoked 2026-09-11 (Tertius: background open / `wait --load` / YAML refs / background click-nav / httpbin fill+radio; seat stayed). Profile switch does not merge the previous helper’s tabs into the next session (**installed** 2026-09-12). Vault Save/passkey PUT advertises Bitwarden-Client-Version `2026.8.0` and blob-encrypts personal items (**installed** 2026-09-12; crate `0.1.0` was 400 “update latest version”). 2026-09-14: CPU OSR HTML5 kanban drag **dogfood good** (unique last_frame, overlay ring, damage continuity, partial GPU; replica `assets/kanban-dnd.html`; remaining mitigations parked). 2026-09-16: page paste keeps newlines (synthetic `ClipboardEvent`); `solactl browser` snapshot `actions:` + `click --text`; large downloads no longer stall the helper (no per-chunk URL copy, skip leftover `.crdownload`, no parallel ranges). **Installed** `browser` release, unsmoked. | — |
| Monitor | **On master** (installed debug 2026-08-21): Bus/Call inspector, kit chrome, call observer. Desk smoke pending. GPU panel SM%/VRAM ranking lands with this merge | — |
| Mail | Rework **on master** 2026-10-19 (merged from `sola-mail`): **installed** `mail` release 2026-10-19 (through `40db4486` — refused recipient named, failures logged; 3 accounts connected); list dates, account tags, decoded subjects and Forward seen on the desk. **Installed** `mail` release 2026-10-11 (unsmoked): focusing the window re-lists the open folder; a working ring beside the count (hover: Checking mail). **Installed** `mail` release 2026-09-10: letter Date + local `HH:MM`; junk is `x`. **Installed** `mail`+`settings` release 2026-09-04: multi-IMAP canonical boxes; Gmail Archive create; parsed-date list; async MOVE; snapshot boot + connect card; Wicket From checklist; Workspace MX fill; attachments send/receive. Standing OK to reinstall `mail`. No HTML engine | — |
| Terminal | **On this branch** (installed 2026-09-02; lib via Workspaces **installed** 2026-09-11 pane `=session:` / 2026-09-10 TUI follow): grid selection is neon accent wash (`#3dd6f5` @ 55%). Wash follows scrolled glyphs (`sel_follow`) and TUI CUP-rewrites during drag and page-sized Grok/Codex wheels. Workspaces PTYs share the palette and the follow (**installed** `workspaces` 2026-09-02). First tab / no-source pane starts in `$HOME`. **2026-10-03 installed** `workspaces` release, unsmoked: the wheel scrolls the alternate-screen buffer. `smcup` is forwarded and a reload clears the old `smcup@` override. Activity for a Codex app-server session follows the rollout. RimWorld `codex` stayed up across the restart. | — |
| Wrapper | **On master.** Slack paints. Edit, off-site links, desk notifications, huddle OSR + mic **smoked** 2026-08-29 (`wrapper` debug). LifeCam Cinema huddle camera **smoked** 2026-09-01 (V4L2 `getUserMedia`; mic is the same USB device as the volume-chip default source). Image paste (Preview **Copy** → ⌘V) **installed** `kit`+`preview`+`browser`+`wrapper` debug 2026-09-01. JS dialog overlay on master. | — |
| Arcade | **Installed** 2026-08-25; 2026-09-01 watch / singleton / refuse-Steam-open / narrow Stop **installed** `arcade`. 2026-09-14: nest ignores `Install=1` / `iscriptevaluator` reapers (Palworld prepare was tearing the nest down) — **installed** `arcade` this slice, Palworld desk smoke pending (quit Steam first). Banner list + nest dogfooded (Core Keeper, PEAK, Factorio); per-title **Fit / resolution** (default 1080p); live Fit follow dogfooded (Factorio rezone, fullscreen on). Standing OK to reinstall after each arcade update | — |
| Calendar | **On master.** **Installed** `bus`+`settings`+`calendar` release 2026-09-08 (unsmoked). Reminders **smoked** `shell`+`calendar` 2026-10-12. Settings accounts: Google / iCloud / CalDAV / URL, multiple. | — |
| Spotify | 2026-10-19: Web API budget slice + sign-in without `/me` **on master**, **installed** `spotify` release (see Now). **Installed** `spotify` release 2026-09-04: artist/album names on a track (list, queue, player bar) open those pages; player-bar cover opens the album; album **Save**/**Follow** and artist **Follow** land on the Albums / Artists tabs. A `/me` 429 no longer kicks the session to sign-in; profile is not re-polled every 2s while missing (5 min backoff). Device poll is 30s and skips while cooling. After Connect marks this computer inactive (or the session drops), a song click **activates then loads**; reconnect waits for the old Spirc to finish so it cannot delete the new device. MPRIS follows the local engine. Earlier: Back/Forward **survives restart** (max 20 back steps); **list-plus** add-to-playlist picker + **New playlist**; media keys apply immediately; page Back/Forward. Launcher row live. Made for you catalog; likes from `liked.json`; **−** skip-once; hide strike+dim; last page + last track. Host NixOS has `alsa-lib` + `libpulseaudio`. Standing OK to `install spotify` after each build. | — |
| Agent | **Retired** (on master). Crate gone; launcher has no **Agent**; settings no longer treats `sola-agent` as a system app. Leftover `/opt/sola/bin/sola-agent` from 2026-08-25 bulk until a later install. Daily agent work is Workspaces. | — |
| Bots | **This worktree.** **Installed** `bots` release 2026-09-21: write-fence escape in the foundation prompt (default home-only; Joshua can order an escape). HTTP SSE `:27419`; iced + iOS clients. Existing sessions adopt AGENTS.md on the next send. ACP dies on `cargo make install bots`. | — |
| Nest paint | wayland+`-b`+`-S fit`; **no `-e`**; `--nested-steam` (no BPM); `-w/-h` from per-title nest; Fit live-pokes nested X only (never host `:0`) | — |

**Install policy:** agents never run `cargo make install` without explicit
permission for that install — except standing OK for **arcade**,
**workspaces** (`projects` on `next`), and **mail** after each finished round of those apps,
**spotify** after each build, and **browser** after each finished round
on this worktree (OSR drag). User smokes.

**Useful:**

```bash
cargo make build                   # release (default)
cargo make install kit shell       # only with your OK; release
cargo make install kit shell --debug
RUST_LOG=debug /opt/sola/bin/sola 2>&1 | tee /opt/sola/log/sola.log
```

---

## Locked models (do not re-litigate)

| Topic | Rule |
|-------|------|
| UI stack | **Iced + sola-kit** only for new apps; WebView host is apocrypha |
| Compositor | **River** external; **sola-river** is the bus ↔ Wayland bridge |
| IPC | **Sola Bus** (fan-out) + **sola-call** (request/reply) + Wayland for surfaces/input |
| D-Bus | **Deprecated as a requirement** (2026-10-18). No new D-Bus dependencies. Existing uses ([inventory](docs/architecture.md#d-bus-deprecated)) are legacy; the future target is Sola's own replacement on Sola Bus. Shape open: [T3](docs/open-questions.md#t3--d-bus-replacement-on-sola-bus-p2). |
| Process model | Multi-process; each app independently restartable |
| Theme | Bus `Topic::Theme` + kit semantic tokens/fonts; shell chrome tokens |
| Browser | **CEF** in single `sola-browser` crate; no `accelerated_osr`; WPE path retired |
| Browser agent control | Agent lives in **Projects** (was Workspaces), not in the browser. Face: `solactl browser` (owner `browser`). Same strip / profile; **Agent** pocket is a skill convention. Page loop: pruned a11y **YAML** + opaque refs; CSS-pixel `--x/--y` click/hover and `key --chord` on `--tab` when the tree is empty (CEF, not compositor). `SOLA_BOT=1` never `--select` / `tab.focus` and must pass `--tab`. Confirm still **D3**. Freeze [`docs/specs/2026-09-11-sola-browser-agent-control-design.md`](docs/specs/2026-09-11-sola-browser-agent-control-design.md). |
| Wrapper | **`sola-wrapper <id>`**; `app_id` is the configured id; per-id CEF profile under `~/.config/sola/wrapper/<id>/`; Applications catalog (`kind` + `url`); not sola-browser chrome |
| Agent product | **Two products.** **Projects** (was Workspaces) = coding (`grok` / `codex` / `claude` in PTYs, worktrees). **Bots** = informational named sessions (`sola-bots` daemon + ACP `grok agent stdio`, dialog UI, `~/Bots/<slug>/`). The iced ACP/Grok-leader GUI (`crates/sola-agent`) stays **retired** — do not rebuild it. Coding agents **call** the browser; bots do too (`solactl browser`). Freeze [`docs/specs/2026-09-15-sola-bots-design.md`](docs/specs/2026-09-15-sola-bots-design.md). |
| Bots | Informational, not coding. Yolo tools. One long dialog (hide compaction). Home **`~/Bots/<slug>/`**. Write fence is the foundation prompt: **default home-only**; Joshua can order an escape (whole computer / a path outside home). Seat steal never lifts. Daemon is process of record; iced + native iOS are clients. Owner `bots`. |
| Projects | Host **user-launched CLI agents in PTYs**. Spawn sibling is the fan-out verb. No ACP chat, no mailbox orchestration. |
| Projects CLI | **Grok, Codex, and Claude are first-class.** Hooks, presence, OSC, and spawn implement Grok first; Codex and Claude get the same spawn / exec / hook path. OpenCode stays presence-only. |
| Projects UI | Load **impeccable** (Operate) + **frontend-design** before any UI. Kit tokens/atoms/components may be refined; do not silently restyle other apps. |
| Projects worktrees | **`<project-root>/.worktrees/<name>`** (D4.2). App may append `/.worktrees/` to the project's `.gitignore` on first spawn. |
| Projects merge / drop | Never remove a git worktree or rail tab unless asked. Merge/LGTM is merge only. "Clean up this worktree" / "merge and clean up" / "remove this worktree, don't merge" also close the tab. If the worktree goes, the tab goes unless they say keep it. |
| Projects names | **Renamed 2026-10-21 on `next`** (Joshua; was Workspaces, so "workspace" means the desktop kind). Crate / app id **`sola-projects`**. Window **Projects**. Owner **`projects`**. The unit inside a project is a **tab** (methods `tab.*`, flag `--tab`, JSON `tabs` / `tab` / `tab_name`); a project's tabs are its root, worktree and folder tabs. Config **`~/.config/sola/projects/`** (migrates `workspaces/`, before that `agent-terminal/`). Runtime names stay: tmux **`sola-ws`** / **`sws-`**, `SOLA_WS_PATH`, `SOLA_WS_HOOKS_SOCK`, ids `ws-*`. Clean break: no `solactl workspaces` alias. |
| Projects calls | Register on **sola-call** as owner `projects`. Face is `solactl projects …`. No `sat` binary. Fail if app/host down. First-class: verbs/payloads stay in lockstep with the app ([CLI freeze](docs/specs/2026-08-18-workspaces-cli-design.md)). |
| Gamescope host | Windowed only (`-W`/`-H`, never host `-f`); product path is Arcade nest |
| Super+K | Keyboard shortcuts overlay (Omarchy chord). Paint Crop is Super+Shift+K. |
| Screenshots | Super+Shift+3/4/5 → compositor clipboard (`image/png`, Fastest, promised offer). No auto file, no auto Preview. `solactl compositor screenshot` still writes a path. |
| cargo make | **Release** is the default for `build` / `install`. `--debug` for unoptimized. `--release` still accepted. |
| Windowing (`next`) | **Next tab bar** (2026-10-21, [spec](docs/specs/2026-10-21-next-tab-bar-design.md)): the shell owns where windows are. A bar docked left lists every window by zone; a tiled zone shows one tab, the rest are left out of the composition. Replaces zones-as-snaps and default-float on `next`; master keeps float + zones until `next` merges (only when all of Next is done and tested on canto). |
---
