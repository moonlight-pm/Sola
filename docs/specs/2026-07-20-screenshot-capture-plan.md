# Screenshot capture — Implementation Plan

> **For agentic workers:** Implement task-by-task in a `.worktrees/` worktree.
> Steps use checkbox (`- [ ]`) syntax. **Never** `cargo make install` without
> express user permission for that install. Verify with `cargo make build`
> (or crate-scoped builds). User installs and smokes from a TTY.

**Goal:** Make full-output and focused-window screenshots work end-to-end —
`solactl screenshot` for automation/agents, Super+Shift+3/4 in sola-shell for
humans — by implementing `wlr-screencopy` capture in `sola-river`.

**Architecture (as-built now):** One capture backend in `sola-river`. CLI and
shell invoke `compositor.screenshot`. No grim binary.

**Tech stack:** Wayland client (`wayland-client` 0.31), vendored
`wlr-screencopy-unstable-v1`, `wl_shm` memfd buffers, `png` crate encode,
existing bus topics.

**Date:** 2026-07-20  
**Status:** **Superseded (request path)** — capture still lives in sola-river; request/reply is `compositor.screenshot` on sola-call (2026-08-13 call-plane freeze). `CaptureScreen` / `Screenshot` bus topics are gone.  
**Roadmap:** `docs/specs/2026-07-20-macos-look-and-feel-roadmap.md` (P0)  
**Prior art:** stub + plan comments in
`crates/sola-river/src/client/screenshot.rs`; older notes in
`docs/specs/2026-04-26-sola-debug-design.md` (topic names evolved to
`CaptureScreen` / `Screenshot` — use **current** bus types).

---

## Global constraints

1. **No install** unless the user explicitly authorizes that install.  
2. **Worktrees only** for code changes.  
3. **Do not** retune theme tokens, menubar chrome, or design-language greys in this plan.  
4. **Do not** depend on `grim` or `grim-rs`.  
5. V1 is **single-output** (first `wl_output`). Multi-output picker is deferred.  
6. Prefer small modules; keep async in-flight state on `AppData`.  
7. Log failures with `tracing` (`warn!` / `error!`); never silent-drop.  

---

## Already done (do not re-implement)

| Piece | Location |
|-------|----------|
| `CaptureScreenPayload`, `CaptureTarget`, `ScreenshotPayload` | `crates/sola-bus/src/topics.rs` |
| `Topic::CaptureScreen` / `Topic::Screenshot` | same |
| Bus dispatch into screenshot handler | `crates/sola-river/src/client/mod.rs` (`Topic::CaptureScreen` arm) |
| Stub handler | `crates/sola-river/src/client/screenshot.rs` |
| `solactl screenshot [-o] [--app] [--window] [-t]` | `crates/solactl/src/{main,screenshot}.rs` |
| Window geometry for region capture | `WindowRegistry::find_by_app_title`, `Entry::frame` |
| memfd pattern | `crates/sola-river/src/client/virtual_keyboard.rs` (`memfd_create`) |
| Protocol vendoring pattern | `crates/sola-river/src/protocol.rs` + `protocols/*.xml` |
| Shell toast helper | `menubar.push_toast` + `ToastExpire` |
| Shell chord registration | `shell_key_chords` + `keys::to_registered` |
| Digit keysyms for 3/4 | `keys.rs` already maps `KEY_3`/`KEY_4` |

---

## File map

| File | Change |
|------|--------|
| `crates/sola-river/protocols/wlr-screencopy-unstable-v1.xml` | **Create** — vendor protocol |
| `crates/sola-river/src/protocol.rs` | Add `wlr_screencopy_unstable_v1` module (mirror virtual-pointer) |
| `crates/sola-river/Cargo.toml` | Add `png` (and `memmap2` if not using rustix mmap) |
| `crates/sola-river/src/client/mod.rs` | Bind `zwlr_screencopy_manager_v1` + `wl_shm`; hold proxies; dispatch frame events |
| `crates/sola-river/src/client/screenshot.rs` | Replace stub with full capture state machine |
| `crates/solactl/src/screenshot.rs` | Fix stale “delegated to grim” module docs only |
| `crates/sola-shell/src/app.rs` | Register Super+Shift+3/4; handle chords; handle `Screenshot` topic for toast |
| `crates/sola-shell/src/app/bus.rs` | Dispatch chord → capture; `Topic::Screenshot` → toast |
| `crates/sola-shell/src/keys.rs` | Only if chord encode/decode needs a tweak (likely already OK) |

**Non-goals for this plan:** `docs/visual/` baselines (P1), theme dump CLI, blur/materials, multi-monitor picker.

---

## Capture flow (target behavior)

```
solactl / shell
    │  Topic::CaptureScreen { path?, target }
    ▼
sola-river screenshot::handle
    │  resolve path (default /tmp/sola/screenshots/<ms>.png)
    │  resolve region (full output or window frame)
    │  manager.capture_output[_region](…)
    ▼
Wayland events: Buffer → copy → Ready | Failed
    │  mmap SHM, convert to RGBA8, png::encode, write file
    ▼
Topic::Screenshot { result: Ok(path) | Err(msg) }
    │
    ├─► solactl: print path / error, exit 0/1/2
    └─► shell: toast path or error (P0b)
```

### Default path

If `path` is `None`:

```
/tmp/sola/screenshots/<unix-millis>.png
```

Create parent dirs (`create_dir_all`) before write.

### Window target

1. `registry.find_by_app_title(app_id, title)`  
2. Read `entry.frame` as `(x, y, w, h)`  
3. If missing frame → `Err("window has no frame yet")`  
4. `capture_output_region` with those coords on the chosen output  
5. Note (document in code + solactl help if needed): region is **screen content at that rect**, including overlaps — same as existing CLI comment.

### Shell hotkeys (P0b)

| Chord | Target |
|-------|--------|
| `Super+Shift+3` (`KeyCode::KEY_3.meta_shift()`) | `CaptureTarget::FullOutput` |
| `Super+Shift+4` (`KeyCode::KEY_4.meta_shift()`) | `CaptureTarget::Window` for **focused** app/window |

For Super+Shift+4: use shell’s focused `app_id` + focused window title if available; if no focus, toast error and do not emit.

Register both chords unconditionally in `shell_key_chords` (always available, not overlay-only).

Shell emits `CaptureScreen` with `path: None` (auto path). On `Topic::Screenshot` from `sola-river`, toast:

- success: `Screenshot saved: <path>` (shorten if needed)  
- error: `Screenshot failed: <msg>`  

Avoid double-toast storms: one toast per completed capture.

---

## Protocol / deps details

### Vendor XML

Source: wlroots / wayland-protocols-wlr
`wlr-screencopy-unstable-v1.xml` (standard file).

Place at `crates/sola-river/protocols/wlr-screencopy-unstable-v1.xml`.

### protocol.rs module (pattern)

Mirror `wlr_virtual_pointer_unstable_v1`:

```rust
pub mod wlr_screencopy_unstable_v1 {
    use wayland_client;
    use wayland_client::protocol::*;

    pub mod __interfaces {
        use wayland_client::protocol::__interfaces::*;
        wayland_scanner::generate_interfaces!(
            "protocols/wlr-screencopy-unstable-v1.xml"
        );
    }
    use self::__interfaces::*;

    wayland_scanner::generate_client_code!(
        "protocols/wlr-screencopy-unstable-v1.xml"
    );
}
```

### Cargo.toml

Add at least:

```toml
png = "0.17"   # or latest compatible; sola-browser-wpe uses 0.18 — prefer workspace-consistent if easy
```

For mmap: prefer `rustix` mm if already available; otherwise:

```toml
memmap2 = "0.9"
```

Expand `rustix` features if using rustix for mmap (`mm` feature).

### Bind globals (client/mod.rs registry handler)

On `wl_registry::Event::Global`:

- `"zwlr_screencopy_manager_v1"` → bind manager, store on `AppData`  
- `"wl_shm"` → bind `WlShm`, store on `AppData`  

Also track at least one `wl_output` if not already stored (needed for `capture_output`). If outputs are only known via wlr-output-management today, bind `wl_output` globals as they appear (version min reasonable, e.g. 3–4) into `Vec<WlOutput>` or first-output `Option`.

If screencopy manager or shm missing at request time → emit
`Screenshot { Err("… protocol not available") }`.

### SHM buffer allocation (critical)

Use the **event’s** `format`, `width`, `height`, `stride` — not `width * bpp`.

1. `memfd_create("sola-screencopy", CLOEXEC)`  
2. `ftruncate(fd, stride * height)`  
3. `wl_shm.create_pool` → `create_buffer(0, width, height, stride, format)`  
4. `frame.copy(buffer)`  
5. On `Ready`: mmap, convert row-by-row to RGBA8  

### Format conversion

Support at least (match what River actually advertises; log unknown formats):

| Format | Conversion |
|--------|------------|
| `Xbgr8888` / `Abgr8888` | Often pass-through layout; set α=255 if X |
| `Xrgb8888` / `Argb8888` | Swap R↔B as needed for PNG RGBA |
| `Bgr888` (3 bpp) | Pack 3→4, α=255; **use event stride** |

Unknown format → `Err` with format debug string; free resources.

### PNG write

RGBA8 buffer → `png` encoder → write to resolved path → `Ok(path)`.

### Concurrency

Track in-flight captures on `AppData` (e.g. `HashMap` keyed by frame proxy id or generation). Concurrent requests should not corrupt each other; serializing to one in-flight is acceptable V1 if documented (`Err("screenshot already in progress")` or queue). Prefer simple **single in-flight** + clear error over a complex queue.

### Dispatch

Implement `wayland_client::Dispatch` for screencopy frame (and manager if needed) on `AppData`, same style as virtual pointer / keyboard modules.

---

## Tasks

### Task 1: Vendor protocol + deps + bind globals

**Files:**
- Create: `crates/sola-river/protocols/wlr-screencopy-unstable-v1.xml`
- Modify: `crates/sola-river/src/protocol.rs`
- Modify: `crates/sola-river/Cargo.toml`
- Modify: `crates/sola-river/src/client/mod.rs` (`AppData` fields + registry bind + init)

- [x] **Step 1:** Vendor the XML (copy from wayland-protocols-wlr upstream; do not invent interface names).  
- [x] **Step 2:** Add `protocol.rs` module as above.  
- [x] **Step 3:** Add `png` (+ mmap) deps.  
- [x] **Step 4:** On `AppData`, add fields for screencopy manager, `wl_shm`, outputs list, and screenshot flight state (can be empty stub struct for now).  
- [x] **Step 5:** Bind globals in the existing registry `Global` match. Log `info!` on bind.  
- [x] **Step 6:** `cargo make build sola-river` (or workspace build). Fix compile errors.  
- [x] **Step 7:** Commit: `feat(sola-river): vendor wlr-screencopy and bind globals`

---

### Task 2: Implement full-output capture body

**Files:**
- Modify: `crates/sola-river/src/client/screenshot.rs` (replace stub)
- Modify: `crates/sola-river/src/client/mod.rs` (wire Dispatch if not in screenshot module)

**Behavior:**
- `handle` for `CaptureTarget::FullOutput` (window path can still Err temporarily if Task 3 separate)
- Resolve path, create dirs, start screencopy, complete async, emit `Screenshot`

- [x] **Step 1:** Design flight struct: path, target, buffer fd/mmap state, dimensions, format, stride.  
- [x] **Step 2:** Implement SHM alloc + copy + Ready/Failed handlers.  
- [x] **Step 3:** Implement pixel→RGBA + PNG encode helpers (private fns in same module).  
- [x] **Step 4:** On any error path, emit `Topic::Screenshot(Err(...))` and clean up proxies/fds.  
- [x] **Step 5:** Build.  
- [x] **Step 6:** Commit: `feat(sola-river): implement full-output screencopy capture`

**Unit tests:** Pure helpers only if easy (e.g. format conversion on a tiny synthetic buffer). Do not mock full Wayland in-process for V1.

---

### Task 3: Window-region capture

**Files:**
- Modify: `crates/sola-river/src/client/screenshot.rs`
- Possibly: `registry.rs` (read-only use of `find_by_app_title`)

- [x] **Step 1:** For `CaptureTarget::Window { app_id, title }`, resolve entry + frame.  
- [x] **Step 2:** Call `capture_output_region` with x/y/w/h (protocol arg types are i32; cast carefully).  
- [x] **Step 3:** Same Ready path as full output.  
- [x] **Step 4:** Build + commit: `feat(sola-river): window-region screenshot capture`

---

### Task 4: solactl docs polish

**Files:**
- Modify: `crates/solactl/src/screenshot.rs` module docs (remove “delegated to grim”)
- Optional: `crates/solactl/src/main.rs` help text if it still implies grim

- [x] **Step 1:** Document: capture performed by sola-river via wlr-screencopy; path printed on success.  
- [x] **Step 2:** Build solactl. Commit: `docs(solactl): screenshot no longer claims grim`

---

### Task 5: Shell Super+Shift+3/4 + toast (P0b)

**Files:**
- Modify: `crates/sola-shell/src/app.rs` (`shell_key_chords`, chord handling)
- Modify: `crates/sola-shell/src/app/bus.rs` (`Topic::Screenshot`, capture emit)
- Possibly: Msg variants if needed (`Msg` already has bus path)

- [x] **Step 1:** Push `KeyCode::KEY_3.meta_shift()` and `KEY_4.meta_shift()` into `shell_key_chords`.  
- [x] **Step 2:** On matching chord in update/bus handler: emit `CaptureScreen` (full vs focused window).  
- [x] **Step 3:** On `Topic::Screenshot` from bus: `push_toast` success/error; schedule `ToastExpire` like other toast sites.  
- [x] **Step 4:** Ensure shell bus subscription includes `Screenshot` if topics are filtered (subscribe to all or add kind).  
- [x] **Step 5:** Build sola-shell. Commit: `feat(sola-shell): Super+Shift+3/4 screenshot chords`

---

### Task 6: Agent self-check (build-only)

- [x] **Step 1:** `cargo make build` for touched crates (at least `sola-river`, `solactl`, `sola-shell`).  
- [x] **Step 2:** Grep for leftover stub string `"screenshot not yet implemented"` — must be gone from success path.  
- [x] **Step 3:** Update plan checkboxes / note residual risks in the PR/commit message.  
- [x] **Step 4:** Do **not** install. Hand off to user for smoke.

---

## User smoke checklist (after install)

User authorizes install, then from a live Sola session:

```bash
# Full output
solactl screenshot -o /tmp/sola/screenshots/smoke-full.png
# expect: path printed, exit 0, file is non-empty PNG

# Optional: window
solactl screenshot --app sola-shell --window menubar -o /tmp/sola/screenshots/smoke-menubar.png

# Hotkeys
# Super+Shift+3 → toast with path
# Super+Shift+4 → focused window region
```

Agent follow-up (same or next session): open PNGs with `read_file`, confirm menubar/desktop visible, then proceed to roadmap P1/P2.

---

## Risks

| Risk | Mitigation |
|------|------------|
| River advertises 3-bpp `Bgr888` | Use event stride; convert 3→4 bpp (this is why grim-rs was rejected) |
| No `wl_output` bound yet | Bind `wl_output` in registry; fail clearly if empty |
| Overlay transparency | Capture is compositor-side; should include composed pixels |
| Shell chord stolen by apps | River grabs registered chords globally for shell list — same as other shell chords |
| Super+Shift+4 with no focus | Toast error; no bus emit |
| Concurrent captures | Single-flight + error |
| PNG of huge 4K buffer | Acceptable V1 latency; log timing at debug |

---

## Out of scope (explicit)

- Theme / greys / menubar restyle  
- `docs/visual/` baseline commit (P1)  
- Multi-monitor output selection  
- Interactive region drag selector (macOS crosshair) — V1 is full or focused-window only  
- Clipboard copy of screenshot  
- Sound / flash effect  

---

## Success criteria

1. `solactl screenshot -o <path>` exits 0 and writes a valid PNG of the live desktop.  
2. `solactl screenshot --app …` captures a window region when frame is known.  
3. Super+Shift+3/4 work in shell with toast feedback.  
4. No grim dependency.  
5. Build passes; user-confirmed smoke before starting token/visual polish (roadmap P2+).
