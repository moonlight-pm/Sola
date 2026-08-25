# HTML/CSS kit (LLM chrome) — research from `kit-retarget`

**Status:** idea. Isolated spike only; **not** a freeze, **not** a switch from iced.  
**Worktree:** `.worktrees/kit-retarget` (branch `kit-retarget`). Crate
`crates/sola-html-spike/` is **workspace-excluded**. Do not merge, do not
`cargo make install` the spike.  
**Out of scope:** replacing `sola-kit` / iced on master; putting the terminal
grid in HTML; bus `Topic::Theme` (live CSS proved without it); IME (English
only on this desk).

Do not implement from this folder without promotion.

---

## Why this exists

Two forces, not “HTML is nicer”:

1. **LLMs already know HTML/CSS/JS/DOM.** Kit chrome authored as markup is
   closer to how models write UI than iced widgets.
2. **Live-load frontend the way we already live-load CSS.** Save a file, see
   behavior — not a full crate rebuild.

Iced stays the **shipped** app kit. This note is whether a second kit is worth
a canary, and what it must not become.

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
| Overlay scrollbar | **Locked.** |
| IME preedit | **Parked** (English-only desk). Space on a Mac board is `NamedKey::Space`. |

**Rejected on the way:** Blitz/`<img>` bake; Vello/Parley type; `.so` / `dlopen`
for live logic (compile gate + ABI, fights LLM-in-the-same-files).

**Still optional, not blocking a judgment:** GPU glyph atlas (pack cosmic-text
bitmaps, instance quads). Quality is already iced-class; atlas is frame-time
and memory. **Implementable later** on this route — skip until a canary cares
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

## Canary channel (if anything is ever ported)

Do **not** replace `/opt/sola/bin/sola-terminal` (or any shipped app) with a
kit experiment.

Per-app **parallel install**:

| | Shipped | Canary |
|---|---|---|
| Binary | `/opt/sola/bin/sola-terminal` | `/opt/sola/bin/sola-terminal-canary` |
| Wayland `app_id` | `sola-terminal` | `sola-terminal-canary` |
| Bus app id | same as `app_id` | same as canary `app_id` |
| Title | unchanged | suffix ` (canary)` so switcher/MRU copy is obvious |

Shell groups, floats, app menus, and `CloseApp` key off `app_id`. A different
id is mandatory or the canary and the real app collide.

Install: a sola-make target that copies `*-canary` only (never the
unsuffixed name). Ask before every install, same as today. Self-watch the
canary binary path.

Do **not** canary `sola-shell` or `sola-river` first. First candidate, if any:
a small chrome surface (spike-as-storybook, or a throwaway panel) — **not**
the PTY grid.

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

Switching now would freeze the LLM thesis before a single shipped app
survives a week of canary.

**When a switch is even discussable:** one canary binary, distinct `app_id`,
live HTML/CSS/(optional) JS, hole for native content, no regressions vs the
iced twin on the tasks that app actually does — then promote a freeze.

Until then: iced is the kit; this is an idea plus an unmerged spike.

---

## Recommended next steps

1. **Leave the spike in this worktree.** Tag or leave the branch; no merge to
   master, no install.
2. **This idea file is the handoff** for the HTML-kit discussion. Do not
   start a freeze until a canary is an explicit Now item.
3. **If we keep proving in the spike:** event-driven QuickJS + 10-method
   `Elem` façade + mtime on `<script>` — the live-logic analogue of CSS
   reload. Still isolated; still no install.
4. **If we port anything:** land **canary install + distinct `app_id` /
   title** in sola-make *before* the app. First port is not terminal, not
   shell.
5. **Do not** start glyph atlas, `.so` reload, or a spec DOM.

**Decision (human):** whether HTML-kit work is parked here, or CURRENT **Now**
grows a canary spike (JS façade and/or install plumbing). Record in
[`open-questions.md`](../open-questions.md) **D5**.
