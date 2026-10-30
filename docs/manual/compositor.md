# Compositor

On Oath, Sola's compositor is rivulet, Oath's C port of River 0.4.5. The
compositor draws the screen, owns the input devices, and talks to every
application. It does not decide where windows go: that is the job of a
separate *window manager* client, and in Sola that client is `sola-river`.
`sola-river` is also the bridge between the compositor and
[the bus](bus.md): it turns window events into bus facts, and the shell's
decisions into compositor requests.

Policy lives in the [Shell](sola-shell.md): which windows are on screen,
which has the keyboard, which zone a window is in. `sola-river` applies
those decisions and adds the few rules of its own that this chapter lists.
[Architecture](architecture.md) shows how the pieces fit together.

## The compositor

rivulet serves River's protocols at the same versions, so `sola-river`
drives it as it drives River; this chapter says *the compositor* for
either. River 0.4 is built around the `river-window-management-v1`
protocol: the compositor keeps the scene, the surfaces, and the input
devices, and one window manager client sets window sizes, positions,
stacking, keyboard focus, key bindings, and fullscreen state. It accepts
one window manager at a time.

On Oath, PID 1 runs the compositor as a service (`svc:rivulet`), and
Sola's own processes as services too: `svc:sola-bus`, `svc:sola-call`,
`svc:sola-river`, `svc:sola-shell`, `svc:sola-session`, and
`svc:sola-kvm`. Sola's process manager, `sola`, refuses to start on Oath
([Process manager](process-manager.md)).

### What sola-river uses

| Global | Used for |
|---|---|
| `river_window_manager_v1` (up to version 4) | Windows, focus, stacking, fullscreen, decorations, and the surfaces `sola-river` draws itself (the background picture, the idle fade) |
| `river_xkb_bindings_v1` | The shell's key bindings |
| `river_layer_shell_v1` | Showing layer-shell clients ([Layer shell](#layer-shell)) |
| `river_xkb_config_v1`, `river_input_manager_v1` | NumLock on for each keyboard |
| `river_libinput_config_v1` | Natural scrolling |
| `zwlr_output_manager_v1` | Picking each output's mode |
| `zwlr_screencopy_manager_v1`, `wl_output`, `wl_shm` | Screenshots and `sample` |
| `ext_foreign_toplevel_list_v1`, `ext_foreign_toplevel_image_capture_source_manager_v1`, `ext_image_copy_capture_manager_v1` | Screenshots of one window |
| `zwlr_virtual_pointer_manager_v1`, `zwp_virtual_keyboard_manager_v1` | Synthesized input (`input.*`) |
| `ext_idle_notifier_v1`, `wp_alpha_modifier_v1` | The desk's opacity, the idle fade, and the dim |
| `wp_cursor_shape_manager_v1` | The move and resize cursors |
| `wl_compositor` | Shadows, rounded corners, the background picture, the fade |

Only the first is required: without it `sola-river` exits. Without one of
the others, the feature it serves is missing; `sola-river` warns at start
when `river_xkb_config_v1` is absent.

## sola-river

`sola-river` is three things in one process: the compositor's window
manager (a Wayland client bound to `river_window_manager_v1`), a bus
client, and a call-plane provider with the owner name `compositor`.

At start it looks for the compositor's socket. It reads the name from
`$XDG_RUNTIME_DIR/sola-wayland` when that file exists (Sola's process
manager writes it; on Oath nothing does). Otherwise it waits up to 15 s
for a `wayland-N` socket in the runtime directory that accepts a
connection (the lowest name first, skipping `wayland-x*` and `.lock`
files), then falls back to `$WAYLAND_DISPLAY`, then `wayland-0`. It then
waits up to 5 s for the socket file and exits with status 1, logging
`wayland socket never appeared`, if it does not come. The runtime
directory is `$XDG_RUNTIME_DIR`, else `/run/user/<uid>` when that exists,
else `/tmp`.

If the compositor does not offer the window-management protocol,
`sola-river` exits with `river_window_manager_v1 not advertised`. If
another window manager already holds it, the compositor refuses and
`sola-river` logs `river_window_manager_v1 unavailable`.

### The manage and render cycle

The compositor batches window management into sequences. When something
changes, it sends the new state followed by `manage_start`. The window
manager answers with sizes, focus, key bindings, close and fullscreen
requests, and tiled state, then `manage_finish`. The compositor configures
the windows, sends their new dimensions and `render_start`, and the window
manager answers with positions, stacking order, visibility, borders, and
its own surfaces, then `render_finish`. Everything decided in one cycle
reaches the screen in the same frame.

`sola-river` reads the bus every 20 ms. Stacking, frames, focus, and key
binding changes that arrive between two reads are folded together; if
anything changed, `sola-river` asks the compositor for a new cycle. A
raise, which the shell sends as a new stack, a frame, and a focus change
at once, lands in one cycle.

`sola-river` never blocks that loop: screenshot encoding and picture
decoding run on threads of their own, because River drops a window
manager that stops answering for more than 3 s.

### Who decides what

| Concern | Decided by | What sola-river does |
|---|---|---|
| Stacking and visibility | Shell (`Composition`) | Shows the listed windows bottom to top, hides the rest |
| Keyboard focus | Shell (`Focus`) | Moves the seat's focus to that window |
| Size and position of tiles, of a window in Cinema, and of the shell's own windows | Shell (`Frame`) | Proposes the size, sets the position |
| Size of a floating window | The application, unless the shell gives it a rectangle (a remembered one, a tile floated in place, gamescope's nest size) | Lets it choose; centers it once if nothing placed it |
| Floating | Shell (`WindowFloating`) | Allows move and resize, draws the shadow |
| Tiled state and the rim | Shell (`WindowChrome`) | Tells tiled windows they are tiled; rims windows whose app draws no frame ([Frames, rims, and corners](#frames-rims-and-corners)) |
| Key bindings | Shell (`RegisteredChords`) | Registers them with the compositor, reports presses |
| Fullscreen | The application, or Cinema | Grants it; ends it when focus moves away |
| The background picture | Whoever sets `Background` | Draws it behind everything ([The background picture](#the-background-picture)) |
| The desk's opacity, the idle fade and dim | Settings → Appearance (`Appearance`) | Draws the picture, then black, over the desk ([The idle fade](#the-idle-fade)) |
| Output mode | sola-river | Picks a mode for each output |

## Windows

### Ids

`sola-river` numbers every window, counting from 1 in the order windows
appear. This `window_id` is not reused while `sola-river` runs; it starts
again from 1 when `sola-river` restarts. Every window topic on the bus and
the `windows` call use it.

A window joins the `Windows` list once the compositor has reported both its
app id and its title. Each entry carries `window_id`, `app_id`, `title`,
and `pid`, the process the compositor attributes the window to. The
protocol marks that pid unreliable; Sola uses it to find a window's
process and to notice when the process is gone.

When the compositor reports that a window belongs to another (a dialog or
a file picker), `sola-river` publishes `WindowParent` for it, and retracts
it when the window closes or loses its parent. The shell gives such a
window no row in the bar and stacks it above its parent.

Gamescope's window often reports an empty app id. When a window's app id
is empty and its process's command line names `gamescope`, `sola-river`
sets the app id to `gamescope`, and the title to `Gamescope` if the title
is empty too.

### Stacking and visibility

The shell sends the whole stack as `Composition`: a list of window ids
from bottom to top. On each render `sola-river` shows every listed window,
places them on top in list order, and hides every window not in the list.
A hidden window is not drawn and receives no pointer input; its client
keeps running.

A window that appears before the shell lists it stays hidden until the
shell does. The shell's overlays (launcher, menus, notifications) rely on
this: they stay mapped at 2×2 while dismissed and appear only when the
shell adds them to the stack.

The shell builds the stack, bottom to top, as: the top row and the bar;
the tab each tiled zone on screen shows, with its dialogs; the floating
windows, least recently raised first; any window the bar has not placed;
a window in Cinema; then the open overlays. It lists only the showing
workspace. Every other window (a tab its zone is not showing, a window on
another workspace, an app hidden with Super+H) is left out, so the
compositor hides it and it keeps running. [Shell](sola-shell.md) describes
the bar and workspaces.

When the shell restarts, the windows it lists come back in a new order. If
the bottom window (the old top row) is gone and a new one sits at the
bottom, `sola-river` keeps the previous relative order of the windows that
remain and puts new ones where the shell placed them.

### Focus

The shell sends `Focus` with a window id, and `sola-river` moves the
seat's keyboard focus to that window. It skips the request when that
window already has focus: asking again makes some clients (Electron)
re-enter focus and flash.

The focus policy is the shell's: focus follows the pointer after a 200 ms
pause, without raising, and only a pointer that moves leads it; a click
shows and focuses the window under it. The shell's own windows never take
focus from the pointer. `sola-river` supplies the input for it:
`MouseEntered` and `MouseLeft` as the pointer crosses windows, and
`MouseClicked` when a window is clicked or touched.

`MouseEntered` carries `still` when the pointer did not move since the
last manage sequence: the window under it changed (a tab shown, a close, a
new window, a workspace switch). The shell leaves focus alone for a still
enter. On the pointer's next move, `sola-river` sends the enter again, not
still, unless that window has focus by then. It does the same after
keyboard focus goes to a window other than the one under the pointer, so
focus follows the pointer again once it moves, even within that window.
The compositor reports each move's position as it happens
(`pointer_position`), ahead of the enter it causes.

When the focused window closes, `sola-river` clears focus before the
window is destroyed. While a layer-shell surface holds exclusive keyboard
focus, the compositor ignores focus requests; `sola-river` keeps the
latest one and applies it when the surface lets go (see
[Layer shell](#layer-shell)).

### Size and position

A new window is not drawn until the window manager proposes a size.
`sola-river` first proposes 0×0, which lets the client choose its own size.
If the shell sends a `Frame` for a window that has not drawn its first
buffer yet, `sola-river` holds the size and applies it as an ordinary
resize once the compositor reports the window's first dimensions. Resizing
a client during its first configure can break a GPU client's swapchain.

`sola-river` sends a size or a position only when it differs from the last
one it sent, so the shell can repeat every frame without each window
receiving a new configure. It ignores a `Frame` with a zero or negative
width or height.

A window the shell has put in the stack but never positioned is centered
on the output once. The centering uses the window's maximum-size hint, or
half the output clamped between 800×600 and 1600×1000 when it has none;
the window keeps its own size.

Once a window has both a size and a position, `sola-river` publishes its
rectangle as `WindowGeometry`, and again whenever it changes. The shell
uses it to remember where floating windows were.

Gamescope host windows are sized differently. Before gamescope's first
frame, `sola-river` proposes 1920×1080, because gamescope does not draw at
a size it chooses itself. After the first frame, `sola-river` keeps that
size for 12 s and then accepts a new size at most every 750 ms, so a game
starting inside gamescope has a stable window.

### Zones and workspaces

The shell lays the desk out on a grid and sends each window's rectangle as
a `Frame`. The top row is 38 px tall (a 10 px gap and its 28 px islands).
The bar is docked left, 240 px wide, from y 48. One 10 px gap runs around
the output under the top row, between the bar and the tiles, and between
tiles; the ground shows through it.

| Zone | Rectangle | Key |
|---|---|---|
| Full | The content area right of the bar | `Super+KP_0` |
| Left | The left half of the content area | `Super+KP_4` |
| Right | The right half, one gap from the left | `Super+KP_6` |
| Floating | The window's own size, or its remembered rectangle; a tile floated with the key keeps its place, 50 px smaller on each side | `Super+KP_*` |
| Cinema | The whole output, top row and bar included, in fullscreen state | `Super+KP_Enter` (again to leave) |

Each tiled zone (Full, Left, Right) shows one tab at a time, and Full and
the halves take turns on screen. The shell frames every tiled tab to its
zone's rectangle, shown or not, so showing one needs no resize; the tabs a
zone is not showing are left out of `Composition`. Floating windows
(Scope, Preview, gamescope, and windows moved there with `Super+KP_*`) all
show, above the tiles; a dialog floats above its parent. The shell
remembers a floating app's rectangle (`FloatGeometry`) and restores it
when the app floats again.

The zone keys work with NumLock on or off: the shell registers the keysyms
of both states. Each kit window's titlebar **Window** menu offers these
moves too; its other zone entries do nothing on this layout
([Shell § Window menu](sola-shell.md#window-menu)).

Each workspace is a desk of its own (Super+1…9 shows one, Super+Tab steps
through them). The shell composes only the showing workspace; the
compositor hides the windows on the others, and they keep running.

### Moving and resizing

Only floating windows move and resize, and only when the application asks:
a drag on a title bar the application draws (`xdg_toplevel.move`), or on
an edge or corner (`xdg_toplevel.resize`). `sola-river` then runs the
compositor's pointer operation. The window follows the pointer; a resize
keeps the opposite edges in place; the pointer shows a move or resize
cursor. A window's top edge cannot go above the top row (y 38), and a
resize stops at 100×100. `WindowGeometry` is published once, when the
button is released. Move and resize requests from windows that are not
floating are ignored.

There are no modifier-drag bindings. Super with a click or a drag reaches
the application: the Browser opens a link behind the page on Super+click,
and Design picks the element under it.

### Fullscreen

`sola-river` grants every fullscreen request from an application, on the
first output. For Xwayland games (Wine, Proton, SDL) this keeps the
fullscreen window managed: when the request is refused, some of them open
a separate surface that bypasses the window manager. Cinema uses the same
state: its `Frame` asks for fullscreen.

A window leaves fullscreen when it asks to, when the shell sends it a
`Frame` without fullscreen (another zone key, or `Super+KP_Enter` again),
and when keyboard focus moves to another window. The last rule covers
games that switch to a windowed mode internally without telling the
compositor, and would otherwise stay above everything.

### Frames, rims, and corners

`sola-river` never asks a client for server-side decorations, so every
client draws its own (the protocol's default). Sola's applications draw a
frame: a titlebar with the app's menus, 12 px rounded corners, and a lit
rim. In Cinema they drop it.

The shell publishes each window's `WindowChrome`. `sola-river` tells a
window in a tiled zone (not in Cinema) that it is tiled on every edge, so
clients that draw shadows outside their window drop them. A window whose
app published no menu is not a Sola app and draws no Sola frame
(`rim`); unless it is in Cinema, `sola-river` frames it:

- Tiled, and not fullscreen: rounded 12 px corners, as the kit draws its
  own tiles. Four decoration surfaces above the window, which take no
  input, paint the corners outside the curve with what lies under them
  (the black ground, or the background picture's own pixels there) and a
  1 px rim along the edge. They are painted again when the window's size,
  its focus, the picture, or the theme changes, and, over a picture, when
  the window moves. Windows smaller than 24 px either way get none.
- Otherwise, a square 1 px border. A floating window's corners have other
  windows behind them, so they cannot be painted round.

The rim is opaque: while the window has focus, the theme's `highlight` a
step lighter, as Sola's own windows draw it; otherwise the theme's base
color 7 % lighter.

Floating windows get a drop shadow: a surface below the window, 14 px
larger than the content on each side, with a 12 px corner radius, an 11 px
soft edge, a 3 px downward offset, and 28 % black at its darkest. The
shadow takes no input and is not part of the window's geometry.
Fullscreen windows have no shadow.

### Closing

On `CloseApp` with an app id, `sola-river` sends the compositor's close
request to every window with exactly that app id; on `CloseWindow`, to
that one window. This is the polite close: the application may ask to
save, or ignore it. The shell's Super+Q sends `CloseApp`, and a bar row's
stoplight sends `CloseWindow` for a window that is not a tab
([Session manager](session.md) describes the stop that runs alongside
`CloseApp`).

A client killed outright never tells the compositor its windows closed.
Every 20 ms `sola-river` drops windows whose process no longer exists and
publishes `Windows` again. Without this, a restarted shell would stay
invisible behind its dead predecessor's surfaces.

### Hide

Super+H hides the focused application without closing it. The shell
records it (`AppHidden`) and leaves its windows out of `Composition`, so
`sola-river` hides them; in a tiled zone, another of the zone's tabs shows
instead. [Shell § Hide](sola-shell.md#hide) describes how to bring them
back.

## The background picture

The ground behind the windows is a picture when one is set (`Background`),
and black otherwise. Paint (File → Set as Background), Browser (Use as
Desktop Background) and `solactl background set <image>` set it; Settings
→ Appearance (**Clear**) and `solactl background clear` take it away. Sola
keeps its own copy under `~/.config/sola/background/`, so the original can
move or go.

`sola-river` decodes the picture off its event loop, turns it upright, and
scales it to fill the output, centre-cropped. It shows it on a surface at
the bottom of the stack, the output's size, at the output's full
resolution when the output's scale is a whole number. The surface takes no
input: clicks on the ground behave as before. The picture is decoded again
when the output's size changes. A picture that fails to decode is logged
(`background failed`) and not retried until the picture or the output
changes; the ground stays as it was.

## The idle fade

Settings → Appearance sets how the desk sits on the ground (`Appearance`).
`sola-river` draws all of it the same way: the picture (or black, without
one) on a surface at the top of the stack, made more or less opaque with
`wp_alpha_modifier_v1`. Fading the windows into the picture looks the same
as raising the picture over them. The surface takes no input.

| Setting | Default | Range |
|---|---|---|
| Desk → **Opacity** | 90 % | 50–100 % |
| Idle → **Fade the windows when idle** | On | On or off |
| Idle → **Start after** | 5 min | 1–60 min |
| Idle → **Fade over** | 30 min | 1–120 min |
| Idle → **Then dim the background** | On | On or off |
| Idle → **Dim after** | 60 min | 1–240 min |
| Idle → **Dim over** | 30 min | 1–120 min |

- **At rest**, the windows, the bar and the top row are *Opacity* % opaque
  over the ground: the picture lies over them at `100 − Opacity` %. At
  100 % nothing is drawn.
- **The fade.** After *Start after* minutes with no input, the picture
  rises over everything, from the rest to fully opaque, over *Fade over*
  minutes, a step at most once a second. The top row's flower and agent
  islands (the rectangle the shell sends as `IdleKeep`) stay at the rest.
  Any key or pointer input brings everything back to the rest over a
  second.
- **The dim.** Once the fade is full and only the picture shows, after
  *Dim after* more minutes a black surface rises over everything, the
  flower and the islands included, over *Dim over* minutes, until a tenth
  of the picture's light is left. Input brings it down with the windows. A
  settings change that puts the dim later lifts it at once.

While the fade runs, `sola-river` publishes `IdleFade` with `fading`:
Projects then shows the island of a session whose tab has the keyboard,
since nobody is looking at it. Once the fade is full, it adds `out`: the
shell reports every tab window off screen (`TabWindow`), so the Browser
rests its pages, until input brings the windows back.

Nothing is drawn over a fullscreen window, at rest or idle. While a window
is fullscreen the fade does not run; it starts over once none is. Idle
comes from `ext-idle-notify`, which honors idle inhibitors: an application
that keeps the screen awake stops the fade. No Sola application does, so
the fade also runs over a video playing in a Browser window.

## Outputs

`sola-river` publishes the output's logical size as `OutputGeometry` when
the compositor reports it, and again whenever it changes. The shell lays
out the top row, the bar, and the zones against that size.

At start, and whenever the outputs change, `sola-river` picks a mode for
every enabled output (`wlr-output-management`):

1. Among the modes whose aspect ratio is within 8 % of the panel's physical
   shape (its size in millimeters), the largest one at 60 Hz or more, or
   else the largest one.
2. When the panel reports no physical size, or no mode matches it, the
   largest mode at 60 Hz or more.

Two variables in `sola-river`'s environment change the choice:

| Variable | Effect |
|---|---|
| `SOLA_OUTPUT_MODE=2560x1440` | That size at its highest refresh rate; if the output has no such mode, the mode closest in area at 60 Hz or more. Sizes under 640×480 are ignored. |
| `SOLA_OUTPUT_PICK=preferred` | The largest mode the display marks as preferred, else the default rule. `SOLA_OUTPUT_MODE` wins when both are set. |

`sola-river` does not set output scale, position, or rotation; the
compositor's defaults apply. The log shows the choice as
`picked output mode` and `applying output mode`.

## Seat, keyboard, and pointer

`sola-river` uses the compositor's first seat.

### Keyboard

`sola-river` does not set a keyboard layout or repeat rate; the
compositor's defaults apply.

A keyboard starts with NumLock off, which makes the number pad send
navigation keys. When the compositor offers `river_xkb_config_v1`,
`sola-river` turns NumLock on for each keyboard as it appears: at session
start, when one is plugged in, and when you return from another console.
The NumLock key still toggles it. The log shows
`numlock enabled by default`. Keys typed from another computer through
the KVM come from `sola-kvm`'s own keyboard, which also starts with NumLock
on ([KVM](sola-kvm-operator.md)).

### Key bindings

`sola-river` binds no keys of its own. The shell publishes the chords it
wants as `RegisteredChords`, a list of keysym and modifier pairs.
`sola-river` registers each pair with the compositor
(`river-xkb-bindings-v1`), removes pairs that left the list, and emits
`Chord` when one is pressed and `ChordReleased` when it is released. A
bound chord never reaches the focused application.

The modifier field is a bit set: Shift 1, Ctrl 4, Alt 8, Super 64.

The shell always registers its own chords
([Shell § Keyboard](sola-shell.md#keyboard)), the Super shortcuts of its
own menu, and the media keys without modifiers. It adds the Super
shortcuts of the focused application's menus while that application has
focus, and Escape while an overlay or menu is open.

While a layer-shell surface holds exclusive keyboard focus, `sola-river`
disables every registered chord so the keys reach that surface, and enables
them again when it lets go.

### Pointer

`sola-river` sets the seat's cursor theme from `XCURSOR_THEME` and
`XCURSOR_SIZE` in its environment, `McMojave` and 24 when they are unset.
Sola's package ships the McMojave cursors in its share directory.

When the compositor offers `river_libinput_config_v1`, natural scrolling
is on for every input device that supports it. It is not configurable.

There are no pointer bindings.

## Layer shell

The compositor shows `wlr-layer-shell` surfaces (panels, overlays, input
barriers) only while the window manager holds `river_layer_shell_v1`.
`sola-river` binds it, attaches each output and the seat, and marks the
first output as the default for clients that do not choose one. The log
shows `bound river_layer_shell_v1 (layer-shell clients enabled)`.

Layer surfaces are not windows: they have no window id, are not in
`Windows`, and are not part of `Composition`.

When a layer surface takes exclusive keyboard focus, the compositor stops
honoring the window manager's focus requests. `sola-river` holds the
shell's focus requests and disables the shell's chords until the
compositor reports the focus released. [KVM](sola-kvm-operator.md)
captures the keyboard and pointer this way.

Exclusive zones are not applied. The shell lays out against the whole
output, so a panel that reserves an edge overlaps windows.

## Xwayland

X11 applications (Steam, Wine and Proton games) run through Xwayland.
`sola-session` gives each application `DISPLAY`: the name in
`$XDG_RUNTIME_DIR/sola-display` when that file exists (Sola's process
manager writes it; on Oath nothing does), else the first
`/tmp/.X11-unix/X<n>` socket that accepts a connection.

X11 windows are ordinary windows. They get window ids, focus, zones,
floating, hiding, and closing like Wayland windows, their fullscreen
requests are granted, and, drawing no Sola frame, they get the rim.

## Bus topics

[The bus](bus.md) documents the encoding and the sticky and persistent
rules. Payload fields are listed here.

### Published by sola-river

| Topic | Kind | Payload | When |
|---|---|---|---|
| `Windows` | sticky | List of `{window_id, app_id, title, pid}`, ordered by id | A window's app id, title, or pid arrives or changes; a window closes or is pruned |
| `OutputGeometry` | sticky | `{width, height}` | The output reports its size |
| `WindowGeometry` | sticky, one per `window_id` | `{window_id, x, y, width, height}` | The window's size or position changes; retracted when it closes |
| `WindowParent` | sticky, one per `window_id` | `{window_id, parent}` | The compositor reports the window's parent; retracted when it closes or has none |
| `MouseEntered` | event | `{window_id, still}` | The pointer enters a window, or moves again after a still enter or a focus change |
| `MouseLeft` | event | none | The pointer leaves a window |
| `MouseClicked` | event | `{window_id}` | A window is clicked or touched |
| `Chord` | event | `{keysym, modifiers}` | A registered chord is pressed |
| `ChordReleased` | event | `{keysym, modifiers}` | A registered chord is released |
| `IdleFade` | sticky | `{fading, out}` | The fade starts, becomes full, or ends |
| `WindowFloating` | sticky, one per `window_id` | `{window_id, floating}` | Only after the bus restarts, to restore the floating windows it knows |

### Read by sola-river

| Topic | Sent by | Effect |
|---|---|---|
| `Composition` | Shell | List of `{window_id}`, bottom to top: the stack |
| `Frame` | Shell | `{window_id, x, y, width, height, fullscreen}`: one window's rectangle |
| `Focus` | Shell | `{window_id}`: keyboard focus |
| `WindowFloating` | Shell | `{window_id, floating}`: allows move and resize, adds the shadow |
| `WindowChrome` | Shell | `{window_id, tiled, cinema, rim}`: tiled state, the rim, rounded corners |
| `RegisteredChords` | Shell | List of `{keysym, modifiers}`: the key bindings |
| `CloseApp` | Shell, kit applications, Arcade | An app id: close request to its windows |
| `CloseWindow` | Shell | A window id: close request to that window |
| `Background` | Paint, Browser, `solactl`, Settings | The picture behind the windows; a retraction for the black ground |
| `Appearance` | Settings | The desk's opacity, the idle fade and dim |
| `IdleKeep` | Shell | `{x, y, w, h}`: what the fade keeps at the rest |
| `Theme` | Kit, Shell | The rim's colors |
| `Shutdown` | Shell | `sola-river` exits with status 0 |

## Call methods

The owner is `compositor`. [`solactl compositor`](solactl.md) and the MCP
tools named `compositor_…` reach these methods.

| Method | Parameters | Reply |
|---|---|---|
| `screenshot` | `output` (path), `app`, `window` (title), `x`, `y`, `width`, `height` (region), `format` (`png` or `rgba`) | `{path}`; with `rgba` also `width`, `height`, and `format: "rgba8"` |
| `sample` | `size` (default 15, at most 65) | The pixels around the pointer |
| `cursor` | `visible` (default true) | `{visible}` |
| `windows` | none | `{app_id: [{title, window_id, pid}]}` |
| `input.click` | `x`, `y`, `button` (`left`, `right`, or `middle`; default left) | `{ok: true}` |
| `input.press` | `button` (default left); `x`, `y` to move there first | `{ok: true}` |
| `input.release` | `button` (default left); `x`, `y` to move there first | `{ok: true}` |
| `input.move` | `x`, `y` | `{ok: true}` |
| `input.scroll` | `dx` (default 0), `dy` (default 5; down is positive) | `{ok: true}` |
| `input.key` | `chord`, a key-chord object | `{ok: true}` |

### screenshot

With no `app` and no region, `screenshot` captures the whole first output
with `wlr-screencopy`. With all four of `x`, `y`, `width`, and `height`, it
captures that rectangle of the output, in output coordinates. Neither asks
for the pointer in the image.

With `app`, it captures one window with `ext-image-copy-capture`: the
first window whose app id is `app` and, when `window` is given, whose title
is exactly `window`. It copies the window's own buffer, so the window does
not need to be on top; it is not raised or focused. Without a match the
call fails with `window not found for capture`.

`png`, the default, writes a PNG with fast compression to `output`, or to
`/tmp/sola/screenshots/<ms>.png` when no path is given. `rgba` writes raw
RGBA8 pixels with no header to `output`, or to
`/dev/shm/sola-freeze-<ms>.rgba` (`/tmp/sola/screenshots/<ms>.rgba`
without `/dev/shm`). Missing parent directories are created. Conversion
and encoding run on a separate thread.

One screenshot runs at a time; a second request fails with
`screenshot already in progress`. `solactl compositor screenshot` waits
10 s by default (`--timeout`). The shell's screenshot chords use this
method with `rgba` and copy the image to the clipboard
([Shell § Screenshots](sola-shell.md#screenshots)).

### sample

`sample` captures a square of `size` pixels centered on the pointer and
kept inside the output, and returns it in the reply: `x` and `y` (the
pointer), `left` and `top` (the square's corner), `width` and `height`,
`hot_x` and `hot_y` (the pointer's pixel inside the square), and `pixels`
(base64 RGBA8). An even size is rounded up to the next odd one. One sample
runs at a time (`sample already in progress`), with a 2 s call timeout.
It fails with `no pointer position yet` until the compositor has reported
where the pointer is. [Scope](sola-scope.md) uses it.

### cursor

`visible: false` sets the seat's cursor theme to the reserved name
`sola-cursor-hidden`, and `visible: true` to `sola-cursor-visible`; a
compositor that knows these names stops drawing the pointer, or draws it
again, while the pointer still moves and applications still receive it.
`solactl compositor` has no command for it; call it through the MCP tool
`compositor_cursor`.

### input

`input.click`, `input.press`, `input.release`, `input.move`, and
`input.scroll` drive a virtual pointer (`wlr-virtual-pointer`) on the seat.
Coordinates are absolute, in the output's logical pixels, and kept inside
the output. A click
moves to the point, then presses and releases the button. A press holds the
button down until a release; moves in between make a drag.

`input.key` drives a virtual keyboard (`virtual-keyboard-unstable-v1`)
with a built-in US keymap. Its `chord` is an object: `keycode` (an XKB
keycode, the evdev code plus 8) and the booleans `meta`, `alt`, `ctrl`, and
`shift`. It sets the chord's modifiers, presses and releases the key, then
clears the modifiers, so no modifier stays held.

Synthesized input goes wherever real input would: to the window under the
pointer, or to the focused window.

```bash
solactl compositor input click 640 400
solactl compositor input drag 300 120 900 120 --steps 20
solactl compositor input press 300 120
solactl compositor input move 600 120
solactl compositor input release 900 120
solactl compositor input key Ctrl+Shift+T
```

`solactl compositor input drag X1 Y1 X2 Y2` presses at the first point,
moves to the second in `--steps` moves (12 by default) about 16 ms apart,
and releases there; `--button` picks the button for `click`, `press`,
`release`, and `drag`. `press` and `release` take an optional point.

`solactl compositor input key` builds the chord from text such as
`Meta+Tab`. The modifiers are `Meta` (also `Super`, `Win`, `Cmd`), `Alt`
(`Option`), `Ctrl` (`Control`), and `Shift`. The keys are `A` to `Z`, `0`
to `9`, `Tab`, `Esc` (`Escape`), `Enter` (`Return`), `Backspace` (`BS`),
`Space`, `Left`, `Right`, `Backtick` (`Grave`, or the character itself),
and the keypad keys `KP0`, `KP2`, `KP4`, `KP5`, `KP6`, `KP8`, `KPEnter`,
`KP*` (`KPMultiply`), `KPAdd`, `KP=` (`KPEqual`), and `KP.`
(`KPDecimal`). Names are not case-sensitive. An unknown key or modifier
exits with status 3.

With `SOLA_BOT` set to `1`, `true`, or `yes` in its environment, `solactl`
refuses `compositor screenshot`, `sample`, and `input` and exits with
status 3, so a bot cannot take over the screen. `compositor windows` still
works.

## Restarts

| What restarts | What happens |
|---|---|
| The compositor | `sola-river`'s connection ends with it, and `sola-river` exits with status 1 so that PID 1 starts it again; it then waits up to 15 s for a live Wayland socket. |
| `sola-river` | PID 1 starts it again. The compositor keeps the application windows. The new `sola-river` numbers them from 1, publishes `Windows`, and gets the shell's chords, the chrome and floating records, the picture, the appearance, and the theme again from the bus; the shell sends frames and the stack for the new list. While `sola-river` is down, new windows do not appear and the shell's chords do nothing. |
| `sola-bus` | `sola-river` reconnects at its next read, subscribes again, and publishes `OutputGeometry`, `Windows`, every `WindowGeometry`, and the floating windows it knows, because the bus lost them. |
| `sola-shell` | The old shell's surfaces are pruned once its process is gone; the stack keeps its order (see [Stacking and visibility](#stacking-and-visibility)). |

On `Shutdown` (Exit Sola), `sola-river` exits with status 0.

## Logs

`sola-river` writes to `sola.log` in Sola's log directory, the log every
Sola process shares, in lines tagged `[river]`. On Oath that is
`$XDG_STATE_HOME/sola/sola.log` (`~/.local/state/sola/sola.log`), unless
`SOLA_LOG_DIR` names another directory.

```bash
solactl logs --follow         # the shared log, as it grows
```

At the default level `sola-river` logs each new window
(`new river window`), each `Frame` it receives (`got Frame`), held sizes,
fullscreen changes, pruned windows, chord set changes
(`applied RegisteredChords`), the background picture, the idle fade's
steps (`idle: the fade starts`, `idle: the background dims`,
`input: the windows come back`), and the end of its compositor connection
(`wayland event loop ended`).

## Limits

- One output. `sola-river` sets a mode on every output, but everything
  else assumes one: the shell lays windows out against one output size
  (with several outputs, whichever reported its size last), which also
  sizes the background picture, the fade, and synthesized pointer input;
  fullscreen, screenshots, and the layer-shell default use the first
  output.
- No keyboard layout, repeat rate, or pointer settings beyond NumLock on
  and natural scrolling.
- Layer-shell exclusive zones are ignored.
- No server-side title bars. An application that draws no frame gets the
  rim; its corners are rounded only while it is tiled, and stay square
  while it floats.
- Only floating windows move or resize, and only by their own title bar
  or edges. There is no keyboard move and no modifier-drag.
- The zones are fixed: Full, two halves at 50 / 50, Floating, and Cinema.
- Window ids change when `sola-river` restarts. Until their publishers
  send them again, records keyed by window id from before the restart
  (`WindowGeometry`, `WindowFloating`, `WindowChrome`) can name other
  windows.
- Only the first seat is used.
- `input.key` uses a US keymap regardless of the keyboard's layout. The
  method declares `chord` as text, so a caller that sends text, as the MCP
  tool `compositor_input_key` does, gets an error;
  `solactl compositor input key` sends the object.
- No Sola application keeps the screen awake: the idle fade runs over a
  video playing in a window.
