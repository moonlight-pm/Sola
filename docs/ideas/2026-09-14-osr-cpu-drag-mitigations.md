# CPU OSR HTML5 drag — parked mitigations

**Status:** idea (parked 2026-09-14). Do not implement from this file.
Promote into a freeze + plan + `CURRENT.md` **Now** only if drag is slow
or trails again, or we want GPU MiB/s down for battery.

**As-built (shipped, dogfood good):** Shortcut kanban ticket drag and
`crates/sola-browser/assets/kanban-dnd.html` feel fast with no trails
after damage continuity + unique `last_frame`. Remaining items below
were ranked and **not** tried.

**Product lock:** CEF CPU `on_paint` (`shared_texture_enabled=0`). Do
**not** enable `accelerated_osr` / dma-buf on this NVIDIA desk.

---

## What the page actually does

Shortcut **tickets** are `@atlaskit/pragmatic-drag-and-drop` native
HTML5 (`draggable=true`, `application/vnd.pdnd`, `setDragImage`).
**Columns** are `react-beautiful-dnd` (mouse lift, not HTML5).

Replica (do not shuffle live Shortcut stories):

- `crates/sola-browser/assets/kanban-dnd.html` — Native HTML5 vs Mouse lift
- `crates/sola-browser/assets/drag-stress.html` — dense dirty-rect storm

CEF OSR has no OS ghost. `RenderHandler::start_dragging` hands us
`DragData`; the host must composite the bitmap and echo
`DragTargetDragEnter` / `Over` / `Drop` / `Leave`.

---

## Pipeline (helper → chrome GPU)

```text
CEF on_paint (full BGRA + dirty_rects)
  → unique last_frame apply (or full blit if shared)
  → overlay: last_frame memcpy + blit ghost  (while HTML5 drag live)
  → helper FrameMailbox (latest-wins; absorb dropped dirty)
  → Unix socket engine.frame.sock (raw BGRA, not bincode)
  → chrome read_frame allocates a new Vec
  → router FrameMailbox (absorb)
  → iced pending (absorb on overwrite)
  → shader queue.write_texture (dirty rects; full only on size change)
```

Telemetry (250 ms windows, `sola_browser=info` in `/opt/sola/log/sola.log`):

- helper: `osr window` / `start_dragging` / `osr drag end`
- chrome: `osr chrome present`

---

## Already shipped (do not redo)

| Slice | Why it existed |
|-------|----------------|
| Partial dirty only onto a uniquely-owned complete last-frame | Fresh/zero dest + partial dirty left holes until hover |
| Overlay 3-slot `PixelRing` (no extra session `Arc`) | Session Arc pinned latest at 3 refs → ~19 MiB alloc/move |
| Skip overlay publish while helper mailbox still holds this tab | Overlay storm ~280–340 Hz, mailbox_drop ~half |
| Rebuild overlay from **clean** last_frame + blit ghost | Recycled ring slots still had an old ghost |
| Damage continuity: mailbox / overlay-skip `pending_dirty` / chrome pending absorb dropped dirty | Skip + latest-wins dropped `View` and intermediate ghost rects → trails |
| Unique last_frame **not** `PixelRing::publish`'d | Ring clone made `try_unwrap` fail every paint (`incomplete_dst` always) |
| GPU uploads overlay dirty (not `force_full` because `frame.drag`) | Full texture every present after the trail fix |
| `page_drag` `request_redraw` pump; cancel outside / Escape / leave | Mouse-up outside ignored; chrome rebuild at 60 Hz |
| `page_drag` only for the painted tab | Inspector/background frames were keeping the pump |

Log snapshot **after** overlay-ring + trail `force_full`, **before**
damage continuity (Native HTML5, ~6 s):

| Counter | ~value |
|---------|--------|
| helper moves | 1404 (~230 Hz) |
| overlays | 267 (~45 Hz) |
| mailbox_drop | 0 |
| overlay_unique / alloc | 265 / 2 |
| chrome presents | 182 (~30 Hz) |
| gpu_full | 182 / 182 |
| GPU | 2.86 GiB (~480 MiB/s) |
| pending_drop | 85 |
| CEF paints `incomplete_dst` | 144 / 144 |

85/182 pending_drop is about **2×** GPU work, not 50–100×. Overlay
still full-copied last_frame on the helper (~15.7 MiB × 267).

---

## Untried (ranked)

Promote only if the cost shows up again in `osr window` /
`osr chrome present`.

### 1. Ghost as a chrome shader quad

Upload the ghost bitmap **once**. Shader samples the page texture plus
a small overlay quad at the pointer. Helper sends ghost pose (x, y,
hotspot) instead of a full composited view.

Removes the per-move last_frame memcpy and most overlay socket
payloads. Harder: iced shader has one page texture today; need a
second bind or atlas, and a non-drag path that stays idle.

### 2. Overlay dest: restore previous ghost, don’t memcpy the view

Today every overlay `clear()` + `extend_from_slice(last_frame)` then
blits the new ghost. A unique overlay `Vec` can erase the **old** ghost
rect from last_frame and blit the new one — if:

- the socket writer **hands the `Arc` back** after `write_frame` so
  the helper can `try_unwrap`, and
- each ring slot records which last_frame revision and ghost rect it
  holds.

Without handback, chrome/mailbox still share the Arc and unique unwrap
fails (same class of bug as pinning latest in `PixelRing`).

### 3. Don’t allocate a second full frame in `read_frame`

`ipc::read_frame` does `vec![0u8; plen]` then `read_exact` (~15.7 MiB
per overlay; ~4.1 GiB allocated over the 6 s snapshot). Recycle a
chrome-side ring, or shared memory, or read into a recycled unique
buffer. Orthogonal to (1): even a pose-only overlay still pays this
on every **view** paint.

### 4. Drop 256-byte `write_texture` row padding

`cpu_import.rs` pads every dirty row to 256 bytes (`COPY_ALIGN`).
wgpu 27 `Queue::write_texture` is **not** the `copy_buffer_to_texture`
path that requires `COPY_BYTES_PER_ROW_ALIGNMENT`. **Verify against
the pinned wgpu version** before ripping this out; if true, skip
staging for unpadded full-width rects (the `x == 0 && w == src_w`
fast path already does).

### 5. Bounded credits on the helper→chrome pipe

Latest-wins + absorb is correct for damage. It is **not**
backpressure: `frame_stream` is unbounded after the socket. Refusing
to overwrite iced `pending` does not slow CEF.

Only if `osr chrome present` still shows a pile after (1)–(3): a
small credit window (helper blocks overlay publish until chrome ACKs
a present) plus a **deferred final publish** on drag end so the last
pose is not dropped. Do not throttle `DragTargetDragOver` — HTML5
hit-testing wants pointer rate.

---

## Rejected

| Idea | Why not |
|------|---------|
| 16 ms chrome timer / always-on vsync | GPU idle law ([`PERFORMANCE.md`](../../PERFORMANCE.md)); shader `request_redraw` hangover is the pump |
| `accelerated_osr` / dma-buf | NVIDIA CPU OSR lock; crate feature pulls wgpu/Vulkan importers we do not use |
| Treat pending-overwrite refuse as backpressure | Unbounded stream after the socket; CEF keeps painting |
| Cap `DragTargetDragOver` | Drops HTML5 drop-target fidelity |
| Use live Shortcut as the only test | Replica pages exist; do not shuffle real stories |

---

## How to re-measure

```bash
# replica
file://…/crates/sola-browser/assets/kanban-dnd.html
# live
Shortcut kanban ticket (Native HTML5), not column (rbd)

# logs
grep -E 'osr window|osr chrome present|start_dragging|osr drag end' /opt/sola/log/sola.log
```

Want: `incomplete_dst` low during drag, `gpu_partial` non-zero,
`mailbox_drop` ~0, `overlay_alloc` ~0, no trails, drag feels like the
page not the compositor.
