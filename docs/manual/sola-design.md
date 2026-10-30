# Design

Design is a canvas where the design is plain HTML and CSS in your project.
There are no drawing tools: an agent edits the files and Design shows them
live. You look, pan, zoom, point, and try things.

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
title = "Settings"              # optional; names the share page (Design shows the folder name)
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

Launcher → **Design**, or `solactl session launch sola-design`. On Oath,
Design is its own package, `pkg:sola-design`, and its pages run on Oath's
CEF (`pkg:cef`). A second launch brings the active window up.

Each window's titlebar carries Design's menus: **Design** (Quit), **Project**,
**Share**, and **View**, then the Window menu. Their shortcuts work while a
page has the keyboard.

## Projects and canvases

Projects are your own list in `~/.config/sola/design/projects.json`, not
the Projects app's folders. **Project → Add Project…** (⌘N) takes a folder
(`~` ok). Each project is its own window — a row in the bar, named for the
project — and Design opens one for every project, the one you used last in
front. With no project yet, one window offers **Add project**.

A project's canvases — every `design/<name>/` with a `canvas.toml` — run
along the top of its window, each labelled with its folder name, and new ones
show up within a couple of seconds. Click one to show it; drag one onto
another to take its place (the order is kept); the wheel scrolls the row
when it is wider than the window. `+` (**New Canvas…**, ⌘⇧N) makes an empty
canvas; its name becomes the folder (`Settings redesign` →
`settings-redesign`). The window remembers which canvas it shows.

Two Design windows can be on screen at once (Left and Right, say), each with
its own canvas, view, pick, tour and notes. The one you last touched takes
the keys and the View and Share menus; moving between them loses nothing.

**Project → Close Window** (⌘⇧W) closes only the window; **Project → Open
*name*** brings it back. Closing the last window quits Design. **Project →
Drop Project** forgets the active window's project and closes its window
(with no project left, the window offering **Add project** comes back), and
`solactl design project.remove --path <folder>` forgets any project. The
files are untouched.

## Look

| Do | How |
|----|-----|
| Pan | Right-drag |
| Zoom | Wheel (about the pointer); ⌘= / ⌘- step (**View → Zoom In / Zoom Out**) |
| Fit everything | ⌘0 (**View → Fit All**) |
| 100% | ⌘⇧0 (**View → Actual Size**; ⌘1 is Super+1, workspace 1) |
| Reload the canvas | ⌘R (**View → Reload Canvas**) |

## Use the pages, pick an element

Pages are live: hover, click, and type (click a field first; input methods
compose as usual) work on the artboard under the pointer. The wheel still
zooms and right-drag still pans.

**Super+click** an element to pick it instead: it gets an outline, and the
bottom bar shows it next to the zoom. An agent reads it with
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
  a spot on the canvas. Hover a marker to read it; click it to open or
  close its card. The notes button on the bottom bar (or **N** while no page
  has the keyboard) opens every note, or, while any card is open, closes
  them all — a tour step's too, and they stay closed on the next steps.
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
The step's notes open by themselves (unless the notes button closed them)
and its outgoing arrows are bright; the others dim. Panning and zooming
never end a tour.

Exits and note positions are read when a page finishes loading and again
when its height changes, so an element that moves for another reason (a
menu that opens) keeps its old arrow until the next reload. Zoomed far out,
arrow labels hide where they do not fit and section titles can overlap.

## Agents

Agents drive Design with `solactl design` or its MCP tools (see
[`solactl.md`](solactl.md#design-solactl-design)): make a canvas in their
project, open a canvas, wait for reloads, screenshot an artboard, export it as
a PDF, inspect what rendered, and read your selection.

Agents never take your screen. A canvas an agent makes or opens shows up
along the top of its project's window (a project an agent registers gets a
window behind yours) and runs off screen; several agents can work on their
own canvases at once. For 30 seconds after an agent's last call on a
canvas, its name along the top and its project's row in the bar show a
spinning ring; click the name to watch. An agent puts a canvas on your
screen only when you ask it to (`--raise`: the canvas comes up in its
project's window). Canvases no window shows that nobody has used for 10
minutes are closed in the background and reopen on the next call.

Boards nobody is looking at slow down to 1 frame a second: a canvas you are
not on, and boards panned out of view. They keep painting, so a board you
come back to shows its page at once. While another window has focus, or
while you are zoomed out below 30%, the boards in view run animations at 1
frame a second too. A board that is loading or that an agent is reading
always runs at full speed, so reloads show at once.

Design keeps its boards within what the machine has free. Every board is a
real browser page and costs memory (about half a gigabyte for a tall page at
2×). Design leaves 15% of the machine's memory (at least 2 GiB) free for
everything else; when free memory drops below that, it discards boards
nobody is using — not in view on your screen, not being worked on by an
agent, untouched for 30 seconds — oldest first. A discarded board keeps its picture on the canvas;
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

Every canvas has a live link, `<public_url>/<token>/` (for example
`https://design.moonlight.pm/<token>/`), where the token is 26 random
characters. The link button on the bottom bar (or **Share → Copy Link**,
⌘⇧L) copies the link of the canvas on screen; the refresh button beside it
(or **Share → New Link (old one stops working)**) makes a new one, copies
it, and the old link stops working. Agents get it with
`solactl design share`.

The link shows the canvas as it is right now, laid out and handled as in
Design: the same artboard heights Design measured, each board's title above
it, and its sections, notes, arrows, and flows (Notes and Flows buttons on
its bar, with the same keys). The pages are live, right-drag pans, the
wheel zooms, and **Fit** (0) frames everything; there is no element
picking and there are no scrollbars.

On a tour the link also lists the step at the right edge: its ways on
(click one to follow it; hover one to light its button and arrow) and its
notes (click one to open or close its card; hover one to show it).
**Hide** folds the list, which starts folded on a screen narrower than
700 px. On a tour, a click in a page that leads nowhere, or on empty
canvas, flashes every button and link that leads on from that board.

**Trackpad** on the link's bar (or **T**) is for a laptop trackpad: two
fingers pan and a pinch zooms, while right-drag still pans. With it off,
the wheel zooms as in Design, and a sideways swipe makes the button pulse
once. The browser remembers the choice. A Safari pinch zooms either way.
On a phone the pages are for looking only: one finger pans anywhere, a
pinch zooms, and there is no on-screen hint.

When a file changes, viewers' pages reload within a couple of seconds. A
link only reaches its own canvas: its folder, shared files in `design/`,
its `root` folder, and the project files its artboards load. Nothing else
in the project is served, and no dotfiles. Links work while Design is
running. There is no password or expiry: anyone with a link sees the
canvas, and **New Link** is how you cut one off.

Design serves the links itself on `127.0.0.1:47820` and, with a
`tunnel_token`, runs a Cloudflare Tunnel connector (`cloudflared`, found on
`PATH` unless `cloudflared` names it) that forwards the `public_url` host
there; the connector stops with Design and restarts if it exits. Settings
are in `~/.config/sola/design/share.toml` (`public_url`, `listen`,
`tunnel_token`, `cloudflared`); keep that file owner-only, because the
tunnel token is a secret. Design writes the link tokens to `share.json`
beside it, owner-only. Without a `public_url`, the links are local
(`http://127.0.0.1:47820/…`). The connector logs to `design-tunnel.log` in
Sola's log directory (`~/.local/state/sola/` on Oath).

