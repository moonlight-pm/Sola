# Phase 1: Minimal Wayland Compositor — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Get a working Smithay-based Wayland compositor that renders a solid color to a real display on canto via DRM/KMS.

**Architecture:** Smithay owns the full display pipeline — DRM/KMS backend with GBM buffer allocation, OpenGL ES rendering, libseat for session management, libinput for input, udev for device discovery. The compositor runs directly on a TTY with no host compositor.

**Tech Stack:** Rust (edition 2024), Smithay 0.7.0, smithay-drm-extras 0.1.0, calloop 0.14, clap 4, tracing

**Note:** Smithay's API is generic-heavy. Code in this plan follows patterns from Smithay's anvil reference compositor. Exact type signatures and trait bounds should be verified against docs.rs/smithay/0.7.0 during implementation if compilation errors arise.

**Documentation standard:** All code must be documented. Explain "what" Smithay/Wayland concepts are and "why" complex Rust techniques are used. Include doc links for further reading. Don't comment basic programming — focus on domain knowledge a reviewer without Wayland experience would need.

**Module conventions:** Prefer many small, focused modules with one-word names. Use directory depth for namespacing (e.g., `backend/session.rs` not `backend_session.rs`).

---

## File Structure

```
Sola/
├── .cargo/config.toml                         # cargo make alias
├── Cargo.toml                                 # Workspace root
├── crates/
│   ├── sola/
│   │   ├── Cargo.toml
│   │   └── src/main.rs                        # CLI entry point
│   ├── sola-compositor/
│   │   ├── Cargo.toml
│   │   └── src/
│   │       ├── lib.rs                         # run() — event loop setup, ties modules together
│   │       ├── state.rs                       # Sola state struct definition
│   │       ├── backend/
│   │       │   ├── mod.rs                     # Re-exports
│   │       │   ├── session.rs                 # libseat session (privilege escalation for DRM)
│   │       │   ├── gpu.rs                     # GPU discovery via udev
│   │       │   ├── device.rs                  # DRM device state and lifecycle
│   │       │   └── input.rs                   # libinput keyboard/mouse handling
│   │       ├── wayland/
│   │       │   ├── mod.rs                     # Re-exports + delegate macros
│   │       │   ├── compositor.rs              # wl_compositor protocol handler
│   │       │   ├── shm.rs                     # wl_shm shared memory handler
│   │       │   ├── seat.rs                    # wl_seat input device handler
│   │       │   ├── shell.rs                   # xdg_shell window management handler
│   │       │   └── data.rs                    # wl_data_device clipboard/DnD handler
│   │       └── output/
│   │           ├── mod.rs                     # Re-exports
│   │           ├── scan.rs                    # Connector scanning and output discovery
│   │           └── render.rs                  # Frame rendering and VBlank handling
│   └── sola-make/
│       ├── Cargo.toml
│       └── src/main.rs                        # Build/deploy CLI
```

---

### Task 1: Workspace Scaffolding

**Files:**
- Modify: `Cargo.toml` (convert to workspace)
- Modify: `.gitignore`
- Create: `.cargo/config.toml`
- Create: `crates/sola/Cargo.toml`
- Create: `crates/sola/src/main.rs`
- Create: `crates/sola-compositor/Cargo.toml`
- Create: `crates/sola-compositor/src/lib.rs`
- Create: `crates/sola-make/Cargo.toml`
- Create: `crates/sola-make/src/main.rs`
- Remove: `src/main.rs`

---

### Task 2: sola-make Build and Deploy Commands

**Files:**
- Modify: `crates/sola-make/src/main.rs`

---

### Task 3: Wayland Protocol Delegates

**Files:**
- Create: `crates/sola-compositor/src/wayland/mod.rs`
- Create: `crates/sola-compositor/src/wayland/compositor.rs`
- Create: `crates/sola-compositor/src/wayland/shm.rs`
- Create: `crates/sola-compositor/src/wayland/seat.rs`
- Create: `crates/sola-compositor/src/wayland/shell.rs`
- Create: `crates/sola-compositor/src/wayland/data.rs`

---

### Task 4: Compositor State

**Files:**
- Create: `crates/sola-compositor/src/state.rs`
- Modify: `crates/sola-compositor/src/lib.rs`

---

### Task 5: Session and GPU Backend

**Files:**
- Create: `crates/sola-compositor/src/backend/mod.rs`
- Create: `crates/sola-compositor/src/backend/session.rs`
- Create: `crates/sola-compositor/src/backend/gpu.rs`
- Create: `crates/sola-compositor/src/backend/device.rs`

---

### Task 6: Output Scanning and Rendering

**Files:**
- Create: `crates/sola-compositor/src/output/mod.rs`
- Create: `crates/sola-compositor/src/output/scan.rs`
- Create: `crates/sola-compositor/src/output/render.rs`

---

### Task 7: Input and Lifecycle

**Files:**
- Create: `crates/sola-compositor/src/backend/input.rs`
- Modify: `crates/sola-compositor/src/lib.rs`

---

### Task 8: Integration Verification on Canto

Manual testing on real hardware.

---

## Implementation Notes

### Smithay API Complexity

Task 6 (output/rendering) is the most type-complex. The DrmCompositor has deeply generic parameters. The implementation engineer MUST reference anvil's source (anvil/src/udev.rs) and docs.rs/smithay/0.7.0.

### NVIDIA Considerations

- Overlay planes MUST be disabled for NVIDIA GPUs (causes atomic commit failures)
- GBM fully supported on driver 495+ (canto has 590.48.01)
- Prefer ARGB8888 format

### Testing Strategy

Hardware-dependent code — unit tests are not practical for DRM/KMS. Verification is: compiles, runs on canto, solid color on screen, clean shutdown.
