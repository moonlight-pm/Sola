# sola-design — HTML artboards on a canvas, edited by agents

**Date:** 2026-10-12  
**Status:** **Frozen** — slices 1–2 **installed** and smoked 2026-10-12; slice 3 (adaptive raster, auto height, PDF) built on worktree `sola-design`  
**Related:** [sola-wrapper](2026-08-21-sola-wrapper-design.md) (CEF via the sola-browser lib); [browser agent control](2026-09-11-sola-browser-agent-control-design.md) (CDP wrap, call owner shape); [call plane](2026-08-13-sola-call-plane-design.md); [Workspaces CLI](2026-08-18-workspaces-cli-design.md) (rail + `solactl` shape); plan [`plans/2026-10-12-sola-design-plan.md`](../plans/2026-10-12-sola-design-plan.md)

| | |
|--|--|
| **Implementation** | `crates/sola-design`; sola-browser CEF **board mode** (per-tab view size + raster scale, board tabs paint regardless of the active tab, per-tab input, `AgentOp::Eval`). Call owner `design` (`app/cli.rs`, specs `calls.rs`). |
| **Dogfood** | Slice 1 **installed** `design` release 2026-10-12; basic smoke done. First smoke found `AgentReply.data` (`serde_json::Value`) undecodable over bincode IPC — fixed (JSON text), router now kills a still-running helper before respawn, Design reopens boards after a helper restart. |
| **Gaps** | Fixed 2× raster: past 200% (at output scale 1) zoom softens; a board that cannot fit 2× (texture side 8192 px, frame 160 MiB) drops the whole canvas to 1× (zoom is per host). Auto height is measured once per load. No IME. No MCP. |

## Intent

A design canvas that is Sola-first and agent-first. The design **is**
plain HTML/CSS on disk, inside the project. sola-design renders it live
with CEF, lays artboards out on an infinite plane, and lets the human
pan, zoom, look, and point. There are no drawing tools, no inspector,
and no property panels. All editing happens through an agent: it edits
the files with its normal file tools and drives the view with
`solactl design`.

The app is the renderer and the agent's eyes:

- **See:** render truth (screenshots of an artboard).
- **Measure:** box, computed styles, overflow.
- **Point:** what the human clicked (“make *this* bigger”).
- **Steer:** show the human the artboard the agent just changed.

Pencil (`.pen` node model behind an MCP), Paper (DOM canvas, cloud file),
and Penpot (own shape model) all keep the document in an app-owned
model. Here the files are the document: diffable, versioned in the
project's git, and openable in any browser.

## Locked

| Topic | Choice |
|-------|--------|
| Crate / binary | `sola-design`; `app_id` `sola-design`; window title **Design**. |
| UI | Iced + sola-kit. Project rail on the left (kit sidebar); canvas on the right. |
| Document | **Files are the document.** No app-owned node model, no `write_html`-style node ops. |
| Projects | Own list at `~/.config/sola/design/projects.json`. **Separate** from Workspaces (may merge later). Add project = pick a folder. |
| Root only | Canvases are read from `<project root>/design/` only. Worktrees are not scanned (v1). |
| Rail | Sections = projects. Items = canvases: each directory `design/<canvas>/` that holds a `canvas.toml`. `+` on a project creates an empty canvas (name only). |
| Engine | CEF via `sola-browser` (`default-features = false`) + `profiles::bind_external("sola-design", …)`. One `--engine` helper. **One OSR browser per artboard** (“board” tab). CPU OSR stays; do **not** enable `accelerated_osr`. |
| Board tabs | `Cmd::OpenBoard` creates a board at its own CSS size and raster scale, shown; `Cmd::SetBoard` resizes / re-scales it. A board is never `was_hidden` and paints whether or not it is the active tab. The browser and wrappers never create board tabs. |
| Raster mechanism | **Page zoom, not device scale.** This CEF's OSR paints at 1× whatever `GetScreenInfo` (or `--force-device-scale-factor`) says, so a board's OSR view is CSS size × scale in pixels and the page is zoomed by the scale: the layout viewport stays the artboard's CSS size, `devicePixelRatio` reports the scale, pixels are real. Chromium keeps zoom per host, so **one scale per canvas**. Board pointer input is CSS px; the engine scales it to view px. |
| Canvas plane | Drawn by iced, not by the page: background, artboard textures (custom wgpu shader, one texture per artboard, dirty-rect uploads), titles, selection outline, mode button. Pan and zoom never repaint CEF. |
| Raster scale | **Fixed integer 2×** (crisp to 200% at output scale 1). Zooming the canvas only scales textures — immediate, no re-raster, and text never shifts against itself. 1× for the whole canvas only if some board cannot fit 2×. (2026-10-12: a zoom-adaptive re-raster was tried and removed — it waited for the camera to settle, popped, and fractional zoom moved text lines by a pixel.) |
| Pan | **Right-drag** pans, in both modes. |
| Zoom | The **wheel always zooms**, about the pointer. No wheel scrolling anywhere, including inside artboards. ⌘0 fits all, ⌘1 is 100%, ⌘= / ⌘- step. |
| Pointer | **No modes.** Pages are always live: pointer and keys go to the artboard under the pointer / the one last clicked. **Super+click** picks instead (2026-10-12: replaced the Select / Interact toggle). Wheel zooms; right-drag pans. |
| Pick | Super+click picks the element under the pointer in that artboard (`document.elementFromPoint` via CDP `Runtime.evaluate`); iced draws the outline. Super+click on empty canvas clears it. The pick is held in the app for the agent. Super comes from River as a bus chord (`sola_browser::input`), since iced does not see a bare Super. |
| Auto height | `height = "auto"`: the viewport starts at 900 and is resized to the page's content height after each load (measured once per load, so `vh` content cannot run away). Until measured, `wait` is not done. |
| Reload | Recursive watch on `design/`. The open canvas's `canvas.toml` → re-layout (open, close, move, resize boards). Any other file in that canvas, or a shared file outside every canvas (`design/tokens.css`) → reload its boards. Edits inside other canvases are ignored. |
| Agent face | Call owner **`design`**; `solactl design …` compiled clap (slice 2). MCP is a later **generic** adapter over call-plane owners, not a second protocol. |
| Install | Never auto-install. `cargo make install design` is the human's call. |

## On disk

```text
<project>/design/
  tokens.css           optional; shared by canvases (`../tokens.css`)
  <canvas>/
    canvas.toml        layout: which artboards, where, how big
    *.html             one file per artboard (plain HTML/CSS)
    *.css, assets/     anything the pages link (relative URLs)
```

`canvas.toml`:

```toml
title = "Settings redesign"     # optional; defaults to the directory name

[[artboard]]
id = "desktop"                  # unique in this canvas; default = file stem
src = "desktop.html"            # path relative to the canvas dir, or http(s) URL
x = 0                           # canvas units = CSS px at 100%
y = 0
width = 1440                    # the artboard's viewport (CSS px)
height = 900                    # or "auto": grow to the page's content
title = "Desktop"               # optional; default = id

[[artboard]]
id = "phone"
src = "phone.html"
x = 1600
y = 0
width = 390
height = 844
```

Each artboard is a real viewport, so media queries and `vw`/`vh` are
true to its size. A `src` URL can point at a running dev server so the
real build sits next to the mock.

## Agent verbs (slice 2)

Owner `design`. Verbs act on the canvas the human sees. `--artboard`
defaults to the selection's artboard, else the only one. Path args are
resolved by `solactl` against the caller's cwd.

| Verb | Does |
|------|------|
| `project.list` | Projects and their canvases (fresh scan). |
| `project.add --path` | Register a project folder. |
| `open --path [--raise]` | Show the canvas holding that path (dir, toml, or any file in it); registers the project if needed. Raises only with `--raise`. |
| `status` | Canvas, artboards (size, scale, painted, loading), zoom, mode, selection. |
| `selection` | The human's pick: project, canvas, artboard, selector, tag, text, rect, trimmed `outerHTML` — or `null`. |
| `view --fit` / `--artboard [--zoom]` / `--zoom` | Steer the camera. |
| `wait [--artboard] [--timeout]` | Until the artboards have loaded and painted. |
| `reload [--artboard] [--timeout]` | Hard-reload, then wait. The file watcher is asynchronous; agents `reload` after editing. |
| `screenshot [--artboard] [-o]` | CDP PNG at the raster scale → `path`. |
| `pdf [--artboard] [-o]` | One-page PDF, the artboard's size, screen media, backgrounds, no margins → `path`. |
| `inspect [--artboard] [--selector]` | Viewport, document size, `cut_off_below`, element box + computed styles, up to 25 overflow / clipping issues. Defaults to the selection. |

Artboards are added, moved, and resized by editing `canvas.toml`; the
CLI has no second way to do that. No raw `eval`.

## Not this

- Tools, inspector, layers panel, property editing, or any drawing UI.
- An app-owned node model, or HTML → iced conversion.
- Paper's one-page canvas (pan/zoom by CSS transform inside one CEF
  page): under CPU OSR every pan frame would be a full repaint and
  upload, and artboards would not have their own viewports.
- Scrolling inside artboards (the wheel is zoom).
- Worktree scanning (v1).

## Slices

1. **Engine board mode + app.** Rail, projects, `canvas.toml`, live
   artboards on the plane, pan/zoom, reload on change, live pages + pick,
   selection outline.
2. **`solactl design`** (call owner `design`) + `docs/manual/sola-design.md`
   — built.
3. `height = "auto"`; PDF export; real 2× raster (page zoom) — built.
   Zoom-adaptive re-raster tried and removed (settle lag, text shift).
4. Generic call-plane → MCP adapter (browser, workspaces, design).
