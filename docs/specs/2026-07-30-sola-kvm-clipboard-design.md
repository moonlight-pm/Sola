# sola-kvm clipboard — Design

**Date:** 2026-07-30  
**Status:** Implemented (text + `image/png`, Enter/Leave, hash cache)  
**Depends on:** `docs/specs/2026-07-27-sola-kvm-design.md` (input path)  
**Hosts:** novus (Linux server) ↔ Linux `sola-kvm listen` or ember (`sola-kvm-mac`)  
**Gaps:** Mac client still text-only (PNG Offer is Ack-rejected); no JPEG/other image MIMEs on the wire; no CLIP1 on dump-only listen

## 1. Goal

Share the **system clipboard** when the pointer moves between machines — the
same moment the seat already crosses the edge (Enter / Leave). No continuous
clipboard watching, no file drop, no rich paste.

| Direction | When | Meaning |
|-----------|------|---------|
| **novus → ember** | **Enter** remote | Whatever was on Linux clipboard becomes Mac pasteboard |
| **ember → novus** | **Leave** remote | Whatever is on Mac pasteboard becomes Linux clipboard |

Daily use: copy on one side, cross the edge, paste on the other.

## 2. Non-goals (v1)

- Images, HTML, RTF, file lists, custom UTI / mime stacks  
- Live “on every clipboard change, stream to peer”  
- Intercepting ⌘C / Ctrl+C to force a mid-session push  
- TLS / auth (same trusted-LAN assumption as input UDP)  
- Bidirectional input (Mac controlling novus)  
- Primary selection (X11-style) — **clipboard only**

## 3. Separation from input path

Clipboard must **not** share the motion/key UDP hot path.

| Concern | Input (today) | Clipboard (this) |
|---------|---------------|------------------|
| Transport | UDP unicast `peer.port` | **TCP on the same `peer.port`** (different protocol; no +1) |
| Thread | grab / barrier / inject | **Dedicated worker thread** each side |
| Payload | ≤ 64-byte events | UTF-8 text up to `max_bytes` (default 1 MiB) |
| Reliability | Lossy motion OK | Ordered, complete transfer required |
| Direction | novus → ember only | **Bidirectional** offers |

Input UDP (`KVM1`) stays unchanged. Clip is a sibling protocol (`CLIP1`).

```
  novus                                      ember
  ┌────────────────┐    UDP :4242            ┌────────────────┐
  │ input / grab   │ ──────────────────────► │ CGEvent inject │
  │   (main loop)  │                         │   (inject thr) │
  └───────┬────────┘                         └───────┬────────┘
          │ channel "sync now"                        │ channel
          ▼                                           ▼
  ┌────────────────┐    TCP :4243            ┌────────────────┐
  │ clip worker    │ ◄─────────────────────► │ clip worker    │
  │ read/write WL  │                         │ NSPasteboard   │
  └────────────────┘                         └────────────────┘
```

Enter / Leave on the input thread only **enqueue** work (`ClipJob::PushToMac`
/ `ClipJob::PullFromMac`). Workers do all I/O and pasteboard/compositor calls.

## 4. Cache (do not re-send unchanged)

Each side keeps a small cache of the **last successfully transferred** text:

```text
last_sent_hash:   Option<u32>   # content we last offered to the peer
last_recv_hash:   Option<u32>   # content we last accepted from the peer
```

**Hash:** CRC32 or xxHash of the UTF-8 bytes (fixed algorithm, documented).
Not cryptographic — collision risk is fine for “skip duplicate.”

### 4.1 Skip rules

Before offering:

1. Read local clipboard text (or empty).  
2. If empty and peer already has empty → **skip**.  
3. If `hash(text) == last_sent_hash` → **skip** (we already sent this body).  
4. If `hash(text) == last_recv_hash` → **skip** (this text *came from* the peer;
   sending it back is a no-op ping-pong).  
5. Else encode `Offer` and send; on success set `last_sent_hash = hash`.

On accept:

1. If `hash == last_recv_hash` (or equals local content) → **skip set**.  
2. Else write pasteboard / Wayland clipboard; set `last_recv_hash = hash`.  
   Optionally set `last_sent_hash = hash` so we don’t echo it on the next leave.

This covers: cross edge, do nothing with clipboard, cross back — **no TCP
payload** after the first transfer of that string.

### 4.2 What invalidates cache

- Successful offer of different content  
- Explicit `Empty` message  
- Process restart (cache is in-memory only)  
- Config change of `max_bytes` is irrelevant to hash identity

## 5. Wire protocol (CLIP1)

TCP stream, little-endian, length-prefixed messages (not UDP datagrams).

### 5.1 Header (every message)

| Field | Type | Notes |
|-------|------|--------|
| `magic` | u32 | `0x43_4c_49_50` (`CLIP` as LE-friendly constant; document exact value in code) |
| `version` | u8 | `1` |
| `type` | u8 | see below |
| `seq` | u32 | per-side monotonic |

### 5.2 Types (v1)

| Type | Name | Payload |
|------|------|---------|
| 1 | `Hello` | `role: u8` (1=novus server, 2=ember client) |
| 2 | `Offer` | `mime: u8`, `len: u32`, `hash: u32`, then `len` raw bytes |
| 3 | `Empty` | none — peer should clear clipboard |
| 4 | `Ack` | `seq: u32` of the Offer/Empty being acked, `status: u8` (0=ok, 1=too large, 2=reject) |
| 5 | `Ping` / `Pong` | optional keepalive if TCP goes idle for hours |

**mime:** `1 = text/plain;charset=utf-8`, `2 = image/png`. Unknown mime is
decoded then Ack-rejected so TCP stays up. Prefer PNG when the compositor
offers both (same rule as kit image clipboard).

**Cap:** if `len > max_bytes`, sender must not send; receiver rejects with
`Ack status=too large` and does not write clipboard.

### 5.3 Connection model

- **ember** listens on `clipboard.port` (same process as UDP agent, extra
  accept loop on the clip worker).  
- **novus** connects on first Enter (or at server start) and **keeps the
  connection warm** for the session.  
- Reconnect with backoff if drop; failed clip must **never** block input
  enter/leave.

## 6. Platform

### 6.1 novus (Linux)

| Priority | Mechanism |
|----------|-----------|
| 1 | **`arboard` crate** (Wayland/X11) — shipped in sola-kvm |
| 2 | Fallback: `wl-paste` / `wl-copy` on PATH or `/run/current-system/sw/bin` |

Read/write **clipboard** only (not primary). UTF-8. Truncate or reject above
`max_bytes`.

### 6.2 ember (macOS)

v1 uses **`pbpaste` / `pbcopy`** CLI (no AppKit link). Later: `NSPasteboard.general`.

No Accessibility permission required for pasteboard.

## 7. Lifecycle hooks

### 7.1 Enter (novus input thread)

```text
on SideEffect::Grab / Enter UDP sent:
  clip_tx.send(ClipJob::PushToMac)   # non-blocking; drop if queue full (log warn)
```

Worker:

```text
text = read_novus_clipboard()
if should_skip(text): return
connect_if_needed()
send Offer | Empty
wait Ack (timeout ~2s)
update last_sent_hash
```

### 7.2 Leave (novus input thread)

```text
on Leave UDP spray / ungrab:
  clip_tx.send(ClipJob::PullFromMac)
```

Worker:

```text
send Request-equivalent: novus asks ember to Offer current pasteboard
  # v1 simplification: ember may Push on Leave notification instead —
  # prefer: novus sends "PullPlease", ember Offers once
if should_skip received: return
write_novus_clipboard(text)
update last_recv_hash
```

**v1 simplification (even simpler):**  

- Enter: novus **Offers** (push).  
- Leave: ember **Offers** (push) when it handles Leave on the input path
  (ember already sees Leave UDP — enqueue clip job there; no PullPlease).

That keeps the protocol offer-only (no Request type required in v1).  
Request/Ack stay available for “too large” and future pull.

Recommended v1 message set in practice: **Hello, Offer, Empty, Ack** only.
Leave-side: ember’s inject/recv path enqueues `ClipJob::PushToNovus` on Leave
the same way novus enqueues on Enter.

## 8. Config

```toml
[peer]
host = "10.0.0.133"
port = 4242

[clipboard]
enable = true
# default = peer.port + 1
port = 4243
max_bytes = 1048576
# enter = novus→ember, leave = ember→novus
sync_on_enter = true
sync_on_leave = true
```

## 9. Failure behavior

| Failure | Behavior |
|---------|----------|
| TCP down | Log warn; skip transfer; input unaffected |
| Timeout waiting Ack | Log warn; leave cache unchanged |
| Oversized local clip | Skip send; log once per hash |
| Empty local | Send `Empty` only if peer might still hold old text (`last_sent_hash.is_some()`) |
| Wayland/pasteboard error | Log; no retry storm (next edge will try again) |

## 10. Logging

- `info` on successful transfer: direction, bytes, hash  
- `info` on skip: `clip skip unchanged hash=…`  
- `warn` on errors / timeouts  
- Never log clipboard **contents**

## 11. Code layout (planned)

| Path | Role |
|------|------|
| `crates/sola-kvm/src/clip/{mod,proto,worker,wayland}.rs` | novus clip |
| `crates/sola-kvm/src/run.rs` | enqueue on Enter/Leave only |
| `crates/sola-kvm/src/config.rs` | `[clipboard]` section |
| `apps/sola-kvm-mac/src/clip/{mod,proto,worker,pasteboard}.rs` | ember clip |
| `apps/sola-kvm-mac/src/agent.rs` | spawn clip worker; enqueue on Leave |

Share proto constants carefully (mirror or small shared crate later — for v1,
mirror like today’s `protocol.rs`).

## 12. Implementation phases

| Phase | Deliverable |
|-------|-------------|
| **C0** | TCP Hello + ping between novus worker and ember worker (no pasteboard) |
| **C1** | Enter: novus text → ember pasteboard + hash cache skip |
| **C2** | Leave: ember text → novus clipboard + cache skip both ways |
| **C3** | Empty, max_bytes, metrics, operator doc |

## 13. Success criteria

- [ ] Copy text on novus → Enter remote → paste on Mac matches  
- [ ] Copy text on Mac while remote → Leave → paste on novus matches  
- [ ] Cross edge twice with same clipboard → second crossing sends no Offer body (skip log)  
- [ ] 2 MiB paste rejected cleanly; input still works  
- [ ] Killing clip TCP does not break mouse/keyboard KVM  

## 14. Decisions locked (this doc)

1. **Separate TCP channel** (not UDP input).  
2. **Separate worker thread** for all clip work on both hosts.  
3. **Sync only on seat move** (Enter / Leave), not continuous watch.  
4. **Hash cache** — do not send if content unchanged / already known.  
5. **UTF-8 text and `image/png`.** Other image MIMEs stay local. Mac
   client applies text only.  
6. **Never block** the input/grab path on clipboard I/O.
