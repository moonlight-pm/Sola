# Remote Desktop: several machines, a Connections list, a Linux host

**Status:** built on `remote-desktop-connections`; see capabilities `kvm`.

**Asked by Joshua (2026-11-17):** "I'd like to enhance our kvm to allow me to
have multiple connections .. we need a 'connection list' as well. Then i'd
like to set up remote desktop to view koeurax" (Kora's PC: Oath, an RTX 4060,
one screen).

Follows [Remote Desktop](2026-11-09-sola-kvm-view-design.md), [measured, a
pointer drawn here, a stream that keeps up](2026-11-11-remote-desktop-latency-design.md)
and [a screen of its own](2026-11-11-remote-desktop-screen-design.md).

## Viewer: one app, a window per machine

- Remote Desktop is one process (`instance.rs`, as Calendar and Paint are
  one): a later launch hands it the machines it names and exits. An
  `iced::daemon`; each machine's window is a tab (`machine/<host>`), its
  title the host.
- **Connections** is its own window, laid out as the kit's navigator: the
  paired machines (`~/.config/sola-kvm/view/*.pub`), "Open" beside those
  with a window up, and a field that finds one or names a new one. Enter or
  a click opens the machine's window, or brings it up, and the list goes. A
  name that is not paired offers **Pair with <name>**: it runs `sola-kvm
  pair <name> --view` (ssh, as from a terminal), and on success opens the
  machine. Its error, in its own words, sits at the top of the list.
- Remote Desktop from the launcher opens the list (before: the one paired
  machine, or words saying there were several). `sola-kvm view <host>`
  opens that machine.
- Menus follow the focused window: a machine's has **View** (Mirror, Extend,
  Statistics, as before, per machine); the list's has **Connection**
  (Connect, Forget <machine>). **Remote Desktop → Connections** brings the
  list up from anywhere.
- Forget removes this machine's key for the other. The other keeps this
  one's until it is paired again; nothing gets in without both.
- Closing a machine's window ends its connection (`net::Running` shuts both
  streams; the video thread stops reconnecting). The last window closing
  quits.
- Mirror or Extend is kept per machine (`mirror.json`, a map by host). A
  file from before (one value for all) reads as Extend, the default.

## Pairing to view a Linux machine

- The host keeps its viewers apart from its input peer:
  `~/.config/sola-kvm/viewers/<name>.pub`, any number. `sola-kvm pair
  <host> --view` runs `sola-kvm pair --accept --viewer <this host>` there.
- A Mac keeps one key for input and viewing, as before: the pairing asks
  `uname -s` over ssh and runs the agent's app
  (`/Applications/SolaKvmMac.app/…/sola-kvm-mac pair --accept`) on Darwin,
  unless `--remote-bin` names one. ssh failing there ends the pairing with
  ssh's own words.
- `sola-kvm pair <host> --view --forget` forgets a machine this one views.

## Host on Linux

- `sola-kvm server` (Oath's `svc:sola-kvm` on every desk) serves the screen
  once a viewer is paired: TCP on the KVM port plus one, a viewer's key read
  as it connects (`Conn::accept_from`), so a pairing needs no restart.
- **Unpaired, the server waits** (`wait.rs`, inotify on its folder) instead
  of exiting: a machine that is only viewed never has an input peer, and
  before this it failed and was restarted until Oath gave up. Input sharing
  starts by itself once a peer is paired. `listen` is unchanged.
- One viewer at a time; a new one takes over, as on the Mac.
- **Picture:** wlr-screencopy (version 3) of the first output into shared
  memory, the pointer drawn in (`overlay_cursor`; wlroots draws it in
  software while a client asks, so its moves are damage). The first frame
  is copied at once, then each copy waits for damage: a still screen sends
  nothing. At most 60 frames a second, whatever the screen's refresh. The
  gate and the bitrate are the Mac's (`host/flow.rs`, `host/rate.rs`, moved
  from the agent and included by it by path, as `link.rs` is).
- **Encoder:** NVENC HEVC from system-memory RGB input, low latency, one
  picture out per frame in, CBR 2–50 Mbps following the gate, an IDR every
  10 s and when the viewer asks, parameter sets on every IDR
  (`host/encode.rs`, `host/nvenc.rs`: `libnvidia-encode.so.1` and
  `libcuda.so.1` loaded at run time). NVIDIA only, as the viewer is.
- The screen is always the host's own: `Down::CanExtend(false)` (additive,
  tag 8; until it comes a host can) greys out View → Extend; `Up::Size` and
  `Up::Mirror` are ignored. A screen that changes size makes a new encoder
  and a new `Display`.
- **Input:** the KVM's Wayland injector (`inject.rs`: virtual pointer and
  keyboard), absolute in the output's pixels, which the `Display` names as
  points too. Keys held when the viewer goes are released.
- **Clipboard:** text, through `clip` (as `listen` does).
- Errors the viewer cannot wait out (no screencopy, no encoder, a format the
  encoder does not take) are `Refused` with words; a desk not up yet ends
  the stream and the viewer comes back.

## Not in this slice

The main output only; no audio; a copied picture stays where it was;
nothing on koeurax's own screen says it is being viewed; the screen read
back through shared memory (a dmabuf straight into NVENC would spare the
copy); a Linux host's scroll direction and colours seen only on the private
desk until a real one.
