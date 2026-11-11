# Remote Desktop: a screen of its own, the size of the window

**Status:** decided 2026-11-11; to build on branch `remote-desktop-screen`.

**Decided by Joshua (2026-11-11):** "weren't you going to make it so the mac
changes its resolution to match what the content size is?" and "is it
possible to remote in to a second 'virtual screen' rather than main screen of
the laptop?". Of a virtual screen sized to the window, against switching the
laptop's own screen between its fixed modes: the virtual screen, made the main
one while connected, gone when the viewer goes ("1. make it the main 2. yes").
Builds on [Remote Desktop](2026-11-09-sola-kvm-view-design.md) and
[its latency work](2026-11-11-remote-desktop-latency-design.md).

## What it does

- While a viewer is connected, the agent makes a virtual display the size of
  the viewer's picture, makes it the main display (the menu bar and new
  windows go there; the laptop's screen sits to its right), and captures and
  drives it instead of the laptop's screen.
- Resizing the window resizes it: no bars, one picture pixel to one window
  pixel.
- When the viewer goes, the display goes with it: macOS moves its windows back
  to the laptop's screen, which is main again.
- The laptop's screen keeps its own mode throughout; someone at ember sees the
  menu bar leave and come back, not their screen change.

## The size

- **Points** are the window's picture area in its logical pixels, so a Mac
  window is as big as a Sola window. **Pixels** are the points at scale 1 (a
  1x mode), or twice them when the window's scale is above 1 (HiDPI, untried:
  novus runs at 1).
- Even, at least 640×480, at most 7680×4320 pixels (the display's maximum),
  points shrunk to fit.
- At novus's usual window, about 3635×2102: fewer pixels than the laptop's
  3600×2338, so encoding takes no longer.

## Wire (additive: version 3 stays)

- **`Up::Size { width, height, scale_percent }`**, viewer to host, on the input
  stream: the picture area in logical pixels and the window's scale. The
  viewer opens the input stream first and sends it there before opening the
  video stream, so the first picture is usually already of the virtual
  screen; it sends it again whenever it changes.
- The host applies a size by stopping capture, making or resizing the display,
  and capturing again: a new `Display`, then a key picture, which the viewer's
  decoder takes as a new sequence. Sizes that come while one is applied
  coalesce: the newest wins. Pictures from the capture before are dropped.

## Without it

The laptop's main screen, as before, when the private class is missing or
will not make a display (an update could take it away), or when no size
comes (an older viewer). The test card follows the size too, drawn at it at
1x, so the viewer's half and the resize path are tried without touching
ember's screens.

## Measured with a probe on ember (macOS 26.5.2, 2026-11-11)

`CGVirtualDisplay` (private, as Sidecar, DeskPad and BetterDisplay use it):
made in 5–25 ms; online 0.15–0.4 s after its first settings, which macOS
announces to no one in this process, so the agent looks; resized in about
60 ms; made main by putting it at (0, 0) with `kCGConfigureForAppOnly`, which
macOS undoes if the agent exits. Released, it is gone for every other process
within a second, but this process's own display list keeps it: the agent
keeps the ids it made. Whether ScreenCaptureKit lists it at once is unseen
(the probe had no Screen Recording grant).

## Not in this

- Moving the laptop's open windows onto the virtual screen when connecting:
  they stay on the laptop's screen, and new ones open on the main.
- Keeping the screen between sessions.
- A closed lid: whether a virtual display keeps it awake is untried.

## Test

- Unit: the wire (`Size` round trip), the size rule (even, bounds, HiDPI).
- End to end: the test card resizing with a window on a private desk, from a
  scratch agent on ember on another port.
- The real virtual screen needs a Screen Recording grant, so the installed
  agent (on Joshua's word): listed by ScreenCaptureKit, main, input landing,
  gone when the window closes.
