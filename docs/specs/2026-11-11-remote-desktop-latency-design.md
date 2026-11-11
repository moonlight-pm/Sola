# Remote Desktop: measured, a pointer drawn here, a stream that keeps up

**Status:** decided 2026-11-11; step 1 built and measured on branch
`remote-desktop-latency` (not merged, the agent on ember not reinstalled);
steps 2 and 3 to build.

**Decided by Joshua (2026-11-11):** after a look at voxl (Tembo's remote
desktop, [tembo/voxl](https://github.com/tembo/voxl)), take three of its ideas,
in this order: measure, then draw the Mac's pointer here, then acks and a
bitrate that adapts ("yes, write the spec and start in a worktree"). Builds on
[Remote Desktop](2026-11-09-sola-kvm-view-design.md).

## Why

- **The pointer is in the picture.** Each move goes to the Mac, is captured,
  encoded, sent back and decoded before it shows here.
- **The host does not know what arrived.** It lets the next frame through once
  the last one is written to its socket, so on a link slower than the stream
  pictures wait in the Mac's socket buffer and the delay grows instead of
  frames being skipped. The bitrate is fixed at 50 Mbps.
- **None of it is measured.** The first slice measured the test card's decode
  time and frame rate, not the delay from capture to the window, and not the
  frame rate under sustained motion.

## Wire version 3

Viewer and agent move together once more. From 3 on, a frame whose tag this
side does not know is skipped, so a later addition needs no new version; a
changed message still does.

- **`Picture`** gains `seq` (from 1, per video stream), `age_us` (capture to the
  start of sending, on the host's clock: encoding and the wait to send),
  `net_us` (the host's estimate of the trip down, from acks; 0 before the
  first), `kbps` (the encoder's bitrate) and `skipped` (capture frames skipped
  since the last picture).
- **`Ack { seq, held_us }`**, viewer to host on the input stream, once a picture
  is decoded: `held_us` is from its last byte read to the ack.
- **`Cursor`** (step 2): the host's pointer image, straight RGBA, with its
  hotspot.

No clock is shared. The round trip is from starting to send a picture to its
ack, less `held_us`, all on the host's clock. The acks are small, so the queue
is on the way down: the trip down is the round trip less half the least round
trip of the last 10 s.

## Step 1: measure

- **Per picture, here:** the host's part (`age_us`), the network (`net_us`),
  decoding (last byte read to decoded) and showing (decoded to on the GPU).
- **Each second of pictures** rolls up: pictures decoded and shown per second,
  megabits per second, the delay from capture to shown (median and worst),
  frames the host skipped, the bitrate.
- **Where it shows:** Show Statistics in Remote Desktop's menu puts it in a
  corner of the window. It changes only when a picture comes, so a still screen
  still draws nothing. `sola-kvm view <host> --measure <pictures>` decodes
  without a window, prints a line per second of pictures and a summary, and
  exits.
- **The test card** gets a band of fine detail that scrolls, so it presses the
  bitrate the way a scrolled page does. The four colors and the moving bar stay.

### Measured (2026-11-11)

The test card from a scratch agent on ember (M2 Max, 3024×1964, HEVC) to
novus, after the first second (connecting and the first key picture take
about 125 ms):

| | Host | Network | Decode | Show | Capture to decoded | To shown |
|---|---|---|---|---|---|---|
| No window, 60 fps | 15.6 | 0.8 | 2.9 | | 19.5 (worst 24) | |
| Card on IOSurfaces, as the screen's frames are | 17.6 | 0.8 | 2.9 | | 21.5 (worst 25) | |
| A window on a private desk | 17.6 | 0.8 | 3.0 | 0.8 | | 22.3 (worst 26) |

Milliseconds, medians per second. 60 pictures a second, all shown, none
skipped; the scrolling band takes about 11 Mbps of the 50.

- **Encoding is most of it.** At 30 fps the host's part stays at 17 ms, so it
  is the encoder's time for a frame, not a wait for the next one. The HEVC
  encoder refuses `MaxFrameDelayCount` and `PrioritizeEncodingSpeedOverQuality`
  (-12900, not supported), and `RealTime` off changes nothing.
- The screen's own path adds ScreenCaptureKit's time before its callback,
  which this does not see: capture is stamped when the frame reaches the
  agent.

## Step 2: the pointer drawn here

- **Host:** while a viewer is connected it reads the system cursor's image and
  hotspot (`NSCursor.currentSystemCursor`) 30 times a second and sends `Cursor`
  when it changes (images up to 256 px). If the cursor can be read when capture
  starts, capture leaves the pointer out of the picture; if not, the pointer
  stays in the picture and nothing is sent. The 30 Hz read is the one timer:
  macOS says nothing when another app changes the cursor, and it orders
  nothing.
- **Viewer:** the shader that draws the picture draws the image at the local
  pointer, scaled as the picture is; the local pointer stays hidden over it. It
  moves with the window's own redraw, which pointer motion already brings,
  without the trip to the Mac. Until a `Cursor` comes, nothing changes.
- **Not in this step:** the compositor drawing it as a cursor surface (iced has
  no custom cursor; it would go in our `iced_winit` patch); a pointer the Mac
  hides (typing, full-screen video), which the viewer keeps drawing; the Mac's
  own mouse moving its pointer.

## Step 3: a stream that keeps up

- **In flight** is pictures sent and not acked, not pictures written to the
  socket. Capture frames are skipped (before encoding, so no picture loses its
  reference) while more are in flight than one round trip and decoding need,
  `(rtt + 40 ms) / frame interval + 2` (3 to 40), or the oldest is older than
  rtt + 120 ms.
- **The bitrate** follows the queue, on each ack:
  - Congested (queue over 30 ms, or the oldest unacked older than rtt + 80 ms):
    at most every 300 ms, 85% of the bitrate, and no more than 85% of what got
    through in the last half second when that was at least half the bitrate.
  - Clear (queue under 15 ms) with the content using at least half of it: a
    second after the last cut, up 6% and 100 kbps every 200 ms, to 90% of the
    rate at the last congestion (forgotten after 15 s) and at most the ceiling.
  - It starts at the ceiling, 50 Mbps: the LAN is the usual case, and its first
    picture should be sharp.
  - VideoToolbox takes `AverageBitRate` during a session, as WebRTC's Apple
    encoder does. These intervals belong to a control loop measured from acks;
    nothing waits on them.

## Later (from the same look, not in this slice)

- The encoder: H.264's low-latency mode (VideoToolbox's, which NVDEC decodes
  too), measured against HEVC's 16–18 ms a frame.
- Sharpen a still screen: send the last frame again at higher quality once
  ScreenCaptureKit says the screen is idle.
- Pixel scrolling: the scroll in pixels, posted on the Mac as continuous pixel
  scroll events.
- The Mac's resolution following the window: Joshua's call, since someone at
  ember sees it change.
- Sound from the Mac (ScreenCaptureKit captures it).
- Recording the stream to MP4 without encoding it again; desk smoke tests that
  wait on damage instead of sleeping.

## Test

- Unit: the wire (new fields, unknown tags skipped, short frames are errors),
  the roll-up, the flow gate and the rate controller.
- End to end, before and after each step: a scratch agent on ember with the test
  card on another port (the installed agent stays), measured from novus with
  `sola-kvm view ember:<port> --measure`.
- The agent on ember is installed once, when the slice is done, on Joshua's
  word.
