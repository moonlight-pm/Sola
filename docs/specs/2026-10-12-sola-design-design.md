# sola-design — HTML artboards on a canvas, edited by agents

**Date:** 2026-10-12  
**Status:** **Frozen** — slices 1–6 on master (2026-10-12), installed; slice 7 (memory budget) installed 2026-10-14; slice 8 (annotations, exits, flows) installed 2026-10-14  
**Related:** [sola-wrapper](2026-08-21-sola-wrapper-design.md) (CEF via the sola-browser lib); [browser agent control](2026-09-11-sola-browser-agent-control-design.md) (CDP wrap, call owner shape); [call plane](2026-08-13-sola-call-plane-design.md); [Workspaces CLI](2026-08-18-workspaces-cli-design.md) (rail + `solactl` shape); plan [`plans/2026-10-12-sola-design-plan.md`](../plans/2026-10-12-sola-design-plan.md)

| | |
|--|--|
| **Implementation** | `crates/sola-design`; sola-browser CEF **board mode** (`OpenBoard` / `SetBoard`, page-zoom raster, board tabs paint regardless of the active tab at their own pace (`BoardPace`), per-tab input, `AgentOp::Eval` / `Pdf`). Call owner `design` (`app/cli.rs`, specs `calls.rs`). MCP: `solactl mcp` (generic over call-plane owners). |
| **Dogfood** | **Installed** `design`+`solactl`+`shell`+`browser`+`wrapper` release 2026-10-12. Smoked: picks, `solactl design` verbs, zoom, Super+click. First agent session (Exit, Claude over MCP) ported a Pencil file to `Exit/design/foundation` (tall auto boards, web fonts, product CSS outside `design/`); its two field reports drove the served origin, settle, linked-file watch, and the `src` / auto-height fixes, re-verified by Exit. 2026-10-12: rail drag-reorder + `project.remove` installed `design`; `project.remove` smoked over solactl and MCP, drag smoked. 2026-10-13: board pace installed `design`, smoked (idle CPU gone). 2026-10-14: memory budget installed `design`; `status` memory and `canvas.close` smoked (ten boards' memory fully back within ~20 s); a discard under real pressure not yet seen. |
| **Gaps** | An `auto` page sized in `vh` grows a few steps per load (the measured height carries across reloads). Page dialogs are auto-accepted. Memory: board cost is an estimate (measured once); a board whose renderer is killed is not recovered; boards stay 2× and one renderer each — see [idea](../ideas/2026-10-13-design-memory-budget.md). |

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
| UI | Iced + sola-kit. Project rail on the left (kit sidebar); canvas on the right. Rail rows drag: canvases within their project, project headers as blocks (order saved in `projects.json`). |
| Document | **Files are the document.** No app-owned node model, no `write_html`-style node ops. |
| Projects | Own list at `~/.config/sola/design/projects.json`. **Separate** from Workspaces (may merge later). Add project = pick a folder. |
| Root only | Canvases are read from `<project root>/design/` only. Worktrees are not scanned (v1). |
| Rail | Sections = projects. Items = canvases: each directory `design/<canvas>/` that holds a `canvas.toml`. `+` on a project creates an empty canvas (name only). |
| Engine | CEF via `sola-browser` (`default-features = false`) + `profiles::bind_external("sola-design", …)`. One `--engine` helper. **One OSR browser per artboard** (“board” tab). CPU OSR stays; do **not** enable `accelerated_osr`. Design's helper runs with `--disable-gpu-compositing` (2026-10-14): with CPU frames, GPU compositing read every frame back whole. |
| Board tabs | `Cmd::OpenBoard` creates a board at its own CSS size and raster scale, shown; `Cmd::SetBoard` resizes / re-scales it. A board paints whether or not it is the active tab. The browser and wrappers never create board tabs. |
| Board pace | **Only what someone looks at runs at full rate** (2026-10-13, human ask: Design was taking ~3.5 cores idle — every frame of an animated board copies its whole surface under CPU OSR). `Cmd::BoardPace` per board: **Live** (60 fps), **Slow** (1 fps, shown), **Asleep** (`was_hidden`: no frames, like a background browser tab; wakes repainted whole). A board loading, settling, or answering a call is Live. On the canvas on screen a board in view (plane + 256 px) is Live while Design has focus, Slow while it does not; out of view it sleeps. Off-screen canvases sleep, except one an agent called on in the last 30 s (Slow). A sleeping board keeps its last picture. Whole boards, not their visible part: a part would re-raster on every pan. |
| Memory budget | **Boards fit in what the machine has free** (2026-10-13/14, human ask after ten agents' canvases filled 125G and froze the box; [decision log](../open-questions.md#decision-log)). No fixed size: Design keeps **15%** of RAM free for everything else (`MemAvailable`, min 2 GiB) and costs each open board at 64 MiB + 9 × its OSR surface bytes (measured: ten boards, 295 MiB of frames, ~2.9 GiB). Opens and discards are carried until `MemAvailable` shows them. Short on memory (2 s check), Design **discards idle boards**, least recently used first: not in view on the human's screen, not working (loading, settling, a call, a wait), not on a canvas an agent called in the last 30 s, not the focus or pick, untouched 30 s. A discarded board's tab closes (same tab id); the plane keeps its picture; its layout, links, and share stay. It reopens when it comes into view (the human — never refused) or an agent's call needs it (`screenshot` / `pdf` / `inspect` wait for the reload, `wait` / `reload` reopen what they cover). An agent's open, call on a closed canvas, or reopen is **refused with the reason** only when the boards in use still do not fit after discarding: free / total / kept free, each open canvas (boards, memory, why kept, path), and the way out (`canvas.close`, wait, ask the human). New boards a file edit adds to an off-screen canvas wait discarded (no picture) until they fit. |
| Origin | **Loopback http, project root as `/`** (D5, 2026-10-12). Design runs a small server (127.0.0.1, random port, GET/HEAD, single byte ranges, files under a registered project root, no dotfile segments, no listing; Host must name a registered project; no CORS header). Each board loads from its own host `http://<board>--<canvas>.<project>.localhost:<port>/design/<canvas>/<src>`, so page zoom (raster scale) is per board. Web `src` URLs load as written. |
| Raster mechanism | **Page zoom, not device scale.** This CEF's OSR paints at 1× whatever `GetScreenInfo` (or `--force-device-scale-factor`) says, so a board's OSR view is CSS size × scale in pixels and the page is zoomed by the scale: the layout viewport stays the artboard's CSS size, `devicePixelRatio` reports the scale, pixels are real. Chromium keeps zoom per host, so **one scale per canvas**. Board pointer input is CSS px; the engine scales it to view px. |
| Canvas plane | Drawn by iced, not by the page: background, artboard textures (custom wgpu shader, one texture per artboard, dirty-rect uploads), titles, selection outline, mode button. Pan and zoom never repaint CEF. |
| Raster scale | **Fixed 2×** per board (1×, then under 1× for a page past 8192 px). Zooming the canvas only scales textures — immediate, no re-raster, and text never shifts against itself. A board that cannot fit 2× renders at 1× (per host: each served board has its own). (2026-10-12: a zoom-adaptive re-raster was tried and removed — it waited for the camera to settle, popped, and fractional zoom moved text lines by a pixel.) |
| Detail renders | Past a board's raster scale, once the camera rests (120 ms), the part on screen is re-rastered at the screen's scale and drawn over the board's texture. Mechanism: the board's layout is pinned with DevTools device metrics (so the OSR surface may exceed the board), and a DevTools viewport override maps a region at a scale — a compositor transform, so nothing relayouts and text holds still (the reason adaptive re-raster was removed). Frames carry the request `seq` Chromium acknowledged. A board whose surface is too small is first grown to the plane. Zoom limit 3200%. (2026-10-12, slice 5.) |
| Pan | **Right-drag** pans, in both modes. |
| Zoom | The **wheel always zooms**, about the pointer. No wheel scrolling anywhere, including inside artboards. ⌘0 fits all, ⌘1 is 100%, ⌘= / ⌘- step. |
| Pointer | **No modes.** Pages are always live: pointer and keys go to the artboard under the pointer / the one last clicked. **Super+click** picks instead (2026-10-12: replaced the Select / Interact toggle). Wheel zooms; right-drag pans. |
| Pick | Super+click picks the element under the pointer in that artboard (`document.elementFromPoint` via CDP `Runtime.evaluate`); iced draws the outline. Super+click on empty canvas clears it. The pick is held in the app for the agent. Super comes from River as a bus chord (`sola_browser::input`), since iced does not see a bare Super. |
| Auto height | `height = "auto"`: the viewport starts at 900 and is resized to the page's content height when the board settles after each load (web fonts ready, two frames). Each resize earns one confirming settle, at most two per load. After the first settle the page **watches its height** for the rest of the load (one pending eval: a `ResizeObserver` on the root, body, and body's children resolves when the content height changes; Design resizes and asks again — no timers), so late images, fetched data, and opened sections resize it. A change that came with a viewport change (`reflow`) is applied at most two times in a row, so `vh` content cannot run away; scrollbars are hidden so layout width always equals the artboard width. |
| Settle | After every load a board is settled (`document.fonts.ready`, two frames) and reports its height, HTTP status, URL, and the local resources it used. An `auto` resize settles again (≤ 2 per load) since the new viewport can reflow. Each load has a generation; a settle answered after the board navigated on is ignored. `wait` / `reload` count a board done only when settled. Design's CEF runs with `--hide-scrollbars` (a scrollbar narrowed the first layout of tall pages). |
| Board errors | A missing `src` file (at layout) or an HTTP status ≥ 400 (at settle) is the board's `error`: red in its title on the canvas, `error` in `status`. `status` also shows each board's actual tab `url` next to `src`. |
| Reload | Recursive watch on `design/` plus the files a board loaded from outside it. The open canvas's `canvas.toml` → re-layout; any other file in that canvas, a shared file outside every canvas (`design/tokens.css`), or a linked outside file → reload its boards (skipped when the boards already show those file times). Edits inside other canvases are ignored. `src` keeps `?query` / `#fragment`. |
| Agent face | Call owner **`design`**; `solactl design …` compiled clap (slice 2). MCP is a later **generic** adapter over call-plane owners, not a second protocol. |
| Agent / human | **Orthogonal** (2026-10-12, human ask). The human can pan, zoom, and look around a canvas an agent is working on: agent reads never depend on the view (`screenshot` / `pdf` hold their board whole — no detail request reaches it until the reply — and the human's pointer leaves the page first, so no `:hover`). `view` moves the camera only until the human has panned / zoomed that canvas; then `moved: false`. |
| Agent glow | What an agent touches shimmers on the overlay (halo + a light sweep): elements it inspects, the selection it reads, a board it screenshots or prints, a board reloading after its files changed (until settled). Never in the pages, so never in an agent's screenshot. |
| IME | Pages compose with the input method: the overlay is wrapped so iced enables IME at the page caret while a board has the keyboard; preedit / commit go to the page as composition. |
| Canvases | **Many live, one on screen** (2026-10-12, human ask: multi-agent, nobody in anyone's way). A canvas an agent makes or opens is live off the human's screen; agents never bring one to the front (`--raise` only when the human asks). Every page call names its canvas (`--canvas`); over MCP it defaults to that agent's last opened / made canvas, so agents working at once stay apart. Each canvas keeps its own boards, files, watcher, and camera. The rail marks a canvas an agent is working in. Off-screen canvases idle 10 min close and reopen on the next call. |
| Share | **Live** (D6, 2026-10-12): every canvas has a link `<public_url>/<token>/` (128-bit token, minted automatically, rotatable). Design is the web server — a thin server that answers only for that canvas's files (its folder, shared `design/` files, its `root`, the files its boards load) and a viewer page handled as in Design (live pages, right-drag pans, wheel zooms, no element picking; a one-line hint; on a phone (2026-10-13) the pages are for looking only — one finger pans, a pinch zooms, no hint; pages reload on change). A Cloudflare Tunnel forwards `design.moonlight.pm` to it; Design runs the connector. |
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
root = "web/public"             # optional: root-absolute URLs look here first, then the project root

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
| `project.remove --path` | Forget a project (its `root`): its canvases leave the rail and close, agents' too. Files stay. |
| `canvas.new NAME [--path] [--raise]` | Make `<project>/design/<slug>/canvas.toml` (project = `--path`, default the caller's cwd), register the project, make it live off screen (`--raise` shows it). Existing → open (`created: false`). |
| `open --path [--raise]` | Make the canvas holding that path (dir, toml, or any file in it) live off the human's screen; registers the project if needed. `--raise` shows it and raises the window. Refused (with the reason) when its boards do not fit. |
| `canvas.close --canvas` | Close a live canvas an agent is done with (`--canvas` required); its boards' memory comes back, files stay, the next call reopens it. The canvas on the human's screen stays (error). |
| `status` | Canvas, artboards (size, scale, painted, loading, discarded), zoom, mode, selection, `memory` (free / total / kept free MiB, Design's board estimate, open / discarded counts). |
| `selection` | The human's pick: project, canvas, artboard, selector, tag, text, rect, trimmed `outerHTML` — or `null`. |
| `share` / `share.rotate [--canvas]` | The canvas's live link; rotate makes a new one (the old stops). |
| `view --fit` / `--artboard [--zoom]` / `--zoom` | Steer the camera, until the human takes it (`moved: false`). |
| `wait [--artboard] [--timeout]` | Until the artboards show the files on disk (reload first when files are newer than the boards) and have settled. |
| `reload [--artboard] [--timeout]` | Hard-reload, then wait. The file watcher is asynchronous; agents `reload` after editing. |
| `screenshot [--artboard] [-o]` | CDP PNG of the whole artboard at the raster scale → `path`; independent of the view. |
| `pdf [--artboard] [-o]` | One-page PDF, the artboard's size, screen media, backgrounds, no margins → `path`. |
| `inspect [--artboard] [--selector] [--all]` | Viewport, document size, `cut_off_below`, element box + computed styles, up to 25 overflow / clipping issues. Defaults to the selection. `--all`: every match as `{selector, tag, rect, text}`. |

Artboards are added, moved, and resized by editing `canvas.toml`; the
CLI has no second way to do that. No raw `eval`.

## Annotations, exits, and flows (slice 8, 2026-10-14)

**Why.** A canvas with dozens of boards is hard to take in. At the zoom
that shows them all, nothing says which boards belong together, which page
leads to which, or what is not obvious; agents were writing that as
comments in `canvas.toml` ("rows follow the admin menu"). A viewer (often
on a share link) also wants to *walk* the product: click through a flow the
way a user would, with the whole map still around them.

**Policy (human, 2026-10-14).** Only agents edit designs. Annotations are
part of the design — they say what is not obvious, give general
information, and show flow. There are no human notes, comments, or
feedback on the canvas.

| What | Declared in | Drawn |
|------|-------------|-------|
| **Section** | `[[section]]` in `canvas.toml` | A titled region behind its boards (title and one-line note above it) |
| **Note** | `[[note]]` in `canvas.toml` | A numbered marker on a board, an element in it, or a canvas point; its card opens on hover, with the notes toggle, or on a tour step |
| **Exit** | `data-design-to="<board id>"` on an element in a page | An arrow from that element to the board; clicking the element moves the camera there |
| **Link** | `[[link]]` in `canvas.toml` | An arrow between two whole boards (a step with no element: "after payment clears") |
| **Flow** | `[[flow]]` in `canvas.toml` | A tour: start at a board, step along its exits and links |

```toml
[[section]]
title = "Sign in"                       # required
boards = ["signin", "signin-sent", "account"]   # required; its box is their bounds
note = "Magic link only; no passwords." # optional line under the title
id = "sign-in"                          # optional; default: slug of the title

[[note]]
board = "overview"                      # pinned to a board…
at = ".lineage"                         # …optionally to the first element matching this selector
text = """
Lineage reads left to right. The cyan edge is the route being edited.
"""
# or a canvas point: x = -600, y = 0 (no board)

[[link]]
from = "checkout"
to = "receipt"
label = "after payment clears"          # optional

[[flow]]
title = "Sign in"                       # required
start = "signin"                        # required: a board id
note = "From a fresh browser to a signed-in session."   # optional
id = "sign-in"                          # optional; default: slug of the title
```

In a page:

```html
<button data-design-to="signin-sent" data-design-label="email sent">Send link</button>
```

**Rules.**

- Every board id named in `canvas.toml` (`boards`, `board`, `from`, `to`,
  `start`) must exist, and section / flow ids are unique; otherwise the
  file is an error like any other (the last good layout stays up). A note
  has `board` **or** `x` + `y`, and non-empty `text` (plain text; blank
  lines are paragraphs). `at` needs `board`.
- Things a page declares are checked when it settles: an exit to an
  unknown board, or a note `at` that matches nothing, is reported in
  `status` and not drawn.
- Plain links between boards draw **no** arrow (a shared sidebar would put
  twenty on every board), but they work: clicking a link whose URL is
  another board's page (path and query; the fragment too when that tells
  boards apart) moves the camera to that board instead of loading the page
  into the board you clicked. Links to anything else behave as before.
- A flow does not list its steps. It starts at `start` and follows the
  current board's exits and links; where there are several, the viewer
  picks one.

**Design.**

- Sections draw under the boards (a canvas layer below the textures): a
  faint rounded region around their boards' bounds — padding in canvas
  units, never less than a few screen px, with room on top for the
  boards' titles — and the title (and note) above its top-left corner at a
  fixed screen size. A board may be in more than one section.
- Arrows are connectors that **never cross a board** (2026-10-14, human:
  lines through a page look bad). Each leaves a port on the source board's
  side facing the target — level with the exit element, or the side's
  middle for a `[[link]]` — and runs orthogonally through the gutters
  (lines beside every board's edges; shortest with a cost per bend; canvas
  units, so routes move with the boards, not the camera) to the target's
  facing side, entering level with where it left when the side allows (a
  straight run between neighbours; ports into one side keep 24 px apart).
  Rounded corners and an arrowhead; runs that share a gutter are nudged
  apart on screen. A label is a pill on its longest run, shown only where
  it covers no board — unless its arrow is lit. Neutral hairlines by default; accent
  only when they matter (the exit under the pointer, the tour step's way
  out); dim during a tour. An exit element under the pointer is outlined;
  no line is drawn from it across its page.
- Clicking an exit (or a plain link to a board) is the app's, not the
  page's: the page does not get the click. The camera glides (≈ 0.4 s) to
  frame the target board; a tall board is framed by its width, top first.
- Notes: a numbered marker (file order) at the element's / board's
  top-right corner or the canvas point; hover shows the card (fixed screen
  size, ~300 px wide) beside it; click keeps it open. The pill's notes
  button (and **N** with no page focused) opens every card.
- **Tours.** The pill's flows button lists the canvas's flows. Picking one
  glides to its start and shows the tour bar: back, the step (flow title,
  step number, board title), next — or, at a fork, the choices (exit label,
  else the element's text, then the target's title; **1–9** pick) — return
  (re-frame the step after looking around), and end. **→ / ←** step and
  go back, **Esc** ends, while no page has the keyboard. The step's notes
  open; arrows from other boards dim. Clicking an exit in a page follows it
  (outside a tour this starts one, so Back works). Panning and zooming
  never end a tour. Tour moves are the human's camera (an agent's `view`
  no longer moves it).
- None of this is in a page, so none of it is in a screenshot or PDF.

**Share link.** The viewer draws the same: sections, arrows, notes, and
tours (a flows button on its bar), with exits read from its own pages (they
are same-origin) and clicks on exits and board links handled the same way.

**Agent face.** No new write verbs: agents edit `canvas.toml` and the
pages. `status` adds `sections`, `links`, `notes` (with any `at` error),
`flows`, and per artboard its `exits` (`to`, `label`, `text`, `rect`,
`error`). `view --section <id>` frames a section.

## Not this

- Tools, inspector, layers panel, property editing, or any drawing UI.
- An app-owned node model, or HTML → iced conversion.
- Paper's one-page canvas (pan/zoom by CSS transform inside one CEF
  page): under CPU OSR every pan frame would be a full repaint and
  upload, and artboards would not have their own viewports.
- Scrolling inside artboards (the wheel is zoom).
- Worktree scanning (v1).
- Human notes, comments, or feedback on the canvas (2026-10-14: only
  agents edit designs; annotations belong to the design).
- Drawing arrows or placing notes by hand; a full-screen prototype mode
  (tours stay on the canvas).

## Slices

1. **Engine board mode + app.** Rail, projects, `canvas.toml`, live
   artboards on the plane, pan/zoom, reload on change, live pages + pick,
   selection outline.
2. **`solactl design`** (call owner `design`) + `docs/manual/sola-design.md`
   — built.
3. `height = "auto"`; PDF export; real 2× raster (page zoom) — built.
   Zoom-adaptive re-raster tried and removed (settle lag, text shift).
4. Generic call-plane → MCP adapter (browser, workspaces, design) —
   `solactl mcp`, built.
5. Limits + agent/human (2026-10-12): detail renders (sharp to 3200%),
   pages past 8192 px, byte ranges, canvas `root`, late `auto`
   re-measures (2026-10-13: replaced by the page's height watch),
   orthogonal agent reads, `view` yields to the human, agent
   glow, page IME — built, installed.
6. Multi-agent canvases (agents never take the screen; `--canvas` on page
   calls; per-agent MCP canvas) and live share (D6: Design serves token
   links; Cloudflare Tunnel) — built, installed.
7. Memory budget (2026-10-14): adaptive reserve, discard idle boards
   (picture kept, reopen on look / call), refuse an agent with the reason,
   `canvas.close`, `memory` in `status` — built, installed; discard under
   real pressure unsmoked.
8. Annotations, exits, flows (2026-10-14): sections, notes, `[[link]]`,
   page exits (`data-design-to`) with arrows and click-to-follow, plain
   board links move the camera, tours — in Design and the share viewer;
   `status` + `view --section` — built,
   installed; arrows through the gutters after the human's look; board
   patches, paced wakes, software compositing (GPU).
