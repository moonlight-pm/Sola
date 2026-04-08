# Phase 1: Minimal Wayland Compositor — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Get a working Smithay-based Wayland compositor that renders a solid color to a real display on canto via DRM/KMS.

**Architecture:** Smithay owns the full display pipeline — DRM/KMS backend with GBM buffer allocation, OpenGL ES rendering, libseat for session management, libinput for input, udev for device discovery. The compositor runs directly on a TTY with no host compositor.

**Tech Stack:** Rust (edition 2024), Smithay 0.7.0, smithay-drm-extras 0.1.0, calloop 0.14, clap 4, tracing

**Note:** Smithay's API is generic-heavy. Code in this plan follows patterns from Smithay's anvil reference compositor. Exact type signatures and trait bounds should be verified against docs.rs/smithay/0.7.0 during implementation if compilation errors arise.

---

## File Structure

```
Sola/
├── .cargo/config.toml                    # cargo make alias
├── Cargo.toml                            # Workspace root
├── crates/
│   ├── sola/
│   │   ├── Cargo.toml
│   │   └── src/main.rs                   # CLI entry point — parses args, calls sola_compositor::run()
│   ├── sola-compositor/
│   │   ├── Cargo.toml                    # Smithay + deps
│   │   └── src/
│   │       ├── lib.rs                    # run() — event loop setup, ties modules together
│   │       ├── state.rs                  # Sola state struct + Wayland protocol delegates
│   │       ├── udev.rs                   # GPU discovery, DRM/GBM/EGL device init, output management
│   │       └── render.rs                 # Render function (solid color for Phase 1)
│   └── sola-make/
│       ├── Cargo.toml
│       └── src/main.rs                   # Build/deploy CLI
└── s                                     # Optional repo-root shortcut
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

- [ ] **Step 1: Convert root Cargo.toml to workspace**

```toml
[workspace]
members = ["crates/*"]
resolver = "3"

[workspace.package]
version = "0.1.0"
edition = "2024"
```

- [ ] **Step 2: Create .cargo/config.toml**

```toml
[alias]
make = "run -q -p sola-make --"
```

- [ ] **Step 3: Create crates/sola/Cargo.toml**

```toml
[package]
name = "sola"
version.workspace = true
edition.workspace = true

[dependencies]
sola-compositor = { path = "../sola-compositor" }
clap = { version = "4", features = ["derive"] }
tracing-subscriber = { version = "0.3", features = ["env-filter"] }
```

- [ ] **Step 4: Create crates/sola/src/main.rs**

```rust
use clap::Parser;

#[derive(Parser)]
#[command(name = "sola", about = "Sola desktop shell")]
struct Cli {}

fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "sola=info,sola_compositor=info".into()),
        )
        .init();

    let _cli = Cli::parse();
    sola_compositor::run()
}
```

- [ ] **Step 5: Create crates/sola-compositor/Cargo.toml**

```toml
[package]
name = "sola-compositor"
version.workspace = true
edition.workspace = true

[dependencies]
smithay = { version = "0.7.0", default-features = false, features = [
    "backend_drm",
    "backend_gbm",
    "backend_egl",
    "backend_libinput",
    "backend_udev",
    "backend_session_libseat",
    "renderer_gl",
    "renderer_multi",
    "wayland_frontend",
    "desktop",
] }
smithay-drm-extras = "0.1.0"
tracing = "0.1"
anyhow = "1"
```

- [ ] **Step 6: Create crates/sola-compositor/src/lib.rs (stub)**

```rust
pub fn run() -> anyhow::Result<()> {
    tracing::info!("sola compositor starting");
    Ok(())
}
```

- [ ] **Step 7: Create crates/sola-make/Cargo.toml**

```toml
[package]
name = "sola-make"
version.workspace = true
edition.workspace = true

[dependencies]
clap = { version = "4", features = ["derive"] }
```

- [ ] **Step 8: Create crates/sola-make/src/main.rs (stub)**

```rust
use clap::Parser;

#[derive(Parser)]
#[command(name = "sola-make", about = "Sola build system")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(clap::Subcommand)]
enum Commands {
    /// Build the project
    Build,
}

fn main() {
    let cli = Cli::parse();
    match cli.command {
        Commands::Build => println!("build: not yet implemented"),
    }
}
```

- [ ] **Step 9: Remove old src/main.rs, update .gitignore**

Delete `src/main.rs` and the `src/` directory. Add to `.gitignore`:

```
/target
/.worktrees
```

- [ ] **Step 10: Verify compilation**

Run: `cargo check`
Expected: compiles with no errors.

Run: `cargo make build`
Expected: prints "build: not yet implemented"

- [ ] **Step 11: Commit**

```bash
git add -A
git commit -m "Set up workspace with sola, sola-compositor, sola-make crates"
```

---

### Task 2: sola-make Build Command

**Files:**
- Modify: `crates/sola-make/src/main.rs`

- [ ] **Step 1: Implement build command with optional target**

```rust
use clap::Parser;
use std::process::Command;

#[derive(Parser)]
#[command(name = "sola-make", about = "Sola build system")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(clap::Subcommand)]
enum Commands {
    /// Build the project
    Build {
        /// Specific crate to build (e.g. "sola", "sola-compositor")
        target: Option<String>,

        /// Build in release mode
        #[arg(long)]
        release: bool,
    },
}

fn main() {
    let cli = Cli::parse();
    match cli.command {
        Commands::Build { target, release } => build(target, release),
    }
}

fn build(target: Option<String>, release: bool) {
    let mut cmd = Command::new("cargo");
    cmd.arg("build");

    if let Some(ref target) = target {
        cmd.args(["-p", target]);
    }

    if release {
        cmd.arg("--release");
    }

    let status = cmd.status().expect("failed to run cargo build");
    if !status.success() {
        std::process::exit(status.code().unwrap_or(1));
    }
}
```

- [ ] **Step 2: Verify**

Run: `cargo make build`
Expected: builds the entire workspace.

Run: `cargo make build sola-compositor`
Expected: builds only sola-compositor.

- [ ] **Step 3: Commit**

```bash
git add crates/sola-make/src/main.rs
git commit -m "Implement cargo make build command"
```

---

### Task 3: Compositor State and Wayland Display

**Files:**
- Create: `crates/sola-compositor/src/state.rs`
- Modify: `crates/sola-compositor/src/lib.rs`

This sets up the core Sola state struct with all required Wayland protocol delegates and the calloop event loop. Even though Phase 1 has no clients, Smithay's architecture requires these delegates.

- [ ] **Step 1: Create state.rs with Sola struct and delegates**

```rust
use smithay::delegate_compositor;
use smithay::delegate_data_device;
use smithay::delegate_output;
use smithay::delegate_seat;
use smithay::delegate_shm;
use smithay::delegate_xdg_shell;
use smithay::input::{SeatHandler, SeatState};
use smithay::reexports::calloop::LoopHandle;
use smithay::reexports::wayland_server::Display;
use smithay::reexports::wayland_server::protocol::wl_surface::WlSurface;
use smithay::wayland::compositor::{self, CompositorHandler, CompositorState};
use smithay::wayland::output::OutputManagerState;
use smithay::wayland::selection::data_device::{
    ClientDndGrabHandler, DataDeviceHandler, DataDeviceState, ServerDndGrabHandler,
};
use smithay::wayland::shell::xdg::{XdgShellHandler, XdgShellState};
use smithay::wayland::shm::{ShmHandler, ShmState};

pub struct Sola {
    pub running: bool,
    pub display: Display<Self>,
    pub loop_handle: LoopHandle<'static, Self>,

    // Wayland protocol state
    pub compositor_state: CompositorState,
    pub shm_state: ShmState,
    pub seat_state: SeatState<Self>,
    pub data_device_state: DataDeviceState,
    pub output_manager_state: OutputManagerState,
    pub xdg_shell_state: XdgShellState,
}

impl Sola {
    pub fn new(
        display: Display<Self>,
        loop_handle: LoopHandle<'static, Self>,
    ) -> Self {
        let dh = display.handle();

        let compositor_state = CompositorState::new::<Self>(&dh);
        let shm_state = ShmState::new::<Self>(&dh, vec![]);
        let seat_state = SeatState::new();
        let data_device_state = DataDeviceState::new::<Self>(&dh);
        let output_manager_state = OutputManagerState::new_with_xdg_output::<Self>(&dh);
        let xdg_shell_state = XdgShellState::new::<Self>(&dh);

        Self {
            running: true,
            display,
            loop_handle,
            compositor_state,
            shm_state,
            seat_state,
            data_device_state,
            output_manager_state,
            xdg_shell_state,
        }
    }
}

// --- Wayland protocol handler implementations ---

impl CompositorHandler for Sola {
    fn compositor_state(&mut self) -> &mut CompositorState {
        &mut self.compositor_state
    }

    fn client_compositor_state<'a>(
        &self,
        client: &'a smithay::reexports::wayland_server::Client,
    ) -> &'a compositor::CompositorClientState {
        &client.get_data::<compositor::CompositorClientState>().unwrap()
    }

    fn commit(&mut self, _surface: &WlSurface) {
        // No client handling in Phase 1
    }
}
delegate_compositor!(Sola);

impl ShmHandler for Sola {
    fn shm_state(&self) -> &ShmState {
        &self.shm_state
    }
}
delegate_shm!(Sola);

impl SeatHandler for Sola {
    type KeyboardFocus = WlSurface;
    type PointerFocus = WlSurface;
    type TouchFocus = WlSurface;

    fn seat_state(&mut self) -> &mut SeatState<Self> {
        &mut self.seat_state
    }

    fn cursor_image(
        &mut self,
        _seat: &smithay::input::Seat<Self>,
        _image: smithay::input::pointer::CursorImageStatus,
    ) {
    }

    fn focus_changed(
        &mut self,
        _seat: &smithay::input::Seat<Self>,
        _focused: Option<&Self::KeyboardFocus>,
    ) {
    }
}
delegate_seat!(Sola);

impl DataDeviceHandler for Sola {
    fn data_device_state(&self) -> &DataDeviceState {
        &self.data_device_state
    }
}
impl ClientDndGrabHandler for Sola {}
impl ServerDndGrabHandler for Sola {}
delegate_data_device!(Sola);

delegate_output!(Sola);

impl XdgShellHandler for Sola {
    fn xdg_shell_state(&mut self) -> &mut XdgShellState {
        &mut self.xdg_shell_state
    }

    fn new_toplevel(&mut self, _surface: smithay::wayland::shell::xdg::ToplevelSurface) {}
    fn new_popup(
        &mut self,
        _surface: smithay::wayland::shell::xdg::PopupSurface,
        _positioner: smithay::wayland::shell::xdg::PositionerState,
    ) {
    }
    fn grab(
        &mut self,
        _surface: smithay::wayland::shell::xdg::PopupSurface,
        _seat: smithay::reexports::wayland_server::protocol::wl_seat::WlSeat,
        _serial: smithay::utils::Serial,
    ) {
    }
}
delegate_xdg_shell!(Sola);
```

- [ ] **Step 2: Update lib.rs to create event loop and state**

```rust
mod state;

use smithay::reexports::calloop::EventLoop;
use smithay::reexports::wayland_server::Display;

pub use state::Sola;

pub fn run() -> anyhow::Result<()> {
    tracing::info!("sola compositor starting");

    let mut event_loop: EventLoop<Sola> = EventLoop::try_new()?;
    let display: Display<Sola> = Display::new()?;

    let mut sola = Sola::new(display, event_loop.handle());

    // Insert Wayland display source into event loop
    let display_source = smithay::reexports::calloop::generic::Generic::new(
        sola.display.backend().poll_fd().as_fd().try_clone_to_owned()?,
        calloop::Interest::READ,
        calloop::Mode::Level,
    );
    event_loop
        .handle()
        .insert_source(display_source, |_, _, sola| {
            sola.display.dispatch_clients(&mut ()).unwrap();
            Ok(calloop::PostAction::Continue)
        })?;

    tracing::info!("entering event loop");
    while sola.running {
        event_loop.dispatch(Some(std::time::Duration::from_millis(16)), &mut sola)?;
    }

    tracing::info!("sola compositor shutting down");
    Ok(())
}
```

- [ ] **Step 3: Verify compilation**

Run: `cargo check`
Expected: compiles. May require adjusting exact import paths — verify against docs.rs/smithay/0.7.0 if needed.

- [ ] **Step 4: Commit**

```bash
git add crates/sola-compositor/src/state.rs crates/sola-compositor/src/lib.rs
git commit -m "Add compositor state with Wayland protocol delegates and event loop"
```

---

### Task 4: Session and GPU Initialization

**Files:**
- Create: `crates/sola-compositor/src/udev.rs`
- Modify: `crates/sola-compositor/src/state.rs`
- Modify: `crates/sola-compositor/src/lib.rs`

This task adds libseat session management and GPU device initialization via udev.

- [ ] **Step 1: Create udev.rs with device initialization**

```rust
use std::collections::HashMap;
use std::path::Path;

use smithay::backend::allocator::gbm::{GbmAllocator, GbmBufferFlags, GbmDevice};
use smithay::backend::drm::{DrmDevice, DrmDeviceFd, DrmEvent, DrmNode, NodeType};
use smithay::backend::egl::{EGLDevice, EGLDisplay};
use smithay::backend::renderer::gles::GlesRenderer;
use smithay::backend::renderer::multigpu::{GbmGlesBackend, GpuManager};
use smithay::backend::session::libseat::LibSeatSession;
use smithay::backend::session::Session;
use smithay::backend::udev::{self, UdevBackend, UdevEvent};
use smithay::reexports::calloop::RegistrationToken;
use smithay::reexports::rustix::fs::OFlags;
use smithay_drm_extras::drm_scanner::DrmScanner;

use crate::Sola;

/// Per-GPU device state.
pub struct GpuDevice {
    pub drm: DrmDevice,
    pub gbm: GbmDevice<DrmDeviceFd>,
    pub drm_scanner: DrmScanner,
    pub registration_token: RegistrationToken,
}

/// Find the primary GPU node for the given seat.
pub fn find_primary_gpu(seat: &str) -> anyhow::Result<DrmNode> {
    // Use smithay-drm-extras to find the primary GPU
    let primary = udev::primary_gpu(seat)?
        .and_then(|p| DrmNode::from_path(&p).ok());

    // Fall back to first available render node
    if let Some(node) = primary {
        tracing::info!(?node, "found primary GPU");
        Ok(node)
    } else {
        anyhow::bail!("no GPU found for seat {seat}");
    }
}

/// Initialize a DRM device from a udev device path.
pub fn device_added(sola: &mut Sola, node: DrmNode, path: &Path) -> anyhow::Result<()> {
    // Open the DRM device via the session (libseat handles permissions)
    let fd = sola.session.open(
        path,
        OFlags::RDWR | OFlags::CLOEXEC | OFlags::NOCTTY | OFlags::NONBLOCK,
    )?;
    let drm_fd = DrmDeviceFd::new(unsafe { smithay::backend::drm::DeviceFd::from(fd) });

    let (drm, drm_notifier) = DrmDevice::new(drm_fd.clone(), true)?;
    let gbm = GbmDevice::new(drm_fd.clone())?;

    // Register GBM device with GPU manager for rendering
    let render_node = node
        .node_with_type(NodeType::Render)
        .unwrap_or(Some(node))
        .unwrap_or(node);
    sola.gpu_manager.as_mut().unwrap().add_node(render_node, gbm.clone())?;

    // Insert DRM event source into event loop for VBlank handling
    let token = sola
        .loop_handle
        .insert_source(drm_notifier, move |event, _metadata, sola| match event {
            DrmEvent::VBlank(crtc) => {
                crate::render::on_vblank(sola, node, crtc);
            }
            DrmEvent::Error(err) => {
                tracing::error!(?err, "DRM error");
            }
        })?;

    let device = GpuDevice {
        drm,
        gbm,
        drm_scanner: DrmScanner::new(),
        registration_token: token,
    };

    sola.devices.insert(node, device);
    tracing::info!(?node, "GPU device initialized");

    // Scan for connected outputs
    device_changed(sola, node)?;

    Ok(())
}

/// Re-scan connectors when a device change event fires.
pub fn device_changed(sola: &mut Sola, node: DrmNode) -> anyhow::Result<()> {
    let device = sola.devices.get_mut(&node).unwrap();
    let scan_result = device.drm_scanner.scan_connectors(&device.drm);

    for event in scan_result {
        match event {
            smithay_drm_extras::drm_scanner::DrmScanEvent::Connected { connector, crtc } => {
                if let Some(crtc) = crtc {
                    crate::render::connector_connected(sola, node, connector, crtc)?;
                }
            }
            smithay_drm_extras::drm_scanner::DrmScanEvent::Disconnected { connector, crtc } => {
                tracing::info!(?connector, ?crtc, "connector disconnected");
            }
        }
    }

    Ok(())
}
```

- [ ] **Step 2: Add backend fields to Sola state**

Add these fields to the `Sola` struct in `state.rs`:

```rust
use std::collections::HashMap;
use smithay::backend::drm::DrmNode;
use smithay::backend::renderer::gles::GlesRenderer;
use smithay::backend::renderer::multigpu::{GbmGlesBackend, GpuManager};
use smithay::backend::session::libseat::LibSeatSession;
use smithay::output::Output;

// Add to Sola struct:
    // Backend state
    pub session: LibSeatSession,
    pub gpu_manager: Option<GpuManager<GbmGlesBackend<GlesRenderer>>>,
    pub primary_gpu: DrmNode,
    pub devices: HashMap<DrmNode, crate::udev::GpuDevice>,

    // Output state
    pub outputs: HashMap<smithay::backend::drm::compositor::DrmCompositor<...>, OutputState>,
```

The DrmCompositor has complex generic parameters. For Phase 1, output state is stored per-device in `GpuDevice` (via the `udev` module) rather than on the Sola struct directly. Add only the backend fields:

```rust
use std::collections::HashMap;
use smithay::backend::drm::DrmNode;
use smithay::backend::renderer::gles::GlesRenderer;
use smithay::backend::renderer::multigpu::{GbmGlesBackend, GpuManager};
use smithay::backend::session::libseat::LibSeatSession;

// Add to Sola struct:
    pub session: LibSeatSession,
    pub gpu_manager: Option<GpuManager<GbmGlesBackend<GlesRenderer>>>,
    pub primary_gpu: DrmNode,
    pub devices: HashMap<DrmNode, crate::udev::GpuDevice>,
```

- [ ] **Step 3: Update lib.rs to initialize session and udev**

```rust
mod render;
mod state;
mod udev;

use smithay::backend::session::libseat::LibSeatSession;
use smithay::backend::session::Session;
use smithay::backend::udev::{UdevBackend, UdevEvent};
use smithay::reexports::calloop::EventLoop;
use smithay::reexports::wayland_server::Display;

pub use state::Sola;

pub fn run() -> anyhow::Result<()> {
    tracing::info!("sola compositor starting");

    let mut event_loop: EventLoop<Sola> = EventLoop::try_new()?;
    let display: Display<Sola> = Display::new()?;

    // Initialize libseat session
    let (session, session_notifier) = LibSeatSession::new()?;
    let seat_name = session.seat();
    tracing::info!(seat = %seat_name, "session opened");

    // Find primary GPU
    let primary_gpu = udev::find_primary_gpu(&seat_name)?;

    // Create GPU manager
    let gpu_manager = smithay::backend::renderer::multigpu::GpuManager::new(
        smithay::backend::renderer::multigpu::GbmGlesBackend::default(),
    )?;

    let mut sola = Sola::new(display, event_loop.handle(), session, gpu_manager, primary_gpu);

    // Insert session notifier into event loop (handles VT switching)
    event_loop
        .handle()
        .insert_source(session_notifier, |_, _, _| {})?;

    // Set up udev device scanning
    let udev_backend = UdevBackend::new(&seat_name)?;

    // Initialize already-present GPUs
    for (device_id, path) in udev_backend.device_list() {
        if let Ok(node) = smithay::backend::drm::DrmNode::from_dev_id(device_id) {
            if node.node_with_type(smithay::backend::drm::NodeType::Primary).is_some() {
                if let Err(err) = udev::device_added(&mut sola, node, &path) {
                    tracing::error!(?err, ?node, "failed to initialize GPU");
                }
            }
        }
    }

    // Listen for GPU hotplug (rare but correct to handle)
    event_loop.handle().insert_source(udev_backend, |event, _, sola| {
        if let UdevEvent::Added { device_id, path } = event {
            if let Ok(node) = smithay::backend::drm::DrmNode::from_dev_id(device_id) {
                let _ = udev::device_added(sola, node, &path);
            }
        }
    })?;

    // Insert Wayland display source into event loop
    event_loop.handle().insert_source(
        smithay::reexports::calloop::generic::Generic::new(
            sola.display.backend().poll_fd().as_fd().try_clone_to_owned()?,
            smithay::reexports::calloop::Interest::READ,
            smithay::reexports::calloop::Mode::Level,
        ),
        |_, _, sola| {
            sola.display.dispatch_clients(&mut ()).unwrap();
            Ok(smithay::reexports::calloop::PostAction::Continue)
        },
    )?;

    tracing::info!("entering event loop");
    while sola.running {
        event_loop.dispatch(Some(std::time::Duration::from_millis(16)), &mut sola)?;
    }

    tracing::info!("sola compositor shutting down");
    Ok(())
}
```

- [ ] **Step 4: Update Sola::new() to accept backend params**

Update the `new()` constructor in state.rs to accept the session, gpu_manager, and primary_gpu parameters. Wire them into the struct fields.

- [ ] **Step 5: Create render.rs stub**

```rust
use smithay::backend::drm::DrmNode;
use smithay::reexports::drm::control::crtc;

use crate::Sola;

pub fn connector_connected(
    _sola: &mut Sola,
    _node: DrmNode,
    _connector: smithay::reexports::drm::control::connector::Info,
    _crtc: crtc::Handle,
) -> anyhow::Result<()> {
    tracing::info!("connector_connected: stub — output setup in next task");
    Ok(())
}

pub fn on_vblank(_sola: &mut Sola, _node: DrmNode, _crtc: crtc::Handle) {
    // Will handle frame submission in next task
}
```

- [ ] **Step 6: Verify compilation**

Run: `cargo check`
Expected: compiles. Fix any import path issues by checking docs.rs/smithay/0.7.0.

- [ ] **Step 7: Commit**

```bash
git add crates/sola-compositor/src/
git commit -m "Add session management and GPU device initialization via udev"
```

---

### Task 5: Output Setup and Rendering

**Files:**
- Modify: `crates/sola-compositor/src/render.rs`
- Modify: `crates/sola-compositor/src/udev.rs`
- Modify: `crates/sola-compositor/src/state.rs`

This is the core task — connecting a display output and rendering a solid color to it.

- [ ] **Step 1: Implement connector_connected in render.rs**

```rust
use smithay::backend::allocator::gbm::{GbmAllocator, GbmBufferFlags};
use smithay::backend::drm::compositor::DrmCompositor;
use smithay::backend::drm::{DrmNode, DrmSurface};
use smithay::backend::renderer::element::RenderElement;
use smithay::backend::renderer::gles::GlesRenderer;
use smithay::backend::renderer::{Color32F, Frame, Renderer};
use smithay::output::{Mode as WlMode, Output, PhysicalProperties, Subpixel};
use smithay::reexports::drm::control::{self, connector, crtc, ModeTypeFlags};
use smithay::utils::{Physical, Rectangle, Size, Transform};

use crate::Sola;

/// Background color — dark blue
const CLEAR_COLOR: Color32F = Color32F::new(0.1, 0.1, 0.2, 1.0);

pub fn connector_connected(
    sola: &mut Sola,
    node: DrmNode,
    connector: connector::Info,
    crtc: crtc::Handle,
) -> anyhow::Result<()> {
    let device = sola.devices.get(&node).unwrap();

    // Pick the preferred mode, or first available
    let mode = connector
        .modes()
        .iter()
        .find(|m| m.mode_type().contains(ModeTypeFlags::PREFERRED))
        .or_else(|| connector.modes().first())
        .copied()
        .ok_or_else(|| anyhow::anyhow!("no modes available on connector"))?;

    tracing::info!(
        ?connector,
        ?crtc,
        width = mode.size().0,
        height = mode.size().1,
        refresh = mode.vrefresh(),
        "output connected"
    );

    // Create the Wayland output object
    let output = Output::new(
        format!("{}-{}", connector.interface().as_str(), connector.interface_id()),
        PhysicalProperties {
            size: (connector.size().unwrap_or((0, 0)).0 as i32, connector.size().unwrap_or((0, 0)).1 as i32).into(),
            subpixel: Subpixel::Unknown,
            make: "Unknown".into(),
            model: "Unknown".into(),
        },
    );

    let wl_mode = WlMode {
        size: (mode.size().0 as i32, mode.size().1 as i32).into(),
        refresh: (mode.vrefresh() * 1000) as i32,
    };
    output.change_current_state(Some(wl_mode), Some(Transform::Normal), None, None);
    output.set_preferred(wl_mode);

    // Create the output global for Wayland clients
    output.create_global::<Sola>(&sola.display.handle());

    tracing::info!("output configured, scheduling first render");

    // The DrmCompositor setup is the most type-complex part of Smithay.
    // The implementation engineer MUST open anvil's source (anvil/src/udev.rs,
    // function `connector_connected`) side-by-side and adapt the following sequence:
    //
    // 1. Create a DrmSurface from the DRM device for this crtc + connector + mode
    // 2. Get the plane list from drm.planes(&crtc)
    // 3. If the driver is NVIDIA, clear overlay planes (see is_nvidia helper below)
    // 4. Create a GbmAllocator from the GBM device
    // 5. Get render formats from the GPU manager
    // 6. Construct DrmCompositor::new() with the surface, planes, allocator, formats
    // 7. Store the DrmCompositor in the GpuDevice's output map (keyed by crtc)
    // 8. Call render_output() to draw the first frame
    //
    // The exact generic parameters on DrmCompositor depend on Smithay 0.7.0's API.
    // Do not guess — read the anvil source and docs.rs/smithay/0.7.0.

    Ok(())
}

pub fn on_vblank(sola: &mut Sola, node: DrmNode, crtc: crtc::Handle) {
    // 1. Get the DrmCompositor for this crtc from sola.devices[node]
    // 2. Call drm_compositor.frame_submitted() to release the scanout buffer
    // 3. Schedule next render via sola.loop_handle.insert_idle(render_output)
}

pub fn render_output(sola: &mut Sola, node: DrmNode, crtc: crtc::Handle) {
    // 1. Get a renderer: sola.gpu_manager.single_renderer(&render_node)
    // 2. Build empty elements list (no windows in Phase 1)
    // 3. drm_compositor.render_frame(&mut renderer, &elements, CLEAR_COLOR)
    // 4. drm_compositor.queue_frame(()) — submits buffer for scanout via page flip
    //
    // After queue_frame, the DRM subsystem will fire a VBlank event when the
    // flip completes, which triggers on_vblank above, completing the loop.
}
```

**Important implementation note:** The `DrmCompositor` setup is the most type-complex part of Smithay. During implementation, the engineer MUST reference Smithay's anvil example (`anvil/src/udev.rs`, the `connector_connected` function) for the exact generic parameters, plane selection, and format negotiation. The pseudocode above shows the flow; the exact types depend on Smithay 0.7.0's `DrmCompositor::new()` signature.

- [ ] **Step 2: Wire up the NVIDIA overlay plane workaround**

When initializing planes for DrmCompositor, check the DRM driver:

```rust
use smithay::backend::drm::DrmDevice;
use smithay::reexports::drm::control::Device;

fn is_nvidia(drm: &DrmDevice) -> bool {
    let driver = drm.get_driver().ok();
    driver
        .as_ref()
        .map(|d| {
            d.name().to_string_lossy().to_lowercase().contains("nvidia")
                || d.description().to_string_lossy().to_lowercase().contains("nvidia")
        })
        .unwrap_or(false)
}

// When selecting planes:
// if is_nvidia(&device.drm) {
//     planes.overlay = vec![];
// }
```

- [ ] **Step 3: Verify compilation**

Run: `cargo check`
Expected: compiles (the TODO sections are documented pseudocode, not actual incomplete code — during implementation these will be filled in based on the actual Smithay API).

- [ ] **Step 4: Commit**

```bash
git add crates/sola-compositor/src/render.rs
git commit -m "Add output setup and solid color rendering"
```

---

### Task 6: Input and Lifecycle

**Files:**
- Modify: `crates/sola-compositor/src/lib.rs`
- Modify: `crates/sola-compositor/src/state.rs`

Add libinput for basic keyboard/mouse input (needed for VT switching and eventual interaction), and signal handling for clean shutdown.

- [ ] **Step 1: Add libinput setup to lib.rs**

After the udev backend setup, add:

```rust
use smithay::backend::input::InputEvent;
use smithay::backend::libinput::{LibinputInputBackend, LibinputSessionInterface};

// Create libinput context
let mut libinput_context =
    smithay::input::libinput::Libinput::new_with_udev(LibinputSessionInterface::from(
        sola.session.clone(),
    ));
libinput_context.udev_assign_seat(&seat_name)?;
let libinput_backend = LibinputInputBackend::new(libinput_context);

// Insert libinput into event loop
event_loop.handle().insert_source(libinput_backend, |event, _, sola| {
    // For Phase 1, just log input events
    match event {
        InputEvent::Keyboard { event } => {
            tracing::trace!("keyboard event");
        }
        _ => {}
    }
})?;
```

- [ ] **Step 2: Add signal handling for clean shutdown**

```rust
use smithay::reexports::calloop::signals::{Signal, Signals};

// Handle SIGINT and SIGTERM for clean shutdown
event_loop.handle().insert_source(
    Signals::new(&[Signal::SIGINT, Signal::SIGTERM])?,
    |signal, _, sola| {
        tracing::info!(?signal, "received signal, shutting down");
        sola.running = false;
    },
)?;
```

- [ ] **Step 3: Verify compilation**

Run: `cargo check`
Expected: compiles.

- [ ] **Step 4: Commit**

```bash
git add crates/sola-compositor/src/
git commit -m "Add libinput handling and signal-based shutdown"
```

---

### Task 7: Deploy Infrastructure

**Files:**
- Modify: `crates/sola-make/src/main.rs`

- [ ] **Step 1: Add deploy command to sola-make**

```rust
use clap::Parser;
use std::process::Command;

#[derive(Parser)]
#[command(name = "sola-make", about = "Sola build system")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(clap::Subcommand)]
enum Commands {
    /// Build the project
    Build {
        /// Specific crate to build
        target: Option<String>,
        /// Build in release mode
        #[arg(long)]
        release: bool,
    },
    /// Deploy to a target machine
    Deploy {
        /// Target machine (e.g. "canto")
        target: String,
    },
}

fn main() {
    let cli = Cli::parse();
    match cli.command {
        Commands::Build { target, release } => build(target, release),
        Commands::Deploy { target } => deploy(&target),
    }
}

fn build(target: Option<String>, release: bool) {
    let mut cmd = Command::new("cargo");
    cmd.arg("build");
    if let Some(ref target) = target {
        cmd.args(["-p", target]);
    }
    if release {
        cmd.arg("--release");
    }
    let status = cmd.status().expect("failed to run cargo build");
    if !status.success() {
        std::process::exit(status.code().unwrap_or(1));
    }
}

fn deploy(target: &str) {
    match target {
        "canto" => deploy_canto(),
        other => {
            eprintln!("unknown deploy target: {other}");
            std::process::exit(1);
        }
    }
}

fn deploy_canto() {
    // Build release first
    println!("Building release...");
    build(None, true);

    // Ensure remote directory exists
    println!("Preparing canto...");
    let status = Command::new("ssh")
        .args(["canto", "mkdir -p /opt/sola/bin"])
        .status()
        .expect("failed to ssh to canto");
    if !status.success() {
        eprintln!("failed to create remote directory");
        std::process::exit(1);
    }

    // rsync the sola binary
    println!("Deploying sola to canto...");
    let status = Command::new("rsync")
        .args(["-az", "--progress", "target/release/sola", "canto:/opt/sola/bin/"])
        .status()
        .expect("failed to rsync");
    if !status.success() {
        eprintln!("rsync failed");
        std::process::exit(1);
    }

    println!("Deployed to canto:/opt/sola/bin/sola");
}
```

- [ ] **Step 2: Verify**

Run: `cargo make deploy canto`
Expected: builds release, rsync's binary to canto.

- [ ] **Step 3: Commit**

```bash
git add crates/sola-make/src/main.rs
git commit -m "Add cargo make deploy canto command"
```

---

### Task 8: Integration Verification on Canto

This task is manual verification on real hardware.

- [ ] **Step 1: Deploy to canto**

```bash
cargo make deploy canto
```

- [ ] **Step 2: SSH to canto and verify the binary exists**

```bash
ssh canto ls -la /opt/sola/bin/sola
```

- [ ] **Step 3: On canto's physical TTY, run sola**

Switch to a free TTY (Ctrl+Alt+F3 or similar) and run:

```bash
/opt/sola/bin/sola
```

Expected: the screen fills with a solid dark blue color (CLEAR_COLOR). The compositor owns the display.

- [ ] **Step 4: Verify clean shutdown**

Press Ctrl+C on the TTY.
Expected: sola logs "received signal, shutting down" and exits cleanly, returning to the TTY prompt.

- [ ] **Step 5: Note any issues**

If the compositor panics, check:
- Is `seatd` running? (`systemctl status seatd`)
- Is the user in the `seat` group?
- Are NVIDIA drivers loaded? (`nvidia-smi`)
- Check `RUST_LOG=debug /opt/sola/bin/sola` for detailed logs.

---

## Implementation Notes

### Smithay API Complexity

The most challenging part of this plan is Task 5 (output setup and rendering). Smithay's `DrmCompositor` has complex generic parameters that depend on the allocator, framebuffer exporter, and session types. The plan provides the architectural flow and key code, but the exact type signatures must be worked out during implementation by referencing:

1. **Smithay's anvil example** — `anvil/src/udev.rs` is the authoritative reference for DRM/KMS compositor setup
2. **docs.rs/smithay/0.7.0** — for exact type signatures and trait bounds
3. **Smithay's GitHub** — github.com/Smithay/smithay for latest examples

### NVIDIA Considerations

- Overlay planes MUST be disabled for NVIDIA GPUs (causes atomic commit failures)
- GBM is fully supported on driver 495+ (canto has 590.48.01)
- Prefer ARGB8888 format, fall back from 10-bit if needed

### Testing Strategy

This is hardware-dependent code. Unit tests are not practical for DRM/KMS initialization. The primary verification method is:
1. Code compiles (`cargo check`)
2. Binary runs on real hardware (canto)
3. Visual verification (solid color on screen)
4. Clean shutdown (signal handling works)
