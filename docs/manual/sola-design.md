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

[[section]]                     # a titled region around artboards
title = "Settings"
boards = ["desktop", "phone"]
note = "Desktop first; the phone stacks the panes."   # optional

[[note]]                        # a numbered note: board (+ at = "css selector"), or x / y
board = "desktop"
at = ".danger-zone"
text = "Delete is the only red button in the app."

[[link]]                        # an arrow between whole artboards
from = "desktop"
to = "phone"
label = "under 600 px"          # optional

[[flow]]                        # a tour that follows exits and links
title = "Change your email"
start = "desktop"
note = "Verify, then the new address takes over."     # optional
```

In a page, `data-design-to="<artboard id>"` (and optionally
`data-design-label="…"`) on a button or link makes it an **exit**: an arrow
from it to that artboard, and a click there moves the camera. Names that are
not artboards make `canvas.toml` an error; an exit to an unknown artboard,
or a note whose `at` matches nothing, is listed in `solactl design status`.

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
each load (it starts at 900) and keeps following it while the page is open —
images, fetched data, or a section you open grow or shrink it. A page sized
in `vh` grows with its own artboard; Design stops that after a few steps per
load, so give `auto` pages content heights, not viewport ones.

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
(**New Canvas…**, ⌘⇧N). Drag a canvas to reorder it within its project;
drag a project's header to move the whole project. The order is kept.
**Drop Project** forgets the open canvas's project (`solactl design
project.remove --path <folder>` forgets any); files are untouched.

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

You and agents use the same pages: text you type, a menu you open, or a
scroll inside a page is what an agent sees in its next screenshot, and an
agent's reload (or a file edit) clears it.

Page dialogs (`alert`, `confirm`) are accepted automatically and logins are
cancelled — pages have no browser chrome here.

## Sections, notes, and flows

A big canvas can say how it is organised. Agents add this; you read and
walk it. All of it is drawn by Design over the canvas, at a fixed size on
screen, and none of it is in the pages or in an agent's screenshots.

- **Sections** are titled regions behind a group of artboards, with an
  optional line under the title. A fit leaves room above for their titles.
- **Notes** are numbered amber markers on an artboard, an element in it, or
  a spot on the canvas. Hover a marker to read it; click it to keep it open.
  The notes button on the bottom bar (or **N** while no page has the
  keyboard) opens every note.
- **Arrows** show flow, through the gaps between artboards (never across
  one): from the edge of an artboard, level with the button or link that
  leads on, to the artboard it leads to — or between two whole artboards.
  A label shows where it fits without covering an artboard, and always on
  a lit arrow. Hover the button to outline it and light its arrow. Leave
  room between artboards and the arrows (and labels) have room to run.
- **Clicking** a button or link that leads to another artboard moves the
  camera there (a short glide) instead of loading that page into the
  artboard you clicked. That includes plain links between artboards, which
  draw no arrow — so a sidebar's links take you around the canvas.

**Tours.** When a canvas has flows, the bottom bar has a flows button. Pick
a flow and the camera frames its first artboard, with the tour bar above
the bottom bar: back, where you are (flow, step, artboard), next, **Return**
(frame the step again after looking around), and end. Where an artboard
leads several places, the tour bar lists them; pick one (or press its
number). Clicking a button in the page follows it too, and outside a tour
that starts one, so Back takes you back. **→** next, **←** back, **1–9** pick,
**Esc** ends — while no page has the keyboard (click empty canvas first).
The step's notes are open and its outgoing arrows are bright; the others
dim. Panning and zooming never end a tour.

Exits and note positions are read when a page finishes loading and again
when its height changes, so an element that moves for another reason (a
menu that opens) keeps its old arrow until the next reload. Zoomed far out,
arrow labels hide where they do not fit and section titles can overlap.

## Agents

Agents drive Design with `solactl design` or its MCP tools (see
[`solactl.md`](solactl.md#design-solactl-design)): make a canvas in their
project, open a canvas, wait for reloads, screenshot an artboard, export it as
a PDF, inspect what rendered, and read your selection.

Agents never take your screen. A canvas an agent makes or opens is listed in
the rail and runs off screen; several agents can work on their own canvases
at once. While an agent is working in a canvas its rail row says **agent**;
pick it to watch. An agent puts a canvas on your screen only when you ask it
to (`--raise`). Canvases nobody has used for 10 minutes are closed in the
background and reopen on the next call.

Boards nobody is looking at sleep, like background browser tabs: a canvas
you are not on (unless an agent is working in it), and boards panned out of
view. While another window has focus, the boards in view run animations at
1 frame a second. A board that is loading or that an agent is reading always
runs at full speed, so reloads show at once.

Design keeps its boards within what the machine has free. Every board is a
real browser page and costs memory (about half a gigabyte for a tall page at
2×). Design leaves 15% of the machine's memory free for everything else; when
free memory drops below that, it discards boards nobody is using — not in
view on your screen, not being worked on by an agent, untouched for 30
seconds — oldest first. A discarded board keeps its picture on the canvas;
its page reloads from its files when you pan to it or an agent needs it.
Boards in use are never discarded. If an agent asks for more than fits even
then, it is told so — how much memory is free, what is open and why — and
asked to close a canvas it is done with (`canvas.close`) or wait. Your own
view is never refused.

Look around while an agent works. Its screenshots and PDFs are the whole
artboard however you are zoomed, and your pointer leaves the page while they
are taken, so no hover state gets in. An agent can frame a canvas's view until
you pan or zoom it yourself; after that its `view` leaves your camera alone.

What an agent touches glows: a soft halo with a light sweep over the elements
it inspects, the selection it reads, an artboard it screenshots or prints,
and an artboard reloading after its files changed.

## Share

Every canvas has a live link, `https://design.moonlight.pm/<token>/`, where
the token is 26 random characters. The link button on the bottom bar (or
**Share → Copy Link**, ⌘⇧L) copies the link of the canvas on screen; the
refresh button beside it (or **Share → New Link**) makes a new one, copies
it, and the old link stops working. Agents get it with `solactl design share`.

The link shows the canvas as it is right now, laid out and handled as in
Design — the same artboard heights Design measured, each board's title
above it, its sections, notes, arrows, and flows (Notes and Flows buttons on
its bar, the same keys), no scrollbars: the pages are live, right-drag pans, the wheel zooms, **Fit** (0)
frames everything (no element picking). On a phone the pages are for
looking only: one finger pans anywhere, a pinch zooms, and there is no hint. When a file changes, viewers' pages reload within a couple
of seconds. A link only reaches its own canvas: its folder, shared files in
`design/`, its `root` folder, and the project files its artboards load —
nothing else in the project, and no dotfiles. Links work while Design is
running. There is no password or expiry: anyone with a link sees the canvas,
and **New Link** is how you cut one off.

Design serves the links itself on `127.0.0.1:47820` and runs a Cloudflare
Tunnel connector (`cloudflared`) that forwards `design.moonlight.pm` there.
Settings are in `~/.config/sola/design/share.toml` (`public_url`, `listen`,
`tunnel_token`, `cloudflared`; owner-only — the token is a secret); tokens
are in `share.json` beside it. Without a `tunnel_token` the links are local
(`http://127.0.0.1:47820/…`). The connector logs to
`/opt/sola/log/design-tunnel.log`.

