# sola-design

A design canvas where the design is plain HTML/CSS in your project. There
are no drawing tools: an agent edits the files and Design shows them live.
You look, pan, zoom, point, and try things.

## Files

```text
<project>/design/
  tokens.css              optional; shared by canvases (../tokens.css)
  <canvas>/
    canvas.toml           which artboards, where, how big
    settings.html         one page per artboard (or shared between them)
```

`canvas.toml`:

```toml
title = "Settings"              # optional; defaults to the directory name

[[artboard]]
id = "desktop"                  # unique; defaults to the src file stem
src = "settings.html"           # relative path, or an http(s) URL (needs an id)
x = 0                           # position on the canvas (CSS px at 100%)
y = 0
width = 1440                    # the artboard's viewport (CSS px)
height = 900                    # or "auto": grow to the page's content
title = "Settings · desktop"    # optional label
```

Each artboard is its own browser viewport, so media queries and `vw`/`vh`
match its size. Content taller than the artboard is cut off at its bottom
edge — the wheel zooms, it does not scroll. For long pages use
`height = "auto"`: the artboard grows to the page's content after each load
(it starts at 900).

Pages render at 2×, so they stay sharp to 200%; zooming only scales them,
so it is immediate. An artboard too big for 2× (over 4096 px on a side)
drops the whole canvas to 1×.

Saving a file reloads the canvas: `canvas.toml` re-lays out the artboards;
any other file in the canvas, or a shared file in `design/` outside every
canvas, reloads its pages. A broken `canvas.toml` shows the error and keeps
the last good layout.

## Launch

Launcher → **Design**, or `solactl session launch sola-design`. A second
launch raises the open window.

## Projects and canvases

Projects are your own list (not Workspaces'). **Project → Add Project…**
(⌘N) takes a folder (`~` ok). The rail lists each project's canvases —
every `design/<name>/` with a `canvas.toml` — and picks up new ones within
a couple of seconds. `+` on a project makes an empty canvas
(**New Canvas…**, ⌘⇧N). **Drop Project** forgets the open canvas's project;
files are untouched.

## Look

| Do | How |
|----|-----|
| Pan | Right-drag |
| Zoom | Wheel (about the pointer); ⌘= / ⌘- step |
| Fit everything | ⌘0 |
| 100% | ⌘1 |
| Reload the canvas | ⌘R |

## Use the pages, pick an element

Pages are live: hover, click, and type (click a field first) work on the
artboard under the pointer. The wheel still zooms and right-drag still pans.

**Super+click** an element to pick it instead: it gets an outline, and the
bar at the bottom shows it next to the zoom. An agent reads it with
`solactl design selection` or inspects it with `solactl design inspect`. The
outline follows the element across reloads. Super+click empty canvas to
clear it.

Page dialogs (`alert`, `confirm`) are accepted automatically and logins are
cancelled — pages have no browser chrome here.

## Agents

Agents drive Design with `solactl design` (see
[`solactl.md`](solactl.md#design-solactl-design)): open a canvas, steer the
view, wait for reloads, screenshot an artboard, export it as a PDF, inspect
what rendered, and read your selection.

## Not yet

No IME composition in pages. Zooming past 200% softens. An `auto` artboard is measured once per
load, so images that arrive late can leave it short until the next reload.
