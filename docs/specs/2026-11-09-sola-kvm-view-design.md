# Remote Desktop (`sola-kvm view`)

**Status:** first slice on master. The agent is installed on ember and the
real screen shows in a window on novus, driven from novus's keyboard and
pointer (Joshua, 2026-11-09: "everything works great").

**Decided by Joshua (2026-11-09):** remote view is a mode of `sola-kvm`, not a
new product ("this would just be another mode for kvm"); our own agent on the
Mac rather than a VNC client; while the window has focus Super goes to the Mac
as Cmd. The app is called **Remote Desktop**; the command stays `sola-kvm view`.

## Shape

`sola-kvm view <host>` shows the host's screen in a window here, and this
keyboard and pointer drive it. Input sharing (`sola-kvm server`) is untouched:
it keeps its UDP path, its one peer and its own process.

| Part | Where | What it does |
|------|-------|--------------|
| Viewer | `crates/sola-kvm-view` | The window: connects, decodes, draws, sends input. Its own process, so the daemon stays free of the GPU. `sola-kvm view` runs it. |
| Host, Mac | `apps/sola-kvm-mac/src/view/` | ScreenCaptureKit on the main display, VideoToolbox hardware HEVC, input through the KVM's `CgInjector`. Nothing runs until a viewer connects. |
| Wire | `crates/sola-kvm/src/view_wire.rs` | Shared by both sides, as `link.rs` is. |

## Wire

Two TCP connections to the KVM port plus one (`4243`), each its own Noise IK
session (`link::Conn`). The first frame says which it is.

- **Video**, host to viewer: `Display`, then `Picture`s (HEVC, Annex B, key
  pictures carry their parameter sets), or `Refused` with words for the window.
- **Input**, viewer to host: KVM1 packets, the same bytes as the UDP path, and
  `Keyframe` when the decoder loses its place.

One stream per direction: input never waits behind the picture, and neither
side shares a Noise state between threads.

## Pairing

`sola-kvm pair <host> --view` keeps the host's key at
`~/.config/sola-kvm/view/<host>.pub`, beside `peer.pub`. Input sharing has one
peer; a machine may view others (novus shares input with canto and views
ember).

## Picture

- One frame in, one out: no reordering, no display delay.
- The host skips capture frames when three pictures are already out, so a slow
  network drops frames before they are encoded and no picture loses its
  reference.
- A still screen sends nothing and the window does not draw (see
  `PERFORMANCE.md`'s law): each picture is one message and one present.
- NVDEC decodes; NV12 goes to two wgpu textures on an iced shader widget,
  letterboxed in the window.
- The host's pointer is in the picture, and the local one hides over it.

## Keyboard and pointer

Physical keys go as evdev codes. A shortcuts inhibitor on the window's surface
keeps the desk's bindings out of the way while the window has the keyboard.
**Ctrl+Alt+Esc** hands keyboard and pointer back, as it does for input sharing;
a click on the picture takes them again. Keys held when focus leaves are
released on the host.

## Not in this slice

NVIDIA decoders only; the main display only; one viewer at a time; no audio; no
clipboard in view mode; the window does not resize the host's screen; no
launcher entry.
