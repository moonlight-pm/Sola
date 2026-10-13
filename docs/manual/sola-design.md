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
root = "web/public"             # optional: root-absolute URLs look in this project folder first

[[artboard]]
id = "desktop"                  # unique; defaults to the src file stem
src = "settings.html"           # relative path (?query / #fragment kept), or an http(s) URL (needs an id)
x = 0                           # position on the canvas (CSS px at 100%)
y = 0
width = 1440                    # the artboard's viewport (CSS px)
height = 900                    # or "auto": grow to the page's content
title = "Settings · desktop"    # optional label
```

Each artboard is its own Chromium viewport, so media queries and `vw`/`vh`
match its size. Design serves the project over http on this machine only
(`http://<artboard>--<canvas>.<project>.localhost:<port>/design/<canvas>/…`),
with the **project root as `/`**: root-absolute URLs (`/shared/x.css`) resolve
against the project folder, and fetch, ES modules, and iframes work as on a
site. Dotfiles (`.git`, `.env`) are never served; video and audio can seek.
With `root`, a root-absolute URL looks in that folder first and then at the
project root, so a product's `/fonts/x.woff2` works and the artboards' own
`/design/<canvas>/…` URLs still do. One page can show two states with a
query: `src = "light.html?theme=dark"`. Content taller than the artboard is
cut off at its bottom edge — the wheel zooms, it does not scroll. For long
pages use `height = "auto"`: the artboard grows to the page's content after
each load (it starts at 900), and is measured again over the next few seconds
for images that arrive late.

Pages render at 2× (a very large artboard at less), and zooming only scales
them, so it is immediate. When you stop zooming past that, the part of each
artboard on screen re-renders at the screen's scale and sharpens in place —
nothing re-lays out, so text does not move. Zoom goes to 3200%.

If an artboard's page cannot load (its `src` file is missing, or the page
answers 404), its title turns red with the reason.

Saving a file reloads the canvas: `canvas.toml` re-lays out the artboards;
any other file in the canvas, a shared file in `design/` outside every
canvas, or any file a page loaded from elsewhere (a product stylesheet)
reloads its pages. Pages finish loading once web fonts are in, and `auto`
artboards are measured then. A broken `canvas.toml` shows the error and keeps
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

Pages are live: hover, click, and type (click a field first; input methods
compose as usual) work on the artboard under the pointer. The wheel still
zooms and right-drag still pans.

**Super+click** an element to pick it instead: it gets an outline, and the
bar at the bottom shows it next to the zoom. An agent reads it with
`solactl design selection` or inspects it with `solactl design inspect`. The
outline follows the element across reloads. Super+click empty canvas to
clear it.

Page dialogs (`alert`, `confirm`) are accepted automatically and logins are
cancelled — pages have no browser chrome here.

## Agents

Agents drive Design with `solactl design` or its MCP tools (see
[`solactl.md`](solactl.md#design-solactl-design)): make a canvas in their
project (it shows up under that project in the rail), open a canvas, steer the
view, wait for reloads, screenshot an artboard, export it as a PDF, inspect
what rendered, and read your selection.

Look around while an agent works. Its screenshots and PDFs are the whole
artboard however you are zoomed, and your pointer leaves the page while they
are taken, so no hover state gets in. An agent can move your view only until
you pan or zoom that canvas yourself; after that its `view` leaves your
camera alone.

What an agent touches glows: a soft halo with a light sweep over the elements
it inspects, the selection it reads, an artboard it screenshots or prints,
and an artboard reloading after its files changed.

## Not yet

An `auto` artboard is measured for a few seconds after each load; content
that grows later than that waits for the next reload. Opening a different
canvas (you or an agent) replaces the one on screen: Design shows one canvas
at a time, and an agent working on another canvas has to open it. The pages
themselves are shared with agents: text you type into a page is there when
an agent looks.
