# Compositor

Sola's compositor is River, an external Wayland compositor. River draws
the screen, owns the input devices, and talks to every application. It
does not decide where windows go: that is the job of a separate *window
manager* client, and in Sola that client is `sola-river`. `sola-river` is
also the bridge between River and [the bus](bus.md): it turns River's
window events into bus facts, and the shell's decisions into River
requests.

Policy lives in the [Shell](sola-shell.md): which window is on top, which
has the keyboard, which zone a window snaps to. `sola-river` applies those
decisions and adds the few rules of its own that this chapter lists.
[Architecture](architecture.md) shows how the pieces fit together.

## River

Sola runs River 0.4.5. River 0.4 is built around the
`river-window-management-v1` protocol: River keeps the scene, the
surfaces, and the input devices, and one window manager client sets
window sizes, positions, stacking, keyboard focus, key bindings, and
fullscreen state. River accepts one window manager at a time.

The NixOS module installs River from Sola's own pinned nixpkgs, built
against wlroots 0.20 with the patches below
([`nix/patches/`](../../nix/patches/)). Use this build:
River 0.3 has no window-management protocol, and an unpatched 0.4 lacks
the fixes.

### How River starts

The process manager starts River before every other component (see
[Process manager](process-manager.md)). It first kills any `river`
process still running under your user and removes stale `wayland-N`
sockets from `$XDG_RUNTIME_DIR`. Then it runs the first `river` on `PATH`:

```text
river -log-level info -c :
```

`-c :` makes River's init command a no-op, so River does not run
`~/.config/river/init`. `WAYLAND_DISPLAY` and `DISPLAY` are removed from
River's environment, so wlroots drives the screen and input directly
(DRM and libinput) instead of nesting inside another session.

The process manager waits up to 30 s for River's socket to accept a
connection and writes its name (`wayland-1`) to
`$XDG_RUNTIME_DIR/sola-wayland`; every Sola process reads the socket name
from that file. It then waits up to 3 s for Xwayland and writes its
display (`:0`) to `$XDG_RUNTIME_DIR/sola-display`.

River writes its own output to `/opt/sola/log/river.log`.

River keeps Ctrl+Alt+F1 to Ctrl+Alt+F12 for itself (switch virtual
terminal). No window manager can take those keys.

If River exits, the session ends: the process manager stops every
component and exits with status 1. Start `sola` again from the console.

### Patches

| Patch | What it fixes |
|---|---|
| `river-xwayland-destroy-state.patch` | River aborted when an Xwayland window was destroyed without being unmapped first: an X client crashing, an override-redirect window going away, gamescope exiting. The patch moves such a window to the closing state, as River already does for Wayland windows. |
| `river-live-pointer-position.patch` | River sent the window manager the pointer position only during a manage sequence (hover change, key press, new window). The patch sends it on every pointer motion, so `sola-river` always knows where the pointer is. Scope and `compositor.sample` depend on it. |
| `river-hide-cursor.patch` | Adds two reserved cursor theme names. Setting the seat's cursor theme to `sola-cursor-hidden` stops River drawing the pointer while it keeps moving; `sola-cursor-visible` draws it again. This is the `compositor.cursor` method. |
| `wlroots-screencopy-omit-sw-cursor.patch` | With a software cursor, wlroots drew the pointer into the frame before `wlr-screencopy` copied it, so screenshots contained the pointer even when they asked for none. The patch copies the frame before the pointer is drawn, and redraws the area under the pointer on the next frame. |

## sola-river

`sola-river` is three things in one process: River's window manager (a
Wayland client bound to `river_window_manager_v1`), a bus client, and a
call-plane provider with the owner name `compositor`. The process manager
starts it after River and restarts it whenever it exits.

It reads River's socket name from `$XDG_RUNTIME_DIR/sola-wayland` and exits
if that file is missing, so it runs only inside a Sola session. If River
does not offer the window-management protocol, it exits with
`river_window_manager_v1 not advertised`. If another window manager
already holds it, River refuses and `sola-river` logs
`river_window_manager_v1 unavailable`.

### The manage and render cycle

River batches window management into sequences. When something changes,
River sends the new state followed by `manage_start`. The window manager
answers with sizes, focus, key bindings, close and fullscreen requests,
then `manage_finish`. River configures the windows, sends their new
dimensions and `render_start`, and the window manager answers with
positions, stacking order, visibility, and decorations, then
`render_finish`. Everything decided in one cycle reaches the screen in
the same frame.

`sola-river` reads the bus every 20 ms. Stacking, frames, focus, and key
binding changes that arrive between two reads are folded together; if
anything changed, `sola-river` asks River for a new cycle. A raise, which
the shell sends as a new stack, a frame, and a focus change at once,
lands in one cycle.

### Who decides what

| Concern | Decided by | What sola-river does |
|---|---|---|
| Stacking and visibility | Shell (`Composition`) | Shows the listed windows bottom to top, hides the rest |
| Keyboard focus | Shell (`Focus`) | Moves the seat's focus to that window |
| Size and position of zoned windows and the shell's own | Shell (`Frame`) | Proposes the size, sets the position |
| Size of any other window | The application | Lets it choose, centers it once |
| Floating | Shell (`WindowFloating`) | Allows move and resize, draws the shadow |
| Key bindings | Shell (`RegisteredChords`) | Registers them with River, reports presses |
| Fullscreen | The application, or the Cinema zone | Grants it; ends it when focus moves away |
| Output mode | sola-river | Picks a mode for each output |
| Idle fade | sola-river | With no input for *Start after* minutes (Settings → Appearance → Idle; 5 by default), the windows fade into the background picture (or black) over *Fade over* minutes (30 by default); any key or pointer input brings them back over a second. The flower and the agent islands at the top left never fade below 20%. Not while a window is fullscreen or an app keeps the screen awake. Drawn as the picture over everything at a rising opacity (`ext-idle-notify`, `wp_alpha_modifier_v1`) |
| Background picture | sola-river | The picture behind the windows (`Topic::Background`; Paint's Set as Background, `solactl background`): filling the output, centre-cropped, under everything; clicks pass through it. Without one the ground is black |
| Borders and decorations | sola-river | Sola's applications draw their own frame. For other applications' windows, a 1 px rim (while focused, the theme's highlight a step lighter, as Sola's own windows draw it; a faint white otherwise); a tiled one gets 12 px rounded corners with the rim along them (the corners show the picture under them), a floating one keeps square corners. A shadow under floating windows |

## Windows

### Ids

`sola-river` numbers every River window, counting from 1 in the order
windows appear. This `window_id` is not reused while `sola-river` runs; it
starts again from 1 when `sola-river` restarts. Every window topic on the
bus and the `windows` call use it.

A window joins the `Windows` list once River has reported both its app id
and its title. Each entry carries `window_id`, `app_id`, `title`, and
`pid`, the process River attributes the window to. River marks that pid
unreliable; Sola uses it to find a window's process and to notice when the
process is gone.

Gamescope's window often reports an empty app id under River. When a
window's app id is empty and its process's command line names `gamescope`,
`sola-river` sets the app id to `gamescope`, and the title to `Gamescope`
if the title is empty too.

### Stacking and visibility

The shell sends the whole stack as `Composition`: a list of window ids
from bottom to top. On each render `sola-river` shows every listed window,
places them on top in list order, and hides every window not in the list.
A hidden window is not drawn and receives no pointer input; its client
keeps running.

A window that appears before the shell lists it stays hidden until the
shell does. The shell's overlays (launcher, switcher, menus) rely on this:
they stay mapped at 2×2 while dismissed and appear only when the shell
adds them to the stack.

The shell builds the stack as the menubar, then application windows from
least to most recently used, then any open overlays on top.
[Shell](sola-shell.md) describes raising and the switcher.

When the shell restarts, it rebuilds its recent-use order from window ids
and would shuffle the windows. If the bottom window (the old menubar) is
the only change, `sola-river` keeps the previous relative order of the
windows that remain and puts new ones where the shell placed them.

### Focus

The shell sends `Focus` with a window id, and `sola-river` moves the
seat's keyboard focus to that window. It skips the request when that
window already has focus: asking again makes some clients (Electron)
re-enter focus and flash.

The focus policy is the shell's: focus follows the pointer after a 200 ms
pause, without raising; a click raises; Super+Tab raises. `sola-river`
supplies the input for it: `MouseEntered` and `MouseLeft` as the pointer
crosses windows, and `MouseClicked` when a window is clicked or touched.

When the focused window closes, `sola-river` clears focus before the
window is destroyed. While a layer-shell surface holds exclusive keyboard
focus, River ignores focus requests; `sola-river` keeps the latest one and
applies it when the surface lets go (see [Layer shell](#layer-shell)).

### Size and position

A new window is not drawn until the window manager proposes a size.
`sola-river` first proposes 0×0, which lets the client choose its own size.
If the shell sends a `Frame` for a window that has not drawn its first
buffer yet, `sola-river` holds the size and applies it as an ordinary
resize once River reports the window's first dimensions. Resizing a client
during its first configure can break a GPU client's swapchain.

`sola-river` sends a size or a position only when it differs from the last
one it sent, so the shell can repeat every frame without each window
receiving a new configure. It ignores a `Frame` with a zero or negative
width or height.

A window the shell has put in the stack but never positioned is centered
on the output once. The centering uses the window's maximum-size hint, or
a size between 800×600 and 1600×1000 when it has none; the window keeps
its own size.

Once a window has both a size and a position, `sola-river` publishes its
rectangle as `WindowGeometry`, and again whenever it changes. The shell
uses it to remember where floating windows were.

Gamescope host windows are sized differently. Before gamescope's first
frame, `sola-river` proposes 1920×1080, because gamescope does not draw at
a size it chooses itself. After the first frame, `sola-river` keeps that
size for 12 s and then accepts a new size at most every 750 ms, so a game
starting inside gamescope has a stable window.

### Floating and zones

An application window floats unless its application has a saved zone. A
floating window keeps the size it chose, is centered when it first
appears, moves and resizes through its own title bar, and has a shadow.
The shell publishes which windows float as `WindowFloating`; kit
applications draw their title bar only while floating.

Super+Numpad snaps the focused window to a zone. The shell computes the
rectangle, sends it as a `Frame`, and saves the zone for that app id
(`Zones`, kept across restarts); each new window of the application opens
in that zone. Zones sit below the 28 px menubar and are fractions of the
area under it:

| Key | Zone | Rectangle |
|---|---|---|
| `Super+KP_4` | Left | Left 28%, full height |
| `Super+KP_5` | Middle | Middle 44%, full height |
| `Super+KP_6` | Right | Right 28%, full height |
| `Super+KP_+` | Middle and right | Right 72%, full height |
| `Super+KP_8` | Top middle | Middle 44%, top 70% |
| `Super+KP_2` | Bottom middle | Middle 44%, bottom 30% |
| `Super+KP_=` | Top | Full width, top 70% |
| `Super+KP_.` | Bottom | Full width, bottom 30% |
| `Super+KP_0` | Fullscreen | The whole area under the menubar |
| `Super+KP_Enter` | Cinema | The whole output, menubar included, in fullscreen state |
| `Super+KP_*` | Float | The window's current rectangle, 50 px smaller on each side |

The zone keys work with NumLock on or off: the shell registers the keysyms
of both states. `Super+KP_*` saves the application as floating; the shell
then remembers its rectangle (`FloatGeometry`) and restores it when the
application opens again; on a window that already floats it does nothing.
Any other zone key ends floating. The **Window**
menu offers every zone too ([Shell § Window menu](sola-shell.md#window-menu)).

### Moving and resizing

Only floating windows move and resize, and only when the application asks:
a drag on a title bar the application draws (`xdg_toplevel.move`), or on
an edge or corner (`xdg_toplevel.resize`). `sola-river` then runs River's
pointer operation. The window follows the pointer; a resize keeps the
opposite edges in place; the pointer shows a move or resize cursor. A
window's top edge cannot go above the menubar, and a resize stops at
100×100. `WindowGeometry` is published once, when the button is released.
Move and resize requests from windows that are not floating are ignored.

There are no modifier-drag bindings. Super with a click or a drag reaches
the application (the Browser uses Super+click to open a link in a new
tab).

### Fullscreen

`sola-river` grants every fullscreen request from an application, on the
first output. For Xwayland games (Wine, Proton, SDL) this keeps the
fullscreen window managed: when the request is refused, some of them open
a separate surface that bypasses the window manager. The Cinema zone uses
the same state: its `Frame` asks for fullscreen.

A window leaves fullscreen when it asks to, when the shell sends it a
`Frame` without fullscreen (another zone key), and when keyboard focus
moves to another window. The last rule covers games that switch to a
windowed mode internally without telling the compositor, and would
otherwise stay above everything.

### Decorations

`sola-river` turns River's borders off for every window and never asks a
client for server-side decorations, so every client draws its own (the
protocol's default). Kit applications draw a title bar while floating; a
client that draws nothing has no title bar.

Floating windows get a drop shadow: a surface below the window, 14 px
larger than the content on each side, with a 14 px corner radius, an 11 px
soft edge, a 3 px downward offset, and 28% black at its darkest. The
shadow takes no input and is not part of the window's geometry.
Fullscreen windows have no shadow.

### Closing

On `CloseApp` with an app id, `sola-river` sends River's close request to
every window with exactly that app id. This is the polite close: the
application may ask to save, or ignore it. The shell's Super+Q sends
`CloseApp`; [Session manager](session.md) describes the stop that runs
alongside it.

A client killed outright never tells River its windows closed. Every
20 ms `sola-river` drops windows whose process no longer exists and
publishes `Windows` again. Without this, a restarted shell would stay
invisible behind its dead predecessor's surfaces.

### Hide

Super+H hides the focused application without closing it. The shell
records it (`AppHidden`) and leaves its windows out of `Composition`, so
`sola-river` hides them. [Shell § Hide](sola-shell.md#hide) describes how
to bring them back.

## Outputs

`sola-river` publishes the output's logical size as `OutputGeometry` when
River reports it, and again whenever it changes. The shell lays out the
menubar and the zones against that size.

At start, and whenever the outputs change, `sola-river` picks a mode for
every enabled output (`wlr-output-management`):

1. Among the modes whose aspect ratio is within 8% of the panel's physical
   shape (its size in millimeters), the largest one at 60 Hz or more, or
   else the largest one.
2. When the panel reports no physical size, or no mode matches it, the
   largest mode at 60 Hz or more.

Two environment variables change the choice. Set them where you start
`sola`:

| Variable | Effect |
|---|---|
| `SOLA_OUTPUT_MODE=2560x1440` | That size at its highest refresh rate; if the output has no such mode, the mode closest in area at 60 Hz or more. Sizes under 640×480 are ignored. |
| `SOLA_OUTPUT_PICK=preferred` | The largest mode the display marks as preferred, else the default rule. `SOLA_OUTPUT_MODE` wins when both are set. |

```bash
SOLA_OUTPUT_MODE=2560x1440 /opt/sola/bin/sola
```

`sola-river` does not set output scale, position, or rotation; River's
defaults apply. The log shows the choice as `picked output mode` and
`applying output mode`.

## Seat, keyboard, and pointer

`sola-river` uses River's first seat.

### Keyboard

`sola-river` does not set a keyboard layout or repeat rate; River's
defaults apply.

River starts each keyboard with NumLock off, which makes the number pad
send navigation keys. `sola-river` turns NumLock on for each keyboard as it
appears: at session start, when one is plugged in, and when you return
from another console. The NumLock key still toggles it. The log shows
`numlock enabled by default`.

### Key bindings

`sola-river` binds no keys of its own. The shell publishes the chords it
wants as `RegisteredChords`, a list of keysym and modifier pairs.
`sola-river` registers each pair with River (`river-xkb-bindings-v1`),
removes pairs that left the list, and emits `Chord` when one is pressed and
`ChordReleased` when it is released. A bound chord never reaches the
focused application.

The modifier field is a bit set: Shift 1, Ctrl 4, Alt 8, Super 64.

The shell always registers its own chords ([Shell § Keyboard](sola-shell.md#keyboard)),
the Super shortcuts of its menus, left Super on its own (so it sees Super
released and can confirm the switcher), and the media keys without
modifiers. It adds the Super shortcuts of the focused application's menus
while that application has focus, Escape while an overlay or menu is open,
and Super+Left and Super+Right while the switcher is open.

While a layer-shell surface holds exclusive keyboard focus, `sola-river`
disables every registered chord so the keys reach that surface, and enables
them again when it lets go.

### Pointer

`sola-river` sets River's cursor theme for the seat from `XCURSOR_THEME`
and `XCURSOR_SIZE` (defaults `McMojave` and 24). The process manager sets
`XCURSOR_THEME=McMojave` unless you set it, and puts
`/opt/sola/share/cursors` first on `XCURSOR_PATH`. River uses a hardware
cursor plane where the driver allows one.

Natural scrolling is on for every input device that supports it. It is not
configurable.

There are no pointer bindings.

## Layer shell

River shows `wlr-layer-shell` surfaces (panels, overlays, input barriers)
only while the window manager holds `river_layer_shell_v1`. `sola-river`
binds it, attaches each output and the seat, and marks the first output as
the default for clients that do not choose one. The log shows
`bound river_layer_shell_v1 (layer-shell clients enabled)`.

Layer surfaces are not windows: they have no window id, are not in
`Windows`, and are not part of `Composition`.

When a layer surface takes exclusive keyboard focus, River stops honoring
the window manager's focus requests. `sola-river` holds the shell's focus
requests and disables the shell's chords until River reports the focus
released. [KVM](sola-kvm-operator.md) captures the keyboard and pointer
this way; the process manager starts it 750 ms after the other components
so the layer shell is bound first.

Exclusive zones are not applied. The shell lays out the menubar and the
zones against the whole output, so a panel that reserves an edge overlaps
windows.

## Xwayland

River is built with Xwayland support and starts the X server itself (the
NixOS module puts `Xwayland` on `PATH`). The session manager gives each
application `DISPLAY`, taken from `$XDG_RUNTIME_DIR/sola-display` or from
the first live socket in `/tmp/.X11-unix`.

X11 windows are ordinary River windows. They get window ids, focus, zones,
floating, hiding, and closing like Wayland windows, and their fullscreen
requests are granted. The Xwayland patch above keeps River running when an
X client disappears without unmapping its windows.

## Bus topics

[The bus](bus.md) documents the encoding and the sticky and persistent
rules. Payload fields are listed here.

### Published by sola-river

| Topic | Kind | Payload | When |
|---|---|---|---|
| `Windows` | sticky | List of `{window_id, app_id, title, pid}`, ordered by id | A window's app id, title, or pid arrives or changes; a window closes or is pruned |
| `OutputGeometry` | sticky | `{width, height}` | The output reports its size |
| `WindowGeometry` | sticky, one per `window_id` | `{window_id, x, y, width, height}` | The window's size or position changes; retracted when it closes |
| `MouseEntered` | event | `{window_id}` | The pointer enters a window |
| `MouseLeft` | event | none | The pointer leaves a window |
| `MouseClicked` | event | `{window_id}` | A window is clicked or touched |
| `Chord` | event | `{keysym, modifiers}` | A registered chord is pressed |
| `ChordReleased` | event | `{keysym, modifiers}` | A registered chord is released |
| `WindowFloating` | sticky, one per `window_id` | `{window_id, floating}` | Only after the bus restarts, to restore the floating windows it knows |

### Read by sola-river

| Topic | Sent by | Effect |
|---|---|---|
| `Composition` | Shell | List of `{window_id}`, bottom to top: the stack |
| `Frame` | Shell | `{window_id, x, y, width, height, fullscreen}`: one window's rectangle |
| `Focus` | Shell | `{window_id}`: keyboard focus |
| `WindowFloating` | Shell | `{window_id, floating}`: allows move and resize, adds the shadow |
| `RegisteredChords` | Shell | List of `{keysym, modifiers}`: the key bindings |
| `CloseApp` | Shell, kit applications | An app id: close request to its windows |
| `Shutdown` | Shell, process manager | `sola-river` exits with status 0 |

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
| `input.move` | `x`, `y` | `{ok: true}` |
| `input.scroll` | `dx` (default 0), `dy` (default 5; down is positive) | `{ok: true}` |
| `input.key` | `chord`, a key-chord object (`solactl` builds it from text) | `{ok: true}` |

### screenshot

With no `app` and no region, `screenshot` captures the whole first output
with `wlr-screencopy`. With all four of `x`, `y`, `width`, and `height`, it
captures that rectangle of the output, in output pixels. Neither includes
the pointer.

With `app`, it captures one window with `ext-image-copy-capture`: the
first window whose app id is `app` and, when `window` is given, whose title
is exactly `window`. It copies the window's own buffer, so the window may
be covered or hidden with Super+H; it is not raised or focused. Without a
match the call fails with `window not found for capture`.

`png`, the default, writes a PNG with fast compression to `output`, or to
`/tmp/sola/screenshots/<ms>.png` when no path is given. `rgba` writes raw
RGBA8 pixels with no header to `output`, or to
`/dev/shm/sola-freeze-<ms>.rgba`. Missing parent directories are created.
Conversion and encoding run on a separate thread: River drops a window
manager that stops answering for more than 3 s.

One screenshot runs at a time; a second request fails with
`screenshot already in progress`. `solactl compositor screenshot` waits
10 s by default (`--timeout`). The shell's screenshot chords use this
method and copy the image to the clipboard
([Shell § Screenshots](sola-shell.md#screenshots)).

### sample

`sample` captures a square of `size` pixels centered on the pointer and
kept inside the output, and returns it in the reply: `x` and `y` (the
pointer), `left` and `top` (the square's corner), `width` and `height`,
`hot_x` and `hot_y` (the pointer's pixel inside the square), and `pixels`
(base64 RGBA8). An even size is rounded up to the next odd one. One sample
runs at a time, with a 2 s deadline. It fails with
`no pointer position yet` until River has reported where the pointer is.
[Scope](sola-scope.md) uses it.

### cursor

`visible: false` stops River drawing the pointer; the pointer still moves
and applications still receive it. `visible: true` draws it again. This
needs the `river-hide-cursor` patch. `solactl compositor` has no command
for it; call it through the MCP tool `compositor_cursor`.

### input

`input.click`, `input.move`, and `input.scroll` drive a virtual pointer
(`wlr-virtual-pointer`) on the seat. Coordinates are absolute output
pixels, kept inside the output. A click moves to the point, then presses
and releases the button.

`input.key` drives a virtual keyboard (`virtual-keyboard-unstable-v1`)
with a built-in US keymap. It sets the chord's modifiers, presses and
releases the key, then clears the modifiers, so no modifier stays held.

Synthesized input goes wherever real input would: to the window under the
pointer, or to the focused window.

```bash
solactl compositor input click 640 400
solactl compositor input key Ctrl+Shift+T
```

`solactl compositor input key` reads chords such as `Meta+Tab`. It knows
Tab, Esc, Enter, Backspace, Space, Left, Right, A to Z, and 0 to 9, and the
modifiers Meta (also Super, Win, Cmd), Alt (Option), Ctrl, and Shift.

With `SOLA_BOT=1` in its environment, `solactl` refuses
`compositor screenshot`, `sample`, and `input` and exits with status 3, so
a bot cannot take over the screen. `compositor windows` still works.

## Restarts

| What restarts | What happens |
|---|---|
| River | The session ends. Run `sola` again. |
| sola-river | The process manager starts it again (after 2 s if it ran for less than 5 s). River keeps the application windows. The new `sola-river` numbers them from 1, publishes `Windows`, and registers the shell's chords again from the bus; the shell sends frames and the stack for the new list. While `sola-river` is down, new windows do not appear and the shell's chords do nothing. |
| sola-bus | `sola-river` reconnects at its next read, subscribes again, and publishes `OutputGeometry`, `Windows`, every `WindowGeometry`, and the floating windows it knows, because the bus lost them. |
| sola-shell | The old shell's surfaces are pruned once its process is gone; the stack keeps its order (see [Stacking and visibility](#stacking-and-visibility)). |

When its connection to River ends, `sola-river` exits with status 1.
River ends that connection when the window manager stops answering for
more than 3 s.

## Logs

| File | Contents |
|---|---|
| `/opt/sola/log/river.log` | River's own output (`-log-level info`), appended across sessions |
| `/opt/sola/log/sola.log` | `sola-river`'s log, lines tagged `[river]`, in the log every component shares |

```bash
solactl logs river            # River's log
solactl logs --follow         # the shared log, as it grows
RUST_LOG=debug /opt/sola/bin/sola   # more detail, for one session
```

At the default level `sola-river` logs each new window
(`new river window`), each `Frame` it receives (`got Frame`), held sizes,
fullscreen changes, pruned windows, chord set changes
(`applied RegisteredChords`), and the end of its River connection
(`wayland event loop ended`).

## Limits

- One output for layout. `sola-river` sets a mode on every output, but the
  shell lays windows out against one output size, and fullscreen,
  screenshots, synthesized pointer input, and the layer-shell default use
  the first output.
- No keyboard layout, repeat rate, or pointer settings beyond NumLock on
  and natural scrolling.
- Layer-shell exclusive zones are ignored.
- No server-side title bars. A client that draws no decorations has none.
- Only floating windows move or resize, and only by their own title bar
  or edges. There is no keyboard move and no modifier-drag.
- Zones are fixed rectangles; there is no tiling.
- Window ids change when `sola-river` restarts.
- Only the first seat is used.
- `input.key` uses a US keymap regardless of the keyboard's layout.
