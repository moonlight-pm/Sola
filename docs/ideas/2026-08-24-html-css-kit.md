# HTML/CSS kit (LLM chrome) — research from `kit-retarget`

**Status:** idea + HTML kit in this worktree. **Not** a freeze, **not** a
switch from iced.  
**Worktree:** `.worktrees/kit-retarget` (branch `kit-retarget`).  
**Crate:** `crates/sola-kit-spike/` — workspace member (wgpu 27). **sctk 0.20 +
calloop**. Binaries (do not install):
- `sola-kit-spike` — storybook. `app_id` `sola-kit-spike`, title `Kit (spike)`.
- `sola-settings-lab` — Applications + Mail twin. `app_id` `sola-settings-lab`,
  title `Settings (lab)`. Same bus topics as iced settings.
- `sola-monitor-lab` — Bus + Call inspector twin. `app_id` `sola-monitor-lab`,
  title `Monitor (lab)`. Same observer path as iced monitor. Chrome is nested
  flex: nav | log column (Filter toolbar + log + inspector) | last-known rail
  to the top of the window. Not absolutely positioned panes.
**Kit components** (defined once in `src/components/`, not copied per app):
`sidebar`, `json`, `button`, `field`, `text`, `badge`, `select`, `card`,
`titlebar`, `split`, `toolbar`, `icon`, `pane`. Settings-lab and monitor-lab
compose those builders. Apps leave a `data-slot` in HTML.
**Removed:** `sola-blitz-spike`, `sola-html-spike` (probes; hole notes below).
**Out of scope:** replacing `sola-kit` / iced on master; putting the terminal
grid in HTML; JS/DOM (punted); IME. `cargo make install` skips `*-spike` /
`*-lab`.

Do not implement from this folder without promotion.

---

## Why this exists

Two forces, not “HTML is nicer”:

1. **LLMs already know HTML/CSS/JS/DOM.** Kit chrome authored as markup is
   closer to how models write UI than iced widgets.
2. **Live-load frontend the way we already live-load CSS.** Save a file, see
   behavior — not a full crate rebuild.

Iced stays the **shipped** app kit. This note is whether a second kit is worth
spike examples, and what it must not become.

---

## Spike (as-built, 2026-08-24)

`crates/sola-html-spike` — Graphite sidebar chrome from Scratch, not mail
pages. User-run binary only (`cargo make build sola-html-spike --release`).

| Claim | Result |
|---|---|
| Type matching iced | **Locked.** cosmic-text + swash, linear blend onto the CSS fill. Bitmap labels rejected (no selection). |
| HTML + CSS subset + Taffy | **Locked.** Flex chrome, `:root` `var()`, overflow clip. |
| Live CSS | **Locked.** F2 cycles in-memory vars; `assets/sidebar.css` mtime reload. |
| HTML from a file | **Locked.** `assets/sidebar.html`: `data-template="row"` cloned into `data-slot="rows"`; `data-bind` for title/query/label. Store still owns the row *list*; file owns *markup*. Mtime reload. |
| wgpu window present | **Locked.** Parent swapchain: GPU CSS boxes + glyph overlay (full-window texture). |
| CSS-sized native hole | **Locked.** `wl_subsurface` (same client). |
| GPU on that hole | **Parked.** Vulkan WSI + later SHM attach trips River `wp_linux_drm_syncobj` (“buffer attached but no acquire point”). Hole is SHM-only when embedding. |
| Another process in the hole | **Locked as nested compositor.** Parent binds `wayland-html-hole-*`, auto-spawns `sola-html-spike --foreign-client` (operator does not start it). Magenta/cyan SHM stripes **dogfooded moving**. Client must read the socket (`blocking_dispatch`) or `wl_buffer.release` never lands and animation freezes after two frames. `wl_subsurface` cannot parent a *River* client (same-connection only). |
| Overlay scrollbar | **Locked** (lab: track+thumb on overflow panes; log uses virt height). |
| IME preedit | **Parked** (English-only desk). Space on a Mac board is `NamedKey::Space`. |

**Rejected on the way:** Blitz/`<img>` bake; Vello/Parley type; `.so` / `dlopen`
for live logic (compile gate + ABI, fights LLM-in-the-same-files).

**Still optional, not blocking a judgment:** GPU glyph atlas (pack cosmic-text
bitmaps, instance quads). Quality is already iced-class; atlas is frame-time
and memory. **Implementable later** on this route — skip until a spike cares
about the full-window glyph upload.

---

## Live-load logic

HTML and CSS already hot-reload on mtime. Logic has the same *loop* only if
the artifact is a **file the process can apply without cargo**.

| Vehicle | Live-load | LLM fit |
|---|---|---|
| `.js` next to the HTML (or `<script>`) | Save → eval. Same latency class as CSS. | Matches HTML/CSS/DOM familiarity. |
| `cdylib` / `dlopen` | Save → **build** → load. ABI + state across unload. | Rust plugin API, not `document`. **Dropped.** |
| Re-exec the binary | What kit apps already do on `/opt/sola/bin` replace. | No ABI; resets state; not “like CSS.” |

**Dropped:** shared-library hot reload.

---

## JS / DOM (if we do it)

QuickJS (or MicroQuickJS) is **only the language**. There is no DOM in the
engine.

**Custom host on our `Elem` tree** — not jsdom, not Servo, not a second HTML
engine. Façade verbs models already emit:

- `querySelector` / `querySelectorAll` — `#id`, `.class`, `[data-*]`, tag
- `classList`, `dataset` / attributes, `textContent`
- `style.setProperty` for CSS vars (the F2 path)
- `addEventListener('click' \| 'input')` from winit hit-test

No `innerHTML` (or only through *our* parser). No `fetch`. No live NodeList,
Shadow DOM, Range, MutationObserver. After mutation: dirty flag, then Taffy +
paint — not layout inside every `classList.add`.

**Performance (this density of chrome):** event-driven script is noise next to
today’s glyph overlay. The hit is **per-frame JS** (`rAF`, busy timers) or
rebuilding the tree from JS. Host policy: run JS on events and on file
reload; do not offer `rAF` until we opt in.

Engine default if spiked: **QuickJS** (`rquickjs` / quickjs-ng). Boa if we
refuse C. Not V8/Node.

First JS spike (still isolated crate): file-authored `<script>` that toggles a
class, mtime reload. Not a Filter-field `eval()`.

---

## Native hole (product constraint)

Terminal **stays** the current surface (alacritty grid + iced), not HTML.
The kit claim is **chrome around a compositor-composited hole**.

True “another Sola process in the hole” is a **nested Wayland display** in
that CSS box (spike proved the shape), not stuffing a River toplevel into a
subsurface.

---

## Spike apps (not a canary install)

Do **not** replace `/opt/sola/bin/sola-kit` (or any shipped app) with a
kit experiment. Do **not** install spikes.

Per-app **parallel identity**, run from the crate `target/` directory:

| | Shipped | Spike |
|---|---|---|
| Binary | `/opt/sola/bin/sola-kit` | `crates/sola-kit-spike/target/release/sola-kit-spike` |
| Wayland `app_id` | `sola-kit` | `sola-kit-spike` |
| Bus app id | same as `app_id` | same as spike `app_id` (or no bus) |
| Title | `sola-kit · …` | `Kit (spike)` |

Shell groups, floats, app menus, and `CloseApp` key off `app_id`. A different
id is mandatory or the spike and the real app collide.

`cargo make install` skips `*-spike`. Later spike apps: `sola-<app>-spike`.

Do **not** spike `sola-shell` or `sola-river` first. First surface is the kit
storybook (`sola-kit-spike`) — **not** the PTY grid.

---

## Should we switch the kit off iced?

**No — not as a wholesale retarget.**

Iced/`sola-kit` is the dogfooded desktop (shell, mail, workspaces, terminal
grid, settings). The spike showed HTML/CSS chrome + a native hole is
*possible*, with iced-quality type and live files. It did **not** show a kit
that can replace:

- shared widgets + storybook
- bus theme / fonts / shell tokens as shipped
- accessibility, IME, text selection in chrome
- one process model for every app
- GPU text atlas under load
- JS host policy and sandbox

Switching now would freeze the LLM thesis before a single spike app
survives a week of dogfood.

**When a switch is even discussable:** one spike binary, distinct `app_id`,
live HTML/CSS/(optional) JS, hole for native content, no regressions vs the
iced twin on the tasks that app actually does — then promote a freeze.

Until then: iced is the kit; this is an idea plus an unmerged spike.

---

## Recommended next steps

1. Settings-lab is the dogfood twin (Applications + Mail). Remaining vs
   iced: storybook still always draws CSD; 0.5px rims; mail rules are
   add/remove not the full condition editor. Next twin or freeze talk —
   not a merge.
2. JS façade stays punted (QuickJS + `Elem` — later).
3. Native hole / nested compositor stays parked until a spike app needs it.
4. **Do not** start glyph atlas, `.so` reload, a spec DOM, or iced retarget.
5. No merge, no install.

**D5 (2026-08-25):** spike examples, `sola-kit-spike` first, sctk not winit,
JS punted, no install. Graduation to a freeze is still open. See
[`open-questions.md`](../open-questions.md) **D5**.
