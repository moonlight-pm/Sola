# sola-kvm — Design

**Date:** 2026-07-27  
**Status:** Implementing — Phase A done; Phase C server path in; Linux `listen` injects via virtual pointer/keyboard  
**Dogfood:** novus server; Linux client (`sola-kvm listen`) or ember macOS agent  
**Gaps:** D2 permanent input ACL on the server; Mac clip is text-only; Super+Tab confirm over Linux listen unsmoked after key-before-modifiers

## 1. Goal

Replace the lan-mouse-based software KVM with a **Sola-native**, minimal path:

- Physical mouse/keyboard on **novus** (Sola / River)
- Control **ember** (macOS) when the pointer is in a **virtual Mac rect**
- Layout is first-class (bottom-align, side placement, motion scale)
- No TLS, discovery, multi-client mesh, or generic KVM product surface
- **UDP spray** of events on a trusted LAN; connection stays warm

## 2. Topology

| Role | Host | Responsibility |
|------|------|----------------|
| **Server** (input owner) | **novus** | Owns HID; owns virtual layout; exclusive grab while “over” the peer; sends packets |
| **Client** (receiver) | Linux peer (`sola-kvm listen`) or **ember** | Listens UDP; Linux injects via `zwlr_virtual_pointer_v1` + `zwp_virtual_keyboard_v1`; Mac injects via Accessibility / CGEvent |

This matches desk intent: hardware stays on novus; Mac is remote display of input only.

```
  mouse + keyboard
         │
         ▼
   ┌──────────┐   UDP events    ┌──────────┐
   │  novus   │ ──────────────► │  ember   │
   │  server  │                 │  client  │
   └──────────┘                 └──────────┘
   virtual Mac rect              inject + warp
```

## 3. Virtual space (layout-first)

Do **not** require a real second DRM/Wayland output.

Maintain a **software rectangle** in the same logical coordinate system as the primary Sola output:

**Example (current hardware, bottoms aligned, Mac to the right):**

| | Size | Placement |
|--|------|-----------|
| novus (real) | 5120×2160 | origin (0, 0) |
| ember (virtual) | 2560×2880 | origin **(5120, 2160 − 2880) = (5120, −720)** |

```
  y=-720  ┌─────────────────┐
          │  virtual ember  │  2560×2880
  y=0     │                 │
          │                 ├──────────────────────────────
  y=2160  └─────────────────┘         real novus 5120×2160
          x=0             x=5120                         x=7680
                    bottoms meet at y=2160
```

### 3.1 Pointer modes

1. **Local** — pointer in the real output rect → normal River/Sola input.  
2. **Remote** — pointer would leave the real output into the virtual Mac rect (edge hit on the shared side) → enter remote mode:
   - Exclusive pointer + keyboard grab
   - Suppress Sola shell chords (Meta+Space, Meta+Tab, …) so keys reach the Mac as Cmd
   - Track a **virtual cursor** in Mac-local coordinates `(mx, my)` with `0 ≤ mx < W`, `0 ≤ my < H`
   - Send absolute (or abs+rel) motion to ember  
3. **Return** — virtual cursor exits the Mac rect toward novus (e.g. `mx < 0` for left edge of virtual) → release grab; warp/restore pointer on the real edge.

### 3.2 Config knobs (v1)

```toml
# e.g. ~/.config/sola-kvm/config.toml  (or sola-managed path)

[peer]
host = "10.0.0.21"
port = 4242

[layout]
# relative to primary sola output
side = "right"           # right | left | top | bottom
align = "bottom"         # bottom | top | center  (along the shared edge)
mac_width = 2560
mac_height = 2880
# optional manual override of origin if align is not enough:
# offset_x = 5120
# offset_y = -720

[motion]
scale = 1.0              # multiply dx/dy before send (match “feel”)

[bind]
release = []             # optional emergency release chord; edge-return is primary
```

## 4. Wire format (minimal UDP)

Trusted LAN only. **No TLS, no fingerprints, no handshake.**

### 4.1 Transport

- UDP unicast novus → ember (and optional tiny ACK ember → novus for enter/leave only if needed)
- Packets are self-contained; lossy motion is acceptable; keys should be reliable enough on LAN (optional sequence + simple retransmit later)
- Keep a **background “ping” or idle traffic** optional; prefer **first packet anytime** without setup

### 4.2 Packet (v1 sketch)

Fixed little-endian header + payload. Version byte for evolution.

| Field | Type | Notes |
|-------|------|--------|
| `magic` | u32 | `0x4b564d31` (`KVM1`) |
| `version` | u8 | `1` |
| `type` | u8 | see below |
| `seq` | u32 | monotonic on server |
| payload | … | type-specific |

**Types:**

| type | name | payload |
|------|------|---------|
| 1 | `Enter` | `edge: u8`, `x: i32`, `y: i32` (Mac-local abs on enter) |
| 2 | `Leave` | empty |
| 3 | `Motion` | `x: i32`, `y: i32` **or** `dx: f32`, `dy: f32` — prefer **abs** in Mac space for layout fidelity |
| 4 | `Button` | `button: u8`, `pressed: u8` |
| 5 | `Key` | `keycode: u32` (Linux evdev), `pressed: u8` (`0` release, `1` press, `2` kernel auto-repeat) |
| 6 | `Scroll` | `dx: f32`, `dy: f32` |
| 7 | `Modifiers` | `mask: u32` (optional explicit mod state) |

**Recommendation:** use **absolute Mac coordinates** for pointer after enter so bottom-align and speed scaling are server-side (`scale` applied when integrating relative HID into the virtual cursor).

## 5. Novus architecture

### 5.1 Process shape

Prefer a small binary **`sola-kvm`** (or `sola-kvmd`) managed by `sola` or a user unit:

- Connects as needed to Wayland / sola-river / bus  
- Owns layout state + UDP send  
- Does **not** reimplement the compositor  

Integration points:

| Component | Role |
|-----------|------|
| **sola-river** | Optional: expose pointer pos, exclusive-focus chord suppress (already started on libei-portal), maybe a thin “kvm active” sticky |
| **River** | Real output geometry; clamp behavior on real edges |
| **sola-kvm** | Edge logic, virtual cursor, grab, UDP |

### 5.2 Capture strategy (v1)

**Chosen for speed of implementation:** edge hit on real output → exclusive grab (layer-shell barrier *or* seat-level grab once past edge), then software virtual cursor.

Alternatives (later):

- True virtual output / headless head of Mac size (heavier River work)  
- Raw `/dev/input` + `EVIOCGRAB` while remote (snappier; more device mess)

v1 does **not** need libei or portals.

### 5.3 Chord / Super handling

While remote:

- Disable River `river_xkb_binding_v1` shell chords (Meta+Space, Meta+Tab, …) so keys are delivered to the capture path and forwarded as Cmd on Mac  
- Re-enable on leave  

(Pattern already prototyped: layer exclusive focus → suppress chords.)

### 5.4 Lifecycle

- Start after Sola/River Wayland socket exists (same wait pattern as lan-mouse wrapper)  
- Autostart via user systemd or sola MANAGED once stable  
- Config + peer IP fixed; no discovery

## 6. Ember architecture

### 6.1 Process

Tiny native agent (Swift or Rust + CGEvent):

- Bind UDP port  
- On `Enter`: warp cursor to `(x, y)`  
- On `Motion`: set/warp or delta as designed  
- On `Button` / `Key` / `Scroll`: CGEvent inject  
- On `Leave`: no-op or local restore  

### 6.2 Permissions

- Accessibility (and Post Event if required) — grant once in System Settings  
- Run in **GUI login session** (LaunchAgent `open` app or gui domain), not SSH  

### 6.3 Keycodes

Map Linux evdev → Mac CGKeyCode table (subset for v1: letters, mods, arrows, Space, Escape, Tab, Enter, F-keys as needed).

## 7. Non-goals (v1)

- TLS, pairing UI, certificate fingerprints  
- Multi-client / multi-Mac / mesh  
- Clipboard, file transfer — **clipboard text is a follow-on**: see
  `docs/specs/2026-07-30-sola-kvm-clipboard-design.md` (TCP side channel,
  worker thread, sync on Enter/Leave only, hash cache)
- Bidirectional control (Mac controlling novus)  
- GNOME/KDE portals, libei, Deskflow/Input Leap protocol compatibility  
- Pixel-perfect multi-monitor beyond one virtual peer rect  
- Matching OS pointer acceleration curves exactly (provide `scale` only)

## 8. Why not lan-mouse / Input Leap

| Approach | Why not for this desk |
|----------|------------------------|
| **lan-mouse** | Works, but clunky handoff, weak layout, relative-only enter, no scale. **Disabled** on novus+ember (unit/wrapper/config quarantined; removed from nix profile). Do not re-enable for daily use. |
| **Input Leap server on River** | Needs EIS/portal; River doesn’t provide it today |
| **sola-kvm (this)** | Own layout math, warm UDP, Sola chord control, only two machines |

## 9. Success criteria

- [ ] Bottoms aligned: enter near bottom of novus → appear near bottom of ember  
- [ ] Switch lag: no crypto handshake; warm path; grab/ungrab only  
- [ ] `motion.scale` makes Mac feel close to novus without maxing Mac OS slider alone  
- [ ] Cmd+Space (and other Cmd chords) work on ember while remote; Meta+Space launcher works again when local  
- [ ] Reboot: log in, start Sola, sola-kvm + Mac agent auto-start; pair-free  
- [ ] Stuck-modifier recovery on leave (synthetic key-ups)

## 10. Implementation plan (phased)

### Phase A — Spec + skeleton (this worktree) ✅

1. This design doc  
2. Crate/binary: `crates/sola-kvm` (server CLI) + `apps/sola-kvm-mac/README.md` (Mac stub)  
3. Packet encode/decode + layout + config unit tests (28 passing)  
4. CLI tools: `show`, `init`, `server` (idle), `listen`, `send-test`  

### Phase B — Mac agent

1. UDP listen + inject motion/button/key  
2. LaunchAgent  
3. Manual feed from `nc`/test tool on novus  

### Phase C — Novus edge + virtual cursor ✅ (logic + grab spike)

1. ~~Read primary output size from sola-river / bus `OutputGeometry`~~ — still config `primary` (bus later)  
2. Edge detect + enter/leave — pure `Session` state machine + layout `try_enter_from_motion`  
3. Grab spike — **evdev `EVIOCGRAB` while remote**; feed/demo backends for no-perms smoke  
4. UDP emit with layout + scale — wired in `run::run_server`  
5. Stuck-modifier recovery on leave (synthetic key/button ups)  
6. Operator doc: `docs/manual/sola-kvm-operator.md`  

**Not done in Phase C (honest):**

- Layer-shell 1px barriers (needs `river_layer_shell_v1` enablement in `sola-river` on this branch; reference: `libei-portal` worktree)  
- Live Wayland pointer warp on leave  
- Automatic Meta chord suppress without layer exclusive focus  
- Bus `OutputGeometry` for primary size  

### Phase D — Polish

1. Autostart  
2. Layer-shell barrier path + sola-river chord suppress port  
3. Compositor warp / abs pointer seed  
4. ~~Drop lan-mouse from daily path~~ — **done on novus** (host purge + docs; secrets quarantined under `~/.config/lan-mouse.disabled`)

## 11. Open decisions

1. **Absolute vs relative motion on the wire** — **chosen: absolute** Mac-local `Motion` after enter.  
2. **Grab mechanism** — Phase C spike = **evdev EVIOCGRAB**; layer-shell barrier remains preferred for precise edge hit once sola-river enables it.  
3. **Ship Mac agent** — app bundle vs single binary + LaunchAgent (Phase B).  
4. **Whether sola-kvm lives in-tree only or also a tiny out-of-repo Mac project.**

## 12. References

- Prior experiment: worktree `libei-portal` (lan-mouse + layer-shell enablement; reverse EIS history)  
- River layer exclusive focus / chord suppress pattern in `sola-river`  
- Desk geometry: novus 5120×2160, ember 2560×2880, peer `10.0.0.21`
