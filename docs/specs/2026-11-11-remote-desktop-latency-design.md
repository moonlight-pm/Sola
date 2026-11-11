# Remote Desktop: measured, a pointer drawn here, a stream that keeps up

**Status:** decided 2026-11-11; all three steps built on branch
`remote-desktop-latency` and measured against a scratch agent on ember (not
merged, the agent on ember not reinstalled).

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
- **`Cursor`** (step 2): the host's pointer image in the picture's pixels
  (at most 256 a side), premultiplied RGBA, with its hot spot.

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
- **Viewer:** the shader that draws the picture draws the image where the
  pointer was last sent, scaled as the picture is and cut to it; the local
  pointer stays hidden over it. It moves with the window's own redraw, which
  pointer motion already brings, without the trip to the Mac. Until a `Cursor`
  comes, nothing changes; after one, nothing is drawn until the first move
  since connecting, since where the Mac's pointer is is not known before.
- **The test card** sends this Mac's pointer too: the card has none of its
  own, and the viewer's half can be tried without moving anyone's.
- **Not in this step:** the compositor drawing it as a cursor surface (iced has
  no custom cursor; it would go in our `iced_winit` patch); a pointer the Mac
  hides (typing, full-screen video), which the viewer keeps drawing; the Mac's
  own mouse moving its pointer.

### Built (2026-11-11)

`apps/sola-kvm-mac/src/view/cursor.rs` reads it; `crates/sola-kvm-view/src/pointer.rs`
draws it, in the picture's render pass. ember (macOS 26.5.2) still answers
`currentSystemCursor`, though Apple marks it deprecated and says it will be
`nil` one day; the agent then keeps the pointer in the picture, as before.
Its arrow comes as 56×80 pixels with the hot spot at (10, 10). Over the test
card in a window on a private desk on novus, it is drawn black with its white
edge, within a pixel of where the pointer was sent. The stream is unchanged:
60 fps, 22 ms from capture to shown. The real screen leaving its pointer out
is unseen: the scratch agent has no Screen Recording grant.

## Step 3: a stream that keeps up

- **In flight** is frames from the encoder to the viewer's ack, not pictures
  written to the socket. Capture frames are skipped (before encoding, so no
  picture loses its reference) while more are in flight than one round trip
  and decoding need, `(rtt + 40 ms) / frame interval + 2` (3 to 40), or the
  oldest is older than rtt + 120 ms. `rtt` is the least round trip lately (0
  before the first), so a queue does not let more in.
- **The queue** is how long a picture waited for the one before it to come,
  past a round trip: not its own crossing, which on a slow link is long for
  a big picture and is not helped by a lower bitrate.
- **The bitrate** follows the queue, on each ack:
  - Congested (a picture waited over 30 ms): at most every 300 ms, 85% of the
    bitrate, and no more than 85% of what got through in the last half second
    when that was at least half the bitrate.
  - Clear (under 15 ms) with the content using at least half of it: a second
    after the last cut, up 6% and 100 kbps every 200 ms, to 90% of what got
    through at the last congestion (forgotten after 15 s) and at most the
    ceiling.
  - A key picture says what the way carries (hundreds of kilobytes cross at
    about its speed): when it crossed slower than the bitrate, the bitrate
    drops to 85% of that at once. Pictures sent while a key picture is out
    say nothing of the queue, and their round trips are not the least.
  - It starts at the ceiling, 50 Mbps: the LAN is the usual case, and its first
    picture should be sharp. It goes no lower than 2 Mbps.
  - VideoToolbox takes `AverageBitRate` during a session, as WebRTC's Apple
    encoder does. These intervals belong to a control loop measured from acks;
    nothing waits on them.
- **The encoder may go coarse** (`MaxAllowedFrameQP` 51, HEVC's coarsest):
  left to itself, VideoToolbox stops short of that and drops frames when the
  bitrate is tight. At 2 Mbps the test card came once a second, each picture
  a second of scrolling (about 250 kB), which only a lower bitrate followed.
  With it, 60 a second, coarser. The ceiling's pictures are unchanged.

### Built (2026-11-11)

`apps/sola-kvm-mac/src/view/flow.rs` (the gate, the queue, what got through)
and `rate.rs` (the bitrate). `sola-kvm view <host> --measure <n> --link <mbps>`
reads no faster than a link that fast would carry, so the host meets a slow
way without touching the network on either machine.

The test card, which wants about 11 Mbps, from a scratch agent on ember to
novus (seconds after the first two):

| Link | Before (step 2) | After |
|---|---|---|
| The LAN | 60 fps, 21 ms | 60 fps, 21 ms, 50 Mbps |
| 20 Mbps | | 60 fps, 30 ms (9 of them crossing), 19.6 Mbps |
| 8 Mbps | 2–10 fps, 2–3.6 s | 60 fps, 31–36 ms, about 6 Mbps |

At 8 Mbps the first key picture takes nearly a second to cross, so the first
two seconds reach half a second; a key picture every 10 s costs a second of
about 40 fps.

## Later (from the same look, not in this slice)

- A key picture only when the viewer asks: the wire is TCP and loses nothing,
  and one every 10 s costs a slow link a stutter.

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
  `sola-kvm view ember:<port> --measure`, and through a slower link with
  `--link <mbps>`.
- The agent on ember is installed once, when the slice is done, on Joshua's
  word.
