# sola-kit CEF Port Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace WebKitGTK + GTK4 with CEF + sctk in `crates/sola-kit/`, keeping the framework's public API (`SolaApp`, `AppCtx`, `WindowHandle`, `BusRegistry`, `asset_bundle!`) intact.

**Architecture:** Single-binary process; main thread runs `CefRunMessageLoop`; background thread polls bus and trampolines into the main thread via `CefPostTask`. Each window = one sctk-managed `xdg_toplevel` paired with one CEF browser in OSR mode; CEF's GPU process delivers dma-bufs via `OnAcceleratedPaint` which we present through `zwp_linux_dmabuf_v1`. Six-checkpoint migration; never merge to master.

**Tech Stack:** Rust, CEF (binary distribution from Spotify CDN, cached at `~/.cache/sola/cef-<ver>/`), `cef` Rust binding crate (decision gate at Task B1), `smithay-client-toolkit` 0.19 (sctk), `wayland-protocols`, `wayland-client`, `swc_core` (unchanged), sola-bus, sola-core.

**Design spec:** `docs/specs/2026-05-04-cef-port-design.md`

**Branch:** Stay on `sola-kit-preact`. Commit at every task. Do not merge to master under any circumstances.

---

## Checkpoint A — CEF Distribution

Goal: `cargo make install-cef` downloads CEF binaries to `~/.cache/sola/cef-<version>/` without touching sola-kit. Build of all crates remains green throughout.

### Task A1: Add `cef` module to sola-make with version constant + path resolver

**Files:**
- Create: `crates/sola-make/src/cef.rs`
- Modify: `crates/sola-make/src/main.rs` (add `mod cef;`)

- [ ] **Step 1: Create `crates/sola-make/src/cef.rs`**

```rust
//! CEF binary distribution: probe + download + path resolution.
//!
//! Single source of truth for CEF version. Bumping is a one-character
//! edit; the cache directory is version-suffixed so multiple versions
//! coexist safely.

use std::path::PathBuf;

/// Pinned CEF release. Update this constant to bump the engine version.
/// Match the binary tarball naming on https://cef-builds.spotifycdn.com/.
pub const CEF_VERSION: &str = "132.3.0+gd62b73a+chromium-132.0.6834.83";

/// Directory name used inside the cache. Stable across version bumps
/// only via the version-suffixed subdirectory.
const CACHE_PREFIX: &str = "cef-";

/// Resolve `~/.cache/sola/cef-<CEF_VERSION>/`.
pub fn cef_path() -> PathBuf {
    let base = dirs::cache_dir()
        .unwrap_or_else(|| PathBuf::from(std::env::var("HOME").unwrap_or_default()).join(".cache"))
        .join("sola");
    base.join(format!("{CACHE_PREFIX}{CEF_VERSION}"))
}

/// Path to the `Release/` subdirectory containing libcef.so + binaries.
pub fn release_dir() -> PathBuf {
    cef_path().join("Release")
}

/// Path to the `Resources/` subdirectory containing icudtl.dat + .pak files.
pub fn resources_dir() -> PathBuf {
    cef_path().join("Resources")
}

/// Path to the `Resources/locales/` subdirectory.
pub fn locales_dir() -> PathBuf {
    resources_dir().join("locales")
}

/// True if a usable CEF tree is present at the cache location.
pub fn is_present() -> bool {
    release_dir().join("libcef.so").exists()
}
```

- [ ] **Step 2: Add `dirs` dependency to sola-make**

In `crates/sola-make/Cargo.toml`, add under `[dependencies]`:

```toml
dirs = "5"
```

- [ ] **Step 3: Wire the module into sola-make**

In `crates/sola-make/src/main.rs`, add near the other `mod` declarations:

```rust
mod cef;
```

- [ ] **Step 4: Verify build**

Run: `cargo make build sola-make`
Expected: builds clean.

- [ ] **Step 5: Commit**

```bash
git add crates/sola-make/Cargo.toml crates/sola-make/src/cef.rs crates/sola-make/src/main.rs
git commit -m "feat(sola-make): add cef module with version pin and path resolution"
```

---

### Task A2: Implement `download_and_extract` and `ensure_cef`

**Files:**
- Modify: `crates/sola-make/src/cef.rs`
- Modify: `crates/sola-make/Cargo.toml`

- [ ] **Step 1: Add download/extract dependencies**

In `crates/sola-make/Cargo.toml`, under `[dependencies]`:

```toml
ureq = { version = "2", features = ["tls"] }
tar = "0.4"
bzip2 = "0.4"
```

(We use blocking `ureq` to avoid pulling tokio just for the downloader.)

- [ ] **Step 2: Implement download + extract**

Append to `crates/sola-make/src/cef.rs`:

```rust
use std::fs;
use std::io::{self, Read};

/// URL for the official Spotify-hosted CEF tarball matching `CEF_VERSION`.
/// Variant is `_linux64_minimal` (drops the C++ wrapper static lib and the
/// example binaries we don't ship).
fn tarball_url() -> String {
    // Spotify URL-encodes the '+' in the version as '%2B'.
    let encoded = CEF_VERSION.replace('+', "%2B");
    format!("https://cef-builds.spotifycdn.com/cef_binary_{encoded}_linux64_minimal.tar.bz2")
}

/// Ensure CEF is present at `cef_path()`. If not, download and extract.
/// Idempotent — short-circuits when libcef.so exists.
pub fn ensure_cef() -> io::Result<PathBuf> {
    let dir = cef_path();
    if is_present() {
        return Ok(dir);
    }
    eprintln!("[cef] not found at {} — downloading {}", dir.display(), CEF_VERSION);
    download_and_extract(&dir)?;
    if !is_present() {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            format!("CEF download completed but libcef.so missing under {}", dir.display()),
        ));
    }
    eprintln!("[cef] installed to {}", dir.display());
    Ok(dir)
}

fn download_and_extract(dir: &Path) -> io::Result<()> {
    let parent = dir.parent().ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "cef_path has no parent"))?;
    fs::create_dir_all(parent)?;

    let url = tarball_url();
    eprintln!("[cef] GET {url}");
    let response = ureq::get(&url)
        .call()
        .map_err(|e| io::Error::new(io::ErrorKind::Other, format!("download failed: {e}")))?;
    let reader = response.into_reader();
    let bz2 = bzip2::read::BzDecoder::new(reader);
    let mut archive = tar::Archive::new(bz2);

    // The tarball contains a top-level dir like `cef_binary_<ver>_linux64_minimal/`.
    // We want its contents, not the dir itself, placed at `dir/`. Easiest:
    // extract to a tmp staging directory, then rename the inner directory.
    let staging = parent.join(format!(".cef-staging-{}", std::process::id()));
    if staging.exists() { fs::remove_dir_all(&staging)?; }
    fs::create_dir_all(&staging)?;

    archive.unpack(&staging)?;

    // Find the single top-level directory inside staging.
    let inner = fs::read_dir(&staging)?
        .filter_map(|e| e.ok())
        .find(|e| e.file_type().map(|t| t.is_dir()).unwrap_or(false))
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "no top-level dir in CEF tarball"))?;

    fs::rename(inner.path(), dir)?;
    fs::remove_dir_all(&staging)?;
    Ok(())
}

use std::path::Path;
```

- [ ] **Step 3: Verify the module compiles**

Run: `cargo make build sola-make`
Expected: builds clean.

- [ ] **Step 4: Manual smoke (optional but recommended)**

Run: `cargo run -p sola-make -- install-cef` — wait, this command doesn't exist yet. We add the CLI in the next task. For now you can write a tiny ad-hoc test:

Create `/tmp/cef-smoke.rs`:

```rust
fn main() {
    let p = sola_make::cef::ensure_cef().expect("cef download");
    println!("CEF at: {}", p.display());
}
```

…actually, sola-make is a binary, not a lib. Skip the manual smoke; we'll smoke-test via the CLI in Task A3.

- [ ] **Step 5: Commit**

```bash
git add crates/sola-make/Cargo.toml crates/sola-make/src/cef.rs
git commit -m "feat(sola-make): implement CEF tarball download and extract"
```

---

### Task A3: Add `install-cef` CLI subcommand

**Files:**
- Modify: `crates/sola-make/src/main.rs`

- [ ] **Step 1: Add the subcommand variant**

Locate the `Commands` enum in `crates/sola-make/src/main.rs`. Add a variant alongside the existing ones:

```rust
/// Download CEF binaries to ~/.cache/sola/cef-<version>/.
/// Idempotent — skips if already present.
InstallCef,
```

- [ ] **Step 2: Wire the dispatch**

In the `match` block where `Commands::Install { … }` is handled, add a new arm:

```rust
Commands::InstallCef => {
    match cef::ensure_cef() {
        Ok(path) => {
            println!("CEF ready at {}", path.display());
        }
        Err(e) => {
            eprintln!("CEF install failed: {e}");
            std::process::exit(1);
        }
    }
}
```

- [ ] **Step 3: Build and run end-to-end**

Run: `cargo make build sola-make`
Expected: builds clean.

Run: `cargo run -p sola-make -- install-cef`
Expected (first run): downloads ~150 MB tarball, extracts, prints `CEF ready at /home/joshua/.cache/sola/cef-132.3.0+...`. Allow ~1-2 minutes.

Run: `cargo run -p sola-make -- install-cef`
Expected (second run): instant — prints `CEF ready at …` because `is_present()` short-circuits.

- [ ] **Step 4: Verify on disk**

Run: `ls ~/.cache/sola/cef-*/Release/libcef.so`
Expected: file exists, several hundred MB.

Run: `ls ~/.cache/sola/cef-*/Resources/`
Expected: contains `icudtl.dat`, `resources.pak`, `locales/` directory.

- [ ] **Step 5: Commit**

```bash
git add crates/sola-make/src/main.rs
git commit -m "feat(sola-make): add install-cef CLI subcommand"
```

---

## Checkpoint B — Empty CEF Window

Goal: sola-kit launches and shows an empty white CEF surface in sola-river. This is the largest single change and the highest-risk checkpoint. If the binding crate proves problematic, we discover it here.

### Task B1: Discover the CEF Rust binding's API surface

**Files:** none (research)

- [ ] **Step 1: Inspect the `cef` crate**

Open https://docs.rs/cef and review the latest version. Find:
- The browser process initialization entry point (likely something like `cef::initialize(&CefSettings)` and `cef::execute_process()`)
- The `CefBrowserHost::create_browser_sync` (or equivalent) signature
- The `CefRenderHandler` trait, especially `on_accelerated_paint` (signature includes the dma-buf fd, format, modifier)
- The `CefMessageRouterBrowserSide::Handler` trait
- The `CefSchemeHandlerFactory` trait
- The `CefPostTask` function for cross-thread main-loop dispatch
- Whether OSR's `external_begin_frame_enabled` is exposed and how `send_external_begin_frame` is called

Note in a scratch file the actual Rust names of:
- `CefInitialize`, `CefExecuteProcess`, `CefRunMessageLoop`, `CefShutdown`, `CefPostTask`
- `CefSettings`, `CefBrowserSettings`, `CefWindowInfo`
- `CefBrowserHost`, `CefBrowser`, `CefFrame`, `CefRequestContext`
- `CefAcceleratedPaintInfo` and the dma-buf fields (fd, format, modifier, plane offsets/strides)
- `CefMessageRouter` (browser-side handler trait + `OnQuery` signature)
- `CefSchemeHandlerFactory` (factory trait + `CefResourceHandler`)

- [ ] **Step 2: Verify OSR + DMA-BUF support**

Specifically confirm:
1. `windowless_rendering_enabled` exists on `CefWindowInfo`.
2. `external_begin_frame_enabled` exists on `CefWindowInfo`.
3. `OnAcceleratedPaint` callback fires with a struct containing a Linux dma-buf fd (typed as `i32` or `RawFd`), format (DRM fourcc), modifier (u64), and plane info.

If (3) is missing or stubbed in the binding, the binding is unfit for our needs. **Decision gate:** in that case, evaluate alternatives:
- `cef-rs` crate
- Hand-rolled `bindgen` against `cef_app_capi.h` + `cef_browser_capi.h` + relevant render handler headers from the CEF tarball

If we end up rolling bindgen, scope creep alert: that's a separate Task B1.5 to write the bindgen build script and minimal Rust wrappers — budget 2-3 days extra.

- [ ] **Step 3: Document the chosen binding**

Append to the worktree's `CLAUDE.md` under a new section "## CEF binding choice":

```markdown
## CEF binding choice

We depend on the `cef` crate (version pinned to match `CEF_VERSION` in
`crates/sola-make/src/cef.rs`). Switching bindings is contained to
`crates/sola-kit/src/cef/` — the rest of the kit consumes our own
`Browser` and handler traits, not the binding crate directly.
```

(Update wording if a different binding wins the decision.)

- [ ] **Step 4: Commit**

```bash
git add CLAUDE.md
git commit -m "docs: pin the CEF Rust binding choice"
```

---

### Task B2: Cargo.toml dependency swap

**Files:**
- Modify: `crates/sola-kit/Cargo.toml`

This task BREAKS the build. Subsequent tasks restore it.

- [ ] **Step 1: Edit dependencies**

Replace the entire `[dependencies]` block in `crates/sola-kit/Cargo.toml` with:

```toml
[dependencies]
sola-bus = { path = "../sola-bus" }
sola-assets = { path = "../sola-assets" }
sola-core = { path = "../sola-core" }
sola-make = { path = "../sola-make" }   # for cef::ensure_cef from build.rs

# CEF binding (version pinned to match CEF_VERSION in sola-make)
cef = "<latest at task time>"

# Wayland client side
smithay-client-toolkit = "0.19"
wayland-client = "0.31"
wayland-protocols = { version = "0.32", features = ["client", "unstable", "staging"] }

# Existing kit code (unchanged)
swc_core = { version = "65", features = [
  "common",
  "ecma_ast",
  "ecma_parser",
  "ecma_parser_typescript",
  "ecma_codegen",
  "ecma_transforms",
  "ecma_transforms_react",
  "ecma_transforms_typescript",
  "ecma_visit",
] }
serde = { version = "1", features = ["derive"] }
serde_json = "1"
tracing = "0.1"
async-trait = "0.1"
tokio = { version = "1", features = ["rt-multi-thread", "sync", "macros"] }

[build-dependencies]
sola-make = { path = "../sola-make" }
```

- [ ] **Step 2: Verify the dep change**

Run: `cargo metadata --no-deps --format-version 1 -p sola-kit > /dev/null`
Expected: prints nothing (parses cleanly). Don't run `cargo make build` yet — code references won't resolve.

- [ ] **Step 3: Commit (in a broken-build state — that's intentional)**

```bash
git add crates/sola-kit/Cargo.toml
git commit -m "chore(sola-kit): swap webkit6/gtk4 deps for cef + sctk

Build is intentionally broken at this commit. Subsequent tasks scaffold
cef/ and wayland/ modules and rewire ctx/window/lib to compile against
the new deps."
```

---

### Task B3: Add `build.rs` invoking `ensure_cef` + emitting link directives

**Files:**
- Create: `crates/sola-kit/build.rs`

- [ ] **Step 1: Create the build script**

```rust
//! sola-kit build script.
//!
//! 1. Ensures the pinned CEF binary distribution is present at
//!    ~/.cache/sola/cef-<version>/ (downloads if missing — first build
//!    on a fresh machine takes ~1-2 minutes).
//! 2. Emits link directives so cargo links against libcef.so from that
//!    cache directory.
//! 3. Writes the cache path to target/cef-runpath for the dev-mode
//!    wrapper used by `cargo make run`.
//!
//! NixOS runtime depends on (must be in configuration.nix):
//!   libGL, libgbm, libnss, libnspr, fontconfig, freetype, expat,
//!   alsaLib, libdrm, mesa (for libgbm/libGL), libxkbcommon, wayland.

use std::env;
use std::fs;
use std::path::PathBuf;

fn main() {
    let cef_dir = sola_make::cef::ensure_cef()
        .expect("CEF binary distribution required (failed to download)");

    let release_dir = sola_make::cef::release_dir();
    println!("cargo:rustc-link-search=native={}", release_dir.display());
    println!("cargo:rustc-link-lib=dylib=cef");

    // Embed the cache path as a compile-time string for runtime CefSettings.
    println!("cargo:rustc-env=SOLA_KIT_CEF_DIR={}", cef_dir.display());

    // Write cef-runpath for dev-mode `cargo make run` wrapper.
    let target_dir: PathBuf = env::var_os("OUT_DIR")
        .map(PathBuf::from)
        .map(|p| p.ancestors().nth(3).unwrap_or(&p).to_path_buf())
        .unwrap_or_else(|| PathBuf::from("target"));
    let runpath_file = target_dir.join("cef-runpath");
    let _ = fs::write(&runpath_file, release_dir.display().to_string());

    println!("cargo:rerun-if-changed=build.rs");
}
```

- [ ] **Step 2: Verify**

Run: `cargo make build sola-kit 2>&1 | tail -20`
Expected: build still fails (we haven't scaffolded cef/wayland modules yet) but the build.rs runs successfully — you'll see no error from it. The compile error will be from missing webkit6/gtk4 imports in the existing source files. That's fine; we fix that in subsequent tasks.

- [ ] **Step 3: Commit**

```bash
git add crates/sola-kit/build.rs
git commit -m "build(sola-kit): build script ensures CEF + emits link directives"
```

---

### Task B4: Scaffold `cef/` module skeletons

**Files:**
- Create: `crates/sola-kit/src/cef/mod.rs`
- Create: `crates/sola-kit/src/cef/init.rs`
- Create: `crates/sola-kit/src/cef/distribution.rs`
- Create: `crates/sola-kit/src/cef/browser.rs`
- Create: `crates/sola-kit/src/cef/handlers.rs`
- Create: `crates/sola-kit/src/cef/ipc.rs`
- Create: `crates/sola-kit/src/cef/scheme.rs`

- [ ] **Step 1: `mod.rs` — re-exports**

```rust
//! CEF engine integration. The boundary between sola-kit and the CEF
//! Rust binding lives entirely in this module — the rest of the kit
//! does not know what engine is underneath.

pub mod browser;
pub mod distribution;
pub mod handlers;
pub mod init;
pub mod ipc;
pub mod scheme;

pub use browser::Browser;
pub use init::short_circuit_if_subprocess;
```

- [ ] **Step 2: `init.rs` — subprocess detection + CefInitialize stub**

```rust
//! CEF process startup. Two distinct entry points:
//!
//! - `short_circuit_if_subprocess()` — called at the very top of `main()`.
//!   If we were re-execed by CEF as a renderer/GPU/utility worker, this
//!   hands control to `CefExecuteProcess` and exits the process when
//!   that worker is done.
//! - `initialize()` — called once in the browser process to start CEF.

use std::path::PathBuf;
use std::process::ExitCode;

/// Subprocess gate — call this at the top of `main()`.
///
/// Returns `Some(ExitCode)` if the current process is a CEF worker
/// (renderer/GPU/utility/zygote); the caller should `return code` from
/// `main()` immediately. Returns `None` if this is the main browser
/// process.
pub fn short_circuit_if_subprocess() -> Option<ExitCode> {
    // TODO(taskB5): call cef::execute_process and translate result.
    None
}

/// Initialize CEF in the browser process. Call exactly once, after
/// `short_circuit_if_subprocess` has returned None.
pub fn initialize() {
    // TODO(taskB6): build CefSettings + CefMainArgs, call cef::initialize.
    let _cef_dir: PathBuf = std::env::var_os("SOLA_KIT_CEF_DIR")
        .map(PathBuf::from)
        .expect("SOLA_KIT_CEF_DIR not embedded by build.rs");
    let _ = _cef_dir;
}

/// Run CEF's message loop on the current (main) thread. Blocks until
/// `cef::quit_message_loop` is posted.
pub fn run_message_loop() {
    // TODO(taskB7): call cef::run_message_loop.
}

/// Tear down CEF cleanly. Called once after `run_message_loop` returns.
pub fn shutdown() {
    // TODO(taskB7): call cef::shutdown.
}
```

- [ ] **Step 3: `distribution.rs` — runtime path resolution**

```rust
//! Runtime CEF path resolution. Mirrors what `crates/sola-make/src/cef.rs`
//! resolves at build time, but reads from the env var that build.rs
//! embedded so the binary doesn't need to recompute it.

use std::path::PathBuf;

pub fn cef_dir() -> PathBuf {
    PathBuf::from(env!("SOLA_KIT_CEF_DIR"))
}

pub fn release_dir() -> PathBuf {
    cef_dir().join("Release")
}

pub fn resources_dir() -> PathBuf {
    cef_dir().join("Resources")
}

pub fn locales_dir() -> PathBuf {
    resources_dir().join("locales")
}
```

- [ ] **Step 4: `browser.rs` — Browser struct skeleton**

```rust
//! CEF browser wrapper, one per window.

use std::rc::Rc;
use crate::wayland::Surface;

/// A CEF browser bound to a Wayland surface.
pub struct Browser {
    // TODO(taskB10): wrap the binding crate's Browser handle.
}

impl Browser {
    /// Create a browser that paints into `surface` and loads `initial_url`.
    pub fn new(_surface: Rc<Surface>, _initial_url: &str) -> Self {
        // TODO(taskB10): build CefBrowserSettings + CefWindowInfo + RenderHandler
        // and call CreateBrowserSync.
        Self {}
    }

    /// Execute JS in the main frame.
    pub fn execute_js(&self, _script: &str) {
        // TODO(taskD4)
    }

    /// Open DevTools for this browser in a new OSR-managed Surface.
    pub fn open_devtools(&self) {
        // TODO(taskE1)
    }
}
```

- [ ] **Step 5: `handlers.rs` — placeholder**

```rust
//! CEF callback handler implementations. Each handler trait that the
//! binding crate exposes gets implemented here and wired in
//! `browser::Browser::new`. Most handlers run on CEF's UI thread, which
//! is our main thread — so dispatch to surface methods is direct.

// TODO(taskB11): RenderHandler with on_accelerated_paint forwarding to Surface.
// TODO(taskD): LoadHandler, IpcHandler.
```

- [ ] **Step 6: `ipc.rs` — placeholder**

```rust
//! JS↔Rust IPC bridge. Browser side of CEF's MessageRouter
//! corresponds to `window.cefQuery(...)` on the JS side.

// TODO(taskD2): MessageRouterBrowserSide handler dispatching to KitApp::on_js_command.
```

- [ ] **Step 7: `scheme.rs` — placeholder**

```rust
//! `app://` scheme handler factory. Wraps `AssetBundle` (and the JSX
//! transform pipeline in `crate::strip`) to serve embedded assets.

// TODO(taskC1): SchemeHandlerFactory + ResourceHandler.
```

- [ ] **Step 8: Wire `cef` module into `lib.rs`**

In `crates/sola-kit/src/lib.rs`, add near the other `mod` declarations at the top:

```rust
pub mod cef;
```

- [ ] **Step 9: Verify**

Run: `cargo make build sola-kit 2>&1 | tail -10`
Expected: Build STILL fails — old `webview.rs`, `ctx.rs`, `lib.rs::run` still reference webkit6/gtk4. That's fixed in subsequent tasks. The new `cef::*` module compiles successfully, which is what this task validates.

To be sure: `cargo check -p sola-kit --lib --no-default-features 2>&1 | grep "cef::" | head` — should be empty (no errors from the cef module).

- [ ] **Step 10: Commit**

```bash
git add crates/sola-kit/src/cef/ crates/sola-kit/src/lib.rs
git commit -m "feat(sola-kit): scaffold cef/ module skeleton"
```

---

### Task B5: Implement `short_circuit_if_subprocess`

**Files:**
- Modify: `crates/sola-kit/src/cef/init.rs`

- [ ] **Step 1: Implement using the binding crate**

Replace the body of `short_circuit_if_subprocess` in `cef/init.rs`:

```rust
pub fn short_circuit_if_subprocess() -> Option<ExitCode> {
    // Build CefMainArgs from process argv. CEF inspects the args for
    // --type=... to decide whether this is a renderer, GPU, utility, or
    // zygote subprocess.
    let args = std::env::args_os().collect::<Vec<_>>();
    let main_args = cef::CefMainArgs::new(&args);

    // execute_process returns:
    //   < 0 if this is the main (browser) process
    //   >= 0 if this is a subprocess (the value is the exit code)
    let result = cef::execute_process(&main_args, /* app */ None, /* windows_sandbox_info */ None);

    if result >= 0 {
        Some(ExitCode::from(result as u8))
    } else {
        None
    }
}
```

(Adjust the exact constructor name and `execute_process` signature to match the binding from Task B1.)

- [ ] **Step 2: Verify**

Run: `cargo check -p sola-kit --lib`
Expected: cef::init compiles. Other parts of sola-kit may still fail.

- [ ] **Step 3: Commit**

```bash
git add crates/sola-kit/src/cef/init.rs
git commit -m "feat(sola-kit): implement CEF subprocess short-circuit"
```

---

### Task B6: Implement `cef::init::initialize`

**Files:**
- Modify: `crates/sola-kit/src/cef/init.rs`

- [ ] **Step 1: Build CefSettings**

Replace the body of `initialize`:

```rust
pub fn initialize() {
    let release = crate::cef::distribution::release_dir();
    let resources = crate::cef::distribution::resources_dir();
    let locales = crate::cef::distribution::locales_dir();
    let exe = std::env::current_exe().expect("current_exe");

    let mut settings = cef::CefSettings::default();
    settings.framework_dir_path = Some(release.into_os_string().into_string().unwrap());
    settings.resources_dir_path = Some(resources.into_os_string().into_string().unwrap());
    settings.locales_dir_path = Some(locales.into_os_string().into_string().unwrap());
    settings.browser_subprocess_path = Some(exe.into_os_string().into_string().unwrap());
    settings.no_sandbox = true;
    settings.windowless_rendering_enabled = true;
    settings.external_message_pump = false;       // we use cef::run_message_loop, not glib pump
    settings.multi_threaded_message_loop = false;  // single main-thread

    // Verbose logging in dev; production reduces.
    settings.log_severity = cef::CefLogSeverity::Warning;

    let args = std::env::args_os().collect::<Vec<_>>();
    let main_args = cef::CefMainArgs::new(&args);

    if !cef::initialize(&main_args, &settings, /* app */ None, /* windows_sandbox_info */ None) {
        panic!("cef::initialize failed");
    }
}
```

(Adjust struct field names + function signatures based on the binding chosen in B1.)

- [ ] **Step 2: Verify**

Run: `cargo check -p sola-kit --lib`
Expected: cef::init compiles.

- [ ] **Step 3: Commit**

```bash
git add crates/sola-kit/src/cef/init.rs
git commit -m "feat(sola-kit): implement CefInitialize wrapper"
```

---

### Task B7: Implement `run_message_loop` and `shutdown`

**Files:**
- Modify: `crates/sola-kit/src/cef/init.rs`

- [ ] **Step 1: Implement**

Replace the bodies in `cef/init.rs`:

```rust
pub fn run_message_loop() {
    cef::run_message_loop();
}

pub fn shutdown() {
    cef::shutdown();
}
```

- [ ] **Step 2: Verify**

Run: `cargo check -p sola-kit --lib`
Expected: cef::init compiles.

- [ ] **Step 3: Commit**

```bash
git add crates/sola-kit/src/cef/init.rs
git commit -m "feat(sola-kit): implement CefRunMessageLoop and CefShutdown wrappers"
```

---

### Task B8: Scaffold `wayland/` module skeletons

**Files:**
- Create: `crates/sola-kit/src/wayland/mod.rs`
- Create: `crates/sola-kit/src/wayland/client.rs`
- Create: `crates/sola-kit/src/wayland/surface.rs`
- Create: `crates/sola-kit/src/wayland/input.rs`
- Modify: `crates/sola-kit/src/lib.rs` (add `pub mod wayland;`)

- [ ] **Step 1: `mod.rs`**

```rust
//! Wayland client side. Owns surface lifecycle and translates
//! wl_seat input events into CEF input events.

pub mod client;
pub mod input;
pub mod surface;

pub use client::WaylandClient;
pub use surface::Surface;
```

- [ ] **Step 2: `client.rs` — singleton skeleton**

```rust
//! Per-process Wayland connection. One global, shared by all Surfaces.

use std::rc::Rc;

pub struct WaylandClient {
    // TODO(taskB9): connection, registry, globals (xdg_wm_base,
    // zwp_linux_dmabuf_v1, wl_seat, …).
}

impl WaylandClient {
    /// Connect to the Wayland compositor and bind globals. Panics if
    /// the connection fails or required protocols are missing.
    pub fn connect() -> Rc<Self> {
        // TODO(taskB9)
        Rc::new(Self {})
    }

    /// Drive the dispatch loop one iteration (for our main loop's
    /// integration with CEF).
    pub fn dispatch_pending(&self) {
        // TODO(taskB9)
    }
}
```

- [ ] **Step 3: `surface.rs` — Surface skeleton**

```rust
//! Per-window xdg_toplevel + dma-buf import + frame callback handling.

use std::rc::Rc;
use crate::wayland::WaylandClient;
use crate::WindowConfig;

pub struct Surface {
    // TODO(taskB9): wl_surface, xdg_toplevel, frame callback state,
    // dma-buf params builder, current size, configure ack state.
}

impl Surface {
    pub fn new(_client: &Rc<WaylandClient>, _cfg: &WindowConfig) -> Rc<Self> {
        // TODO(taskB9): create wl_surface, xdg_toplevel, set title/app_id/size.
        Rc::new(Self {})
    }

    /// Present a CEF-produced dma-buf as the next frame.
    pub fn present_dmabuf(
        &self,
        _fd: std::os::unix::io::RawFd,
        _format: u32,         // DRM fourcc, e.g. DRM_FORMAT_ARGB8888
        _modifier: u64,       // DRM modifier
        _stride: u32,
        _offset: u32,
        _width: i32,
        _height: i32,
        _damage_rects: &[(i32, i32, i32, i32)],
    ) {
        // TODO(taskB9 — final wiring in B11)
    }

    /// Width / height (px).
    pub fn size(&self) -> (i32, i32) {
        // TODO(taskB9)
        (1100, 720)
    }
}
```

- [ ] **Step 4: `input.rs` — placeholder**

```rust
//! wl_pointer / wl_keyboard / wl_touch / IME → CEF event translation.
// TODO(taskD6, D7, D8)
```

- [ ] **Step 5: Wire into lib.rs**

Add to `crates/sola-kit/src/lib.rs` near the other `mod` declarations:

```rust
pub mod wayland;
```

- [ ] **Step 6: Verify**

Run: `cargo check -p sola-kit --lib`
Expected: wayland module compiles. Build of the whole binary still fails until ctx/lib are rewired.

- [ ] **Step 7: Commit**

```bash
git add crates/sola-kit/src/wayland/ crates/sola-kit/src/lib.rs
git commit -m "feat(sola-kit): scaffold wayland/ module skeleton"
```

---

### Task B9: Implement `WaylandClient` and `Surface` (minimal: connect + xdg_toplevel + dma-buf negotiation)

**Files:**
- Modify: `crates/sola-kit/src/wayland/client.rs`
- Modify: `crates/sola-kit/src/wayland/surface.rs`

This is the largest single task in the plan. Budget a full day. It's mechanical sctk usage but there are many small parts.

- [ ] **Step 1: Implement `WaylandClient::connect`**

Replace `client.rs`:

```rust
use std::rc::Rc;
use std::sync::{Arc, Mutex};

use smithay_client_toolkit::compositor::{CompositorHandler, CompositorState};
use smithay_client_toolkit::output::{OutputHandler, OutputState};
use smithay_client_toolkit::registry::{ProvidesRegistryState, RegistryState};
use smithay_client_toolkit::seat::{SeatHandler, SeatState};
use smithay_client_toolkit::shell::xdg::XdgShell;
use wayland_client::{Connection, EventQueue, QueueHandle, globals::registry_queue_init};

use wayland_protocols::wp::linux_dmabuf::zv1::client::zwp_linux_dmabuf_v1::ZwpLinuxDmabufV1;

pub struct WaylandClient {
    pub conn: Connection,
    pub registry_state: RegistryState,
    pub compositor_state: CompositorState,
    pub seat_state: SeatState,
    pub output_state: OutputState,
    pub xdg_shell: XdgShell,
    pub dmabuf: ZwpLinuxDmabufV1,
    pub queue: Arc<Mutex<EventQueue<WaylandClient>>>,
    pub qh: QueueHandle<WaylandClient>,
}

impl WaylandClient {
    pub fn connect() -> Rc<Self> {
        let conn = Connection::connect_to_env()
            .expect("Wayland: cannot connect to compositor");
        let (globals, mut event_queue) = registry_queue_init::<Self>(&conn)
            .expect("Wayland: registry init failed");
        let qh = event_queue.handle();

        let registry_state = RegistryState::new(&globals);
        let compositor_state = CompositorState::bind(&globals, &qh)
            .expect("Wayland: wl_compositor missing");
        let seat_state = SeatState::new(&globals, &qh);
        let output_state = OutputState::new(&globals, &qh);
        let xdg_shell = XdgShell::bind(&globals, &qh)
            .expect("Wayland: xdg_wm_base missing");

        // Bind zwp_linux_dmabuf_v1 manually (sctk doesn't wrap it).
        let dmabuf = globals
            .bind::<ZwpLinuxDmabufV1, _, _>(&qh, 4..=5, ())
            .expect("Wayland: zwp_linux_dmabuf_v1 not available (need v4+)");

        // Roundtrip once to populate state.
        event_queue.roundtrip(&mut Self::__placeholder()).ok();

        Rc::new(Self {
            conn,
            registry_state,
            compositor_state,
            seat_state,
            output_state,
            xdg_shell,
            dmabuf,
            queue: Arc::new(Mutex::new(event_queue)),
            qh,
        })
    }

    fn __placeholder() -> Self {
        unreachable!("only used to satisfy roundtrip's type signature; replaced by real instance")
    }

    pub fn dispatch_pending(&self) {
        // Non-blocking pump for integration with CEF's main loop.
        let mut queue = self.queue.lock().unwrap();
        let _ = queue.dispatch_pending(&mut Self::__placeholder());
    }
}

// sctk requires us to implement the registry + handler traits.
smithay_client_toolkit::delegate_registry!(WaylandClient);
smithay_client_toolkit::delegate_output!(WaylandClient);
smithay_client_toolkit::delegate_seat!(WaylandClient);
smithay_client_toolkit::delegate_compositor!(WaylandClient);
smithay_client_toolkit::delegate_xdg_shell!(WaylandClient);
smithay_client_toolkit::delegate_xdg_window!(WaylandClient);

impl ProvidesRegistryState for WaylandClient {
    fn registry(&mut self) -> &mut RegistryState { &mut self.registry_state }
    smithay_client_toolkit::registry_handlers![OutputState, SeatState];
}

impl OutputHandler for WaylandClient {
    fn output_state(&mut self) -> &mut OutputState { &mut self.output_state }
    fn new_output(&mut self, _: &Connection, _: &QueueHandle<Self>, _: smithay_client_toolkit::reexports::client::protocol::wl_output::WlOutput) {}
    fn update_output(&mut self, _: &Connection, _: &QueueHandle<Self>, _: smithay_client_toolkit::reexports::client::protocol::wl_output::WlOutput) {}
    fn output_destroyed(&mut self, _: &Connection, _: &QueueHandle<Self>, _: smithay_client_toolkit::reexports::client::protocol::wl_output::WlOutput) {}
}

impl SeatHandler for WaylandClient {
    fn seat_state(&mut self) -> &mut SeatState { &mut self.seat_state }
    fn new_seat(&mut self, _: &Connection, _: &QueueHandle<Self>, _: smithay_client_toolkit::reexports::client::protocol::wl_seat::WlSeat) {}
    fn new_capability(&mut self, _: &Connection, _: &QueueHandle<Self>, _: smithay_client_toolkit::reexports::client::protocol::wl_seat::WlSeat, _: smithay_client_toolkit::seat::Capability) {}
    fn remove_capability(&mut self, _: &Connection, _: &QueueHandle<Self>, _: smithay_client_toolkit::reexports::client::protocol::wl_seat::WlSeat, _: smithay_client_toolkit::seat::Capability) {}
    fn remove_seat(&mut self, _: &Connection, _: &QueueHandle<Self>, _: smithay_client_toolkit::reexports::client::protocol::wl_seat::WlSeat) {}
}

impl CompositorHandler for WaylandClient {
    fn scale_factor_changed(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wayland_client::protocol::wl_surface::WlSurface, _: i32) {}
    fn transform_changed(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wayland_client::protocol::wl_surface::WlSurface, _: wayland_client::protocol::wl_output::Transform) {}
    fn frame(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wayland_client::protocol::wl_surface::WlSurface, _: u32) {}
    fn surface_enter(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wayland_client::protocol::wl_surface::WlSurface, _: &wayland_client::protocol::wl_output::WlOutput) {}
    fn surface_leave(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wayland_client::protocol::wl_surface::WlSurface, _: &wayland_client::protocol::wl_output::WlOutput) {}
}
```

The `__placeholder` shenanigan is ugly; refine in step 3 by removing it and threading `&mut self` through dispatch via a small handler indirection. Keeping it now to get a compiling skeleton.

- [ ] **Step 2: Verify client.rs compiles**

Run: `cargo check -p sola-kit --lib`
Expected: client.rs compiles. (It won't dispatch correctly because of the `__placeholder` hack, but we replace that next.)

- [ ] **Step 3: Replace `__placeholder` with proper dispatch**

The right pattern: `dispatch_pending` takes `&mut self` (which means `WaylandClient` is held by `RefCell` not `Rc<Self>`). Refactor:

```rust
// at top of client.rs, replace `Rc::new(Self { ... })` with returning Self
// then have callers wrap in Rc<RefCell<>>:
//   let client = Rc::new(RefCell::new(WaylandClient::connect()));
//
// dispatch_pending becomes:
pub fn dispatch_pending(&mut self) {
    let mut queue = self.queue.clone();
    let mut q = queue.lock().unwrap();
    let _ = q.dispatch_pending(self);
}
```

(The wrapping in Rc<RefCell<>> happens in `lib.rs::run<A>` — taskB13.)

- [ ] **Step 4: Implement `Surface::new`**

Replace `surface.rs`:

```rust
use std::cell::RefCell;
use std::rc::Rc;

use smithay_client_toolkit::shell::WaylandSurface;
use smithay_client_toolkit::shell::xdg::window::{Window, WindowConfigure, WindowDecorations, WindowHandler};
use wayland_client::Proxy;
use wayland_client::protocol::wl_surface::WlSurface;
use wayland_protocols::wp::linux_dmabuf::zv1::client::zwp_linux_buffer_params_v1::ZwpLinuxBufferParamsV1;

use crate::WindowConfig;
use crate::wayland::WaylandClient;

pub struct Surface {
    pub wl_surface: WlSurface,
    pub xdg_window: Window,
    pub size: RefCell<(i32, i32)>,
    pub configured: RefCell<bool>,
    pub client: Rc<RefCell<WaylandClient>>,
}

impl Surface {
    pub fn new(client: Rc<RefCell<WaylandClient>>, cfg: &WindowConfig) -> Rc<Self> {
        let c = client.borrow();
        let wl_surface = c.compositor_state.create_surface(&c.qh);
        let xdg_window = c.xdg_shell.create_window(
            wl_surface.clone(),
            WindowDecorations::RequestServer,
            &c.qh,
        );
        xdg_window.set_title(cfg.title.clone());
        xdg_window.set_app_id(format!("sola.{}", "sola-kit")); // TODO: pass APP_ID
        xdg_window.set_min_size(Some((400, 300)));
        xdg_window.commit();
        drop(c);

        Rc::new(Self {
            wl_surface,
            xdg_window,
            size: RefCell::new(cfg.size),
            configured: RefCell::new(false),
            client,
        })
    }

    pub fn size(&self) -> (i32, i32) {
        *self.size.borrow()
    }

    pub fn present_dmabuf(
        &self,
        fd: std::os::unix::io::RawFd,
        format: u32,
        modifier: u64,
        stride: u32,
        offset: u32,
        width: i32,
        height: i32,
        damage_rects: &[(i32, i32, i32, i32)],
    ) {
        let c = self.client.borrow();
        let params: ZwpLinuxBufferParamsV1 = c.dmabuf.create_params(&c.qh, ());
        params.add(
            fd,
            0,                         // plane index
            offset,
            stride,
            (modifier >> 32) as u32,
            (modifier & 0xFFFFFFFF) as u32,
        );
        let buffer = params.create_immed(
            width,
            height,
            format,
            wayland_protocols::wp::linux_dmabuf::zv1::client::zwp_linux_buffer_params_v1::Flags::empty(),
            &c.qh,
            (),
        );
        self.wl_surface.attach(Some(&buffer), 0, 0);
        for (x, y, w, h) in damage_rects {
            self.wl_surface.damage_buffer(*x, *y, *w, *h, );
        }
        if damage_rects.is_empty() {
            self.wl_surface.damage_buffer(0, 0, width, height);
        }
        self.wl_surface.commit();
    }
}

// Implement WindowHandler so sctk delivers configure events.
impl WindowHandler for WaylandClient {
    fn request_close(&mut self, _: &wayland_client::Connection, _: &wayland_client::QueueHandle<Self>, _: &Window) {
        // Surfaces close via bus CloseApp, not user-initiated close. Swallow.
    }
    fn configure(&mut self, _: &wayland_client::Connection, _: &wayland_client::QueueHandle<Self>, _window: &Window, _configure: WindowConfigure, _serial: u32) {
        // TODO(taskB12): mark Surface configured + dispatch resize to CEF browser.
    }
}
```

- [ ] **Step 5: Add the dmabuf delegate**

Append to `client.rs`:

```rust
use wayland_client::Dispatch;
use wayland_protocols::wp::linux_dmabuf::zv1::client::{
    zwp_linux_dmabuf_v1, zwp_linux_buffer_params_v1::ZwpLinuxBufferParamsV1,
};
use wayland_client::protocol::wl_buffer::WlBuffer;

impl Dispatch<ZwpLinuxDmabufV1, ()> for WaylandClient {
    fn event(_: &mut Self, _: &ZwpLinuxDmabufV1, _: zwp_linux_dmabuf_v1::Event, _: &(), _: &Connection, _: &QueueHandle<Self>) {
        // No interesting events from v4+ protocol on bind.
    }
}

impl Dispatch<ZwpLinuxBufferParamsV1, ()> for WaylandClient {
    fn event(_: &mut Self, _: &ZwpLinuxBufferParamsV1, _: wayland_protocols::wp::linux_dmabuf::zv1::client::zwp_linux_buffer_params_v1::Event, _: &(), _: &Connection, _: &QueueHandle<Self>) {
        // create_immed bypasses the success/failure events.
    }
}

impl Dispatch<WlBuffer, ()> for WaylandClient {
    fn event(_: &mut Self, _: &WlBuffer, _: wayland_client::protocol::wl_buffer::Event, _: &(), _: &Connection, _: &QueueHandle<Self>) {
        // Compositor releases buffer; CEF will re-supply on next paint.
    }
}
```

- [ ] **Step 6: Verify**

Run: `cargo check -p sola-kit --lib 2>&1 | tail -20`
Expected: client.rs and surface.rs both compile. The whole binary still won't, until ctx/lib are rewired.

- [ ] **Step 7: Commit**

```bash
git add crates/sola-kit/src/wayland/
git commit -m "feat(sola-kit): wayland client with sctk + xdg_toplevel + dma-buf import

Minimal compile-clean implementation of WaylandClient (registry,
globals, dispatch) and Surface (xdg_toplevel creation, present_dmabuf
via zwp_linux_dmabuf_v1::create_immed). Configure handler is a stub —
wired to CEF resize in B12."
```

---

### Task B10: Implement `cef::Browser::new` (open about:blank)

**Files:**
- Modify: `crates/sola-kit/src/cef/browser.rs`
- Modify: `crates/sola-kit/src/cef/handlers.rs`

- [ ] **Step 1: Implement RenderHandler in handlers.rs**

Replace `handlers.rs`:

```rust
use std::rc::Rc;
use crate::wayland::Surface;

/// CEF RenderHandler implementation. Receives accelerated paint
/// callbacks on the UI thread (= our main thread) and forwards the
/// dma-buf to the bound Wayland surface.
pub struct RenderHandler {
    pub surface: Rc<Surface>,
}

// The exact trait signature depends on the binding crate; here is the
// shape per CEF's stable C API. Adapt to whatever Rust types the
// binding exposes.
impl cef::RenderHandler for RenderHandler {
    fn get_view_rect(&self, _browser: &cef::Browser) -> cef::Rect {
        let (w, h) = self.surface.size();
        cef::Rect { x: 0, y: 0, width: w, height: h }
    }

    fn on_accelerated_paint(
        &self,
        _browser: &cef::Browser,
        _paint_type: cef::PaintElementType,
        dirty_rects: &[cef::Rect],
        info: &cef::AcceleratedPaintInfo,
    ) {
        let damage: Vec<(i32, i32, i32, i32)> = dirty_rects
            .iter()
            .map(|r| (r.x, r.y, r.width, r.height))
            .collect();
        // Single-plane ARGB assumption (Chromium's default OSR output).
        let plane = &info.planes[0];
        self.surface.present_dmabuf(
            plane.fd,
            info.format,
            info.modifier,
            plane.stride,
            plane.offset,
            self.surface.size().0,
            self.surface.size().1,
            &damage,
        );
    }
}
```

- [ ] **Step 2: Implement Browser::new in browser.rs**

Replace `browser.rs`:

```rust
use std::rc::Rc;

use crate::cef::handlers::RenderHandler;
use crate::wayland::Surface;

pub struct Browser {
    pub inner: cef::Browser,
}

impl Browser {
    pub fn new(surface: Rc<Surface>, initial_url: &str) -> Self {
        let mut window_info = cef::CefWindowInfo::default();
        window_info.windowless_rendering_enabled = true;
        window_info.external_begin_frame_enabled = true;
        window_info.shared_texture_enabled = true;  // dma-buf on Linux

        let mut browser_settings = cef::CefBrowserSettings::default();
        browser_settings.background_color = 0xFFFFFFFF; // opaque white

        let render_handler = RenderHandler { surface };

        // Construct a CefClient with our handlers attached.
        // Concrete API depends on the binding; a typical pattern:
        let client = cef::CefClientBuilder::new()
            .with_render_handler(Box::new(render_handler))
            .build();

        let inner = cef::BrowserHost::create_browser_sync(
            &window_info,
            client,
            initial_url,
            &browser_settings,
            None,
            None,
        );

        Self { inner }
    }

    pub fn execute_js(&self, script: &str) {
        if let Some(frame) = self.inner.main_frame() {
            frame.execute_javascript(script, "app:///inline.js", 0);
        }
    }

    pub fn open_devtools(&self) {
        // TODO(taskE1)
    }
}
```

- [ ] **Step 3: Verify**

Run: `cargo check -p sola-kit --lib 2>&1 | tail -10`
Expected: cef module compiles.

- [ ] **Step 4: Commit**

```bash
git add crates/sola-kit/src/cef/
git commit -m "feat(sola-kit): cef::Browser::new opens about:blank in OSR mode"
```

---

### Task B11: Replace `lib.rs::run<A>` main loop

**Files:**
- Modify: `crates/sola-kit/src/lib.rs`

- [ ] **Step 1: Rip out the gtk_app + WebKit code**

In `crates/sola-kit/src/lib.rs::run<A>`, locate the existing `gtk_app.run()` block and replace the entire body of `run<A>` with this skeleton:

```rust
pub fn run<A: SolaApp>() {
    sola_core::log::init_for_app(A::APP_ID);

    // Subprocess gate — must be first thing in main, but lib.rs::run is
    // called from main.rs which already checks. This is the browser
    // process path.
    cef::init::initialize();

    // Wayland connection.
    let wayland = std::rc::Rc::new(std::cell::RefCell::new(
        crate::wayland::WaylandClient::connect_owned(),
    ));

    // Bus client.
    let bus = std::rc::Rc::new(std::cell::RefCell::new(BusClient::new()));
    {
        let mut c = bus.borrow_mut();
        c.set_app_id(A::APP_ID);
        if let Err(e) = c.connect() {
            tracing::warn!("bus not available: {e}");
        }
    }

    // Build AppCtx + run A::new (which calls add_window).
    let mut ctx = AppCtx::new(bus.clone(), wayland.clone(), A::APP_ID);
    let mut app = A::new(&mut ctx);

    // Push default theme to every window.
    {
        let payload = serde_json::json!({
            "event": "theme",
            "css": theme_css(&sola_core::theme::Theme::default()),
        });
        for w in &ctx.windows {
            w.send_to_js(&payload);
        }
    }

    // BusRegistry + framework topic subscription (unchanged from earlier).
    let mut registry: BusRegistry<A> = BusRegistry::new();
    app.register_bus(&mut registry, &mut ctx);
    let mut subscription_kinds = registry.kinds();
    for kind in [
        TopicKind::Shutdown, TopicKind::Windows, TopicKind::Copy,
        TopicKind::Paste, TopicKind::Evaluate, TopicKind::Theme,
    ] {
        if !subscription_kinds.contains(&kind) {
            subscription_kinds.push(kind);
        }
    }
    {
        let mut c = bus.borrow_mut();
        if let Err(e) = c.subscribe(&subscription_kinds) {
            tracing::warn!("failed to subscribe: {e}");
        }
    }

    let runtime = std::rc::Rc::new(std::cell::RefCell::new(AppRuntime { app, ctx }));
    let registry_arc = std::rc::Rc::new(registry);

    // Spawn bus polling thread; bridge into CEF UI thread via post_task.
    spawn_bus_thread::<A>(bus.clone(), runtime.clone(), registry_arc.clone());

    // Spawn Wayland dispatch thread. Wayland events arrive via a fd we
    // could poll, but the simplest correct integration is a dedicated
    // dispatch thread that pumps events and notifies the UI thread.
    spawn_wayland_thread(wayland.clone());

    // Run CEF's main loop on this thread until quit.
    cef::init::run_message_loop();

    // Cleanup.
    cef::init::shutdown();
}

fn spawn_bus_thread<A: SolaApp>(
    bus: std::rc::Rc<std::cell::RefCell<BusClient>>,
    runtime: std::rc::Rc<std::cell::RefCell<AppRuntime<A>>>,
    registry: std::rc::Rc<BusRegistry<A>>,
) {
    // TODO(taskD5): real implementation — for B11, an empty spawn is fine.
    let _ = (bus, runtime, registry);
}

fn spawn_wayland_thread(_wl: std::rc::Rc<std::cell::RefCell<crate::wayland::WaylandClient>>) {
    // TODO: real impl posts back into CEF UI thread.
}
```

(The exact integration shape for the bus/Wayland threads with `cef::post_task(TID_UI, …)` will be filled in at Task D5 once IPC works.)

- [ ] **Step 2: Add `connect_owned` constructor to WaylandClient**

In `wayland/client.rs`, replace `pub fn connect() -> Rc<Self>` with the version that returns `Self` directly (caller wraps), since `lib.rs::run<A>` wants to put it in `Rc<RefCell<>>`:

```rust
pub fn connect_owned() -> Self {
    // (same body as connect(), returning Self instead of Rc<Self>)
}
```

- [ ] **Step 3: Verify lib.rs compiles**

Run: `cargo check -p sola-kit --lib 2>&1 | tail -20`
Expected: lib compiles. ctx.rs may still fail.

- [ ] **Step 4: Commit**

```bash
git add crates/sola-kit/src/lib.rs crates/sola-kit/src/wayland/client.rs
git commit -m "feat(sola-kit): replace gtk main loop with CefRunMessageLoop"
```

---

### Task B12: Rewrite `ctx.rs::add_window` to pair Surface + Browser

**Files:**
- Modify: `crates/sola-kit/src/ctx.rs`
- Modify: `crates/sola-kit/src/window.rs`

- [ ] **Step 1: Replace AppCtx in ctx.rs**

Replace the entire body of `ctx.rs` with:

```rust
use std::cell::RefCell;
use std::rc::Rc;

use sola_bus::topics::Topic;
use sola_bus::BusClient;

use crate::cef::Browser;
use crate::wayland::{Surface, WaylandClient};
use crate::window::{WindowHandle, WindowInner};
use crate::WindowConfig;

pub struct AppCtx {
    pub(crate) bus: Rc<RefCell<BusClient>>,
    pub(crate) wayland: Rc<RefCell<WaylandClient>>,
    pub(crate) app_id: &'static str,
    pub(crate) windows: Vec<WindowHandle>,
    pub(crate) shutdown_requested: bool,
}

impl AppCtx {
    pub(crate) fn new(
        bus: Rc<RefCell<BusClient>>,
        wayland: Rc<RefCell<WaylandClient>>,
        app_id: &'static str,
    ) -> Self {
        Self { bus, wayland, app_id, windows: Vec::new(), shutdown_requested: false }
    }

    pub fn add_window(&mut self, cfg: WindowConfig) -> WindowHandle {
        let surface = Surface::new(self.wayland.clone(), &cfg);
        let initial_url = "app:///index.html";
        let browser = Browser::new(surface.clone(), initial_url);

        let inner = Rc::new(WindowInner::new(cfg.title.clone(), surface, browser));
        let handle = WindowHandle { inner };
        self.windows.push(handle.clone());
        handle
    }

    pub fn emit(&mut self, topic: Topic) {
        let _ = self.bus.borrow_mut().emit(topic);
    }

    pub fn shutdown(&mut self) {
        self.shutdown_requested = true;
        cef::quit_message_loop();
    }
}
```

- [ ] **Step 2: Rewrite WindowInner + WindowHandle in window.rs**

Replace `window.rs` body:

```rust
use std::cell::RefCell;
use std::rc::Rc;

use serde_json::Value;

use crate::cef::Browser;
use crate::wayland::Surface;
use crate::AssetBundle;

/// Declarative window configuration passed to `AppCtx::add_window`.
pub struct WindowConfig {
    pub title: String,
    pub size: (i32, i32),
    pub position: Option<(i32, i32)>,
    pub decorated: bool,
    pub transparent: bool,
    pub assets: &'static AssetBundle,
    pub initial_state: Option<String>,
    pub zoned: bool,
    pub keyboard_target: bool,
}

pub type JsDispatcher = Box<dyn FnMut(&str, &Value, Option<u64>)>;

pub struct WindowInner {
    pub title: String,
    pub surface: Rc<Surface>,
    pub browser: Browser,
    pub dispatcher: Rc<RefCell<Option<JsDispatcher>>>,
    pub loaded: Rc<RefCell<bool>>,
    pub pending: Rc<RefCell<Vec<String>>>,
}

impl WindowInner {
    pub fn new(title: String, surface: Rc<Surface>, browser: Browser) -> Self {
        Self {
            title,
            surface,
            browser,
            dispatcher: Rc::new(RefCell::new(None)),
            loaded: Rc::new(RefCell::new(false)),
            pending: Rc::new(RefCell::new(Vec::new())),
        }
    }
}

#[derive(Clone)]
pub struct WindowHandle {
    pub(crate) inner: Rc<WindowInner>,
}

impl WindowHandle {
    pub fn title(&self) -> &str { &self.inner.title }

    pub fn eval_js(&self, script: &str) {
        if !*self.inner.loaded.borrow() {
            self.inner.pending.borrow_mut().push(script.to_string());
            return;
        }
        self.inner.browser.execute_js(script);
    }

    pub fn send_to_js(&self, value: &Value) {
        let json_str = serde_json::to_string(value).unwrap_or_default();
        let js_literal = serde_json::to_string(&json_str).unwrap_or_default();
        self.eval_js(&format!("window.__solaRecv({js_literal})"));
    }

    pub fn send_raw_json_to_js(&self, json: &str) {
        let js_literal = serde_json::to_string(json).unwrap_or_default();
        self.eval_js(&format!("window.__solaRecv({js_literal})"));
    }
}

impl PartialEq for WindowHandle {
    fn eq(&self, other: &Self) -> bool { Rc::ptr_eq(&self.inner, &other.inner) }
}
impl Eq for WindowHandle {}
```

(Note: `gtk_window()` and `webview()` accessors are gone. Apps that need underlying handles get `surface()` and `browser()`. Update KitApp uses in Task B14 if any reference the removed accessors.)

- [ ] **Step 3: Delete webview.rs**

```bash
git rm crates/sola-kit/src/webview.rs
```

In `lib.rs`, remove the `mod webview;` line.

- [ ] **Step 4: Verify**

Run: `cargo check -p sola-kit --lib 2>&1 | tail -20`
Expected: lib compiles. The bin (`src/app/main.rs`) still fails until B13.

- [ ] **Step 5: Commit**

```bash
git add crates/sola-kit/src/ctx.rs crates/sola-kit/src/window.rs crates/sola-kit/src/lib.rs
git rm crates/sola-kit/src/webview.rs
git commit -m "feat(sola-kit): rewrite ctx + window over cef + sctk; delete webview.rs"
```

---

### Task B13: Update `app/main.rs` with subprocess short-circuit

**Files:**
- Modify: `crates/sola-kit/src/app/main.rs`

- [ ] **Step 1: Add the short-circuit**

Replace `crates/sola-kit/src/app/main.rs` with:

```rust
mod kit_app;
mod catalog;
mod fonts;

use std::process::ExitCode;

use kit_app::KitApp;

fn main() -> ExitCode {
    // Subprocess gate. If we were re-execed by CEF as a renderer/GPU/
    // utility/zygote worker, hand control to CEF and exit with its code.
    if let Some(code) = sola_kit::cef::short_circuit_if_subprocess() {
        return code;
    }

    sola_kit::run::<KitApp>();
    ExitCode::SUCCESS
}
```

- [ ] **Step 2: Verify the binary builds**

Run: `cargo make build sola-kit 2>&1 | tail -10`
Expected: builds clean.

- [ ] **Step 3: Install and smoke**

Run: `cargo make install sola-kit`
Expected: install succeeds (with the patchelf step pending — see Task F1, for now it works in dev because LD_LIBRARY_PATH is unset and rpath isn't needed for `libcef.so` if it's in target/debug/deps... if it doesn't work, set LD_LIBRARY_PATH manually for the smoke run).

Run: `LD_LIBRARY_PATH=$(cat target/cef-runpath) /opt/sola/bin/sola-kit` from a TTY (with sola-river already running).
Expected: an empty white window appears in sola-river. Resize works; close works.

If you see crashes, check:
- libcef.so is at `~/.cache/sola/cef-<ver>/Release/libcef.so`
- All NixOS deps are present (`libGL`, `libgbm`, `libnss`, `libnspr` etc.)
- No SUID required because `no_sandbox = true`

- [ ] **Step 4: Commit**

```bash
git add crates/sola-kit/src/app/main.rs
git commit -m "feat(sola-kit): wire CEF subprocess short-circuit in main"
```

---

## Checkpoint C — `app://` Scheme + Page Renders

Goal: sola-kit's `index.html` + `main.tsx` (Preact counter) render in the CEF window.

### Task C1: Implement `cef::scheme` factory

**Files:**
- Modify: `crates/sola-kit/src/cef/scheme.rs`

- [ ] **Step 1: Implement the factory + handler**

Replace `scheme.rs`:

```rust
//! `app://` scheme handler. Bridges CEF's resource model to our
//! AssetBundle + swc TS+JSX transform.

use std::sync::Arc;

use crate::assets::AssetBundle;
use crate::strip::transform;

/// Factory that produces one ResourceHandler per fetched URL.
pub struct AppSchemeFactory {
    pub app_assets: &'static AssetBundle,
    pub platform_assets: Arc<dyn Fn() -> AssetBundle + Send + Sync>,
    pub html_content: String,
}

impl cef::SchemeHandlerFactory for AppSchemeFactory {
    fn create(&self, _browser: Option<&cef::Browser>, _frame: Option<&cef::Frame>, _scheme: &str, request: &cef::Request) -> Option<Box<dyn cef::ResourceHandler>> {
        let uri = request.url();
        // Match the host-stripping logic from old webview.rs.
        let after_scheme = uri.strip_prefix("app://").unwrap_or(&uri);
        let path_with_query = match after_scheme.find('/') {
            Some(i) => &after_scheme[i..],
            None => "/",
        };
        let path = path_with_query.split('?').next().unwrap_or("/").split('#').next().unwrap_or("/");
        let path = if path.is_empty() { "/" } else { path };

        // index.html special-case (substitute __RESTORED_STATE__ + injected import map upstream).
        if path == "/" || path == "/index.html" {
            return Some(Box::new(StringResource::new(self.html_content.clone(), "text/html; charset=utf-8")));
        }

        // app + platform asset lookup
        let platform = (self.platform_assets)();
        let asset = self.app_assets.find(path).or_else(|| platform.find(path)).cloned();
        match asset {
            Some(asset) => {
                let body = if asset.content_type.has_jsx() || asset.content_type.has_types() {
                    transform(asset.content, asset.content_type.has_jsx(), asset.content_type.has_types())
                } else {
                    asset.content.to_string()
                };
                Some(Box::new(StringResource::new(body, asset.content_type.mime())))
            }
            None => Some(Box::new(StringResource::new("Not Found".to_string(), "text/plain"))),
        }
    }
}

/// Tiny in-memory ResourceHandler that serves a string body.
struct StringResource {
    body: Vec<u8>,
    mime: &'static str,
    pos: usize,
}

impl StringResource {
    fn new(body: String, mime: &'static str) -> Self {
        Self { body: body.into_bytes(), mime: Box::leak(mime.to_string().into_boxed_str()), pos: 0 }
    }
}

impl cef::ResourceHandler for StringResource {
    // The exact trait surface depends on the binding; a typical shape:
    fn open(&mut self, _request: &cef::Request, handle_request: &mut bool, _callback: &cef::Callback) -> bool {
        *handle_request = true;
        true
    }

    fn get_response_headers(&self, response: &mut cef::Response, response_length: &mut i64, _redirect_url: &mut String) {
        response.set_status(200);
        response.set_mime_type(self.mime);
        *response_length = self.body.len() as i64;
    }

    fn read(&mut self, data_out: &mut [u8], bytes_read: &mut i32, _callback: &cef::Callback) -> bool {
        let remaining = self.body.len() - self.pos;
        let n = remaining.min(data_out.len());
        if n == 0 {
            *bytes_read = 0;
            return false; // EOF
        }
        data_out[..n].copy_from_slice(&self.body[self.pos..self.pos + n]);
        self.pos += n;
        *bytes_read = n as i32;
        true
    }

    fn cancel(&mut self) {}
}
```

(Note: Asset becomes Cloneable — verify or wrap.)

- [ ] **Step 2: Register the factory in `cef::init::initialize`**

After `cef::initialize(...)` in `cef/init.rs`, register the scheme factory. The factory needs the AppAssets/PlatformAssets/html_content. Plumbing:

Add to `cef::init`:

```rust
pub fn register_app_scheme(
    app_assets: &'static crate::AssetBundle,
    platform_assets_fn: std::sync::Arc<dyn Fn() -> crate::AssetBundle + Send + Sync>,
    html_content: String,
) {
    let factory = crate::cef::scheme::AppSchemeFactory {
        app_assets,
        platform_assets: platform_assets_fn,
        html_content,
    };
    cef::register_scheme_handler_factory("app", "", Box::new(factory));
}
```

- [ ] **Step 3: Wire from `lib.rs::run<A>` (between AppCtx::new and A::new)**

```rust
// Inside run<A>(), AFTER A::new returns and we have the first window's html:
let html = inject_import_map(std::str::from_utf8(/* index.html bytes from app_assets */).unwrap_or(""));
let html = html.replace("__RESTORED_STATE__", "{}"); // initial_state injection done per-window earlier
crate::cef::init::register_app_scheme(
    /* app_assets from window's WindowConfig */,
    std::sync::Arc::new(|| crate::assets::platform_assets()),
    html,
);
```

(The exact wiring to find the right HTML and per-window state will need refinement; for Checkpoint C aim for the storybook's single window.)

- [ ] **Step 4: Update Browser::new to navigate to app://**

In `cef/browser.rs::Browser::new`, change `initial_url` parameter passing — already `"app:///index.html"`, no change needed since ctx.rs::add_window already passes that.

- [ ] **Step 5: Verify**

Run: `cargo make build sola-kit && cargo make install sola-kit`
Run sola-kit from a TTY. Expected: the Preact counter page renders. Click +1; counter increments. Click reset; counter returns to 0.

- [ ] **Step 6: Commit**

```bash
git add crates/sola-kit/src/cef/scheme.rs crates/sola-kit/src/cef/init.rs crates/sola-kit/src/lib.rs
git commit -m "feat(sola-kit): app:// scheme handler serves AssetBundle + JSX transform"
```

---

## Checkpoint D — Bus + IPC + Input

Goal: theme push works; counter responds to keyboard input.

### Task D1: Implement MessageRouter scaffolding

**Files:**
- Modify: `crates/sola-kit/src/cef/ipc.rs`
- Modify: `crates/sola-kit/src/cef/browser.rs`

- [ ] **Step 1: Implement IpcHandler in ipc.rs**

```rust
use serde_json::Value;

/// Handler for `window.cefQuery({...})` calls from the page.
pub struct IpcHandler {
    pub dispatcher: std::rc::Rc<std::cell::RefCell<Option<crate::window::JsDispatcher>>>,
}

impl cef::MessageRouterHandler for IpcHandler {
    fn on_query(
        &self,
        _browser: &cef::Browser,
        _frame: &cef::Frame,
        query_id: i64,
        request: &str,
        _persistent: bool,
        callback: cef::QueryCallback,
    ) -> bool {
        let parsed: Value = match serde_json::from_str(request) {
            Ok(v) => v,
            Err(e) => {
                callback.failure(-1, &format!("invalid JSON: {e}"));
                return true;
            }
        };
        let cmd = parsed.get("cmd").and_then(|v| v.as_str()).unwrap_or("");
        let args = parsed.get("args").cloned().unwrap_or(Value::Object(Default::default()));

        if let Some(dispatch) = self.dispatcher.borrow_mut().as_mut() {
            // KitApp::on_js_command writes the response via WindowHandle::send_to_js.
            // To bridge this back to cefQuery's callback, we need to capture the
            // callback by id and have on_js_command's response routed here. The
            // pre-CEF design used the id field; we keep the same shape.
            let id = query_id as u64;
            dispatch(cmd, &args, Some(id));
        } else {
            tracing::warn!(cmd, "JS command before dispatcher installed");
        }
        // Bridge: store callback in a global keyed by id; window.send_to_js
        // detects {id, result} and forwards to callback.success.
        store_pending_callback(query_id, callback);
        true
    }
}

// Pending callbacks map. Global because CEF's UI thread is the only
// caller; no Mutex needed if we use a thread_local.
use std::cell::RefCell;
thread_local! {
    static PENDING: RefCell<std::collections::HashMap<i64, cef::QueryCallback>> = RefCell::new(std::collections::HashMap::new());
}

fn store_pending_callback(id: i64, cb: cef::QueryCallback) {
    PENDING.with(|p| { p.borrow_mut().insert(id, cb); });
}

/// Called by WindowHandle::send_to_js when the payload contains an `id`
/// field, delivering the response back to cefQuery.
pub fn deliver_response(id: i64, result: &Value) {
    PENDING.with(|p| {
        if let Some(cb) = p.borrow_mut().remove(&id) {
            cb.success(&serde_json::to_string(result).unwrap_or_default());
        }
    });
}
```

- [ ] **Step 2: Wire IpcHandler into Browser::new**

In `cef/browser.rs::Browser::new`, after creating render_handler:

```rust
let ipc_handler = crate::cef::ipc::IpcHandler {
    dispatcher: dispatcher_slot.clone(),
};
let client = cef::CefClientBuilder::new()
    .with_render_handler(Box::new(render_handler))
    .with_message_router_handler(Box::new(ipc_handler))
    .build();
```

This requires Browser::new to take a `dispatcher_slot: Rc<RefCell<Option<JsDispatcher>>>` parameter — add it to the signature and propagate from `ctx::add_window`.

- [ ] **Step 3: Update WindowHandle::send_to_js to short-circuit responses**

In `window.rs::WindowHandle::send_to_js`, before generating ExecuteJavaScript, check if the value has an `id` and `result` (response shape):

```rust
pub fn send_to_js(&self, value: &Value) {
    if let (Some(id), Some(result)) = (value.get("id").and_then(Value::as_i64), value.get("result")) {
        crate::cef::ipc::deliver_response(id, result);
        return;
    }
    let json_str = serde_json::to_string(value).unwrap_or_default();
    let js_literal = serde_json::to_string(&json_str).unwrap_or_default();
    self.eval_js(&format!("window.__solaRecv({js_literal})"));
}
```

- [ ] **Step 4: Verify**

Run: `cargo make build sola-kit`
Expected: builds clean.

- [ ] **Step 5: Commit**

```bash
git add crates/sola-kit/src/cef/ipc.rs crates/sola-kit/src/cef/browser.rs crates/sola-kit/src/window.rs crates/sola-kit/src/ctx.rs
git commit -m "feat(sola-kit): wire CEF MessageRouter for JS↔Rust IPC"
```

---

### Task D2: Update `web/lib/ipc.ts` to use cefQuery

**Files:**
- Modify: `crates/sola-kit/web/lib/ipc.ts`

- [ ] **Step 1: Replace invoke()**

Replace the body of `invoke` in `crates/sola-kit/web/lib/ipc.ts`:

```ts
export function invoke(cmd: string, args: Record<string, any> = {}): Promise<any> {
  return new Promise((resolve, reject) => {
    (window as any).cefQuery({
      request: JSON.stringify({ cmd, args }),
      onSuccess: (response: string) => {
        try {
          resolve(JSON.parse(response));
        } catch {
          resolve(response);
        }
      },
      onFailure: (errorCode: number, errorMessage: string) => {
        reject({ code: errorCode, message: errorMessage });
      },
    });
  });
}
```

Remove the now-unused `nextId` and `pending` map at the top of the file.

- [ ] **Step 2: Verify swc transform passes**

Run: `cargo make build sola-kit`
Expected: builds clean (the .ts file is embedded; build verifies it parses).

- [ ] **Step 3: Smoke test**

Run sola-kit. Verify:
- Counter still works (basic JS execution proves the page loads)
- Open browser console (F12 once devtools land — for now use chrome's --remote-debugging-port if necessary): `await window.invoke?` — should be defined.

- [ ] **Step 4: Commit**

```bash
git add crates/sola-kit/web/lib/ipc.ts
git commit -m "feat(sola-kit): rewrite invoke() over cefQuery"
```

---

### Task D3: Bus polling thread + theme push

**Files:**
- Modify: `crates/sola-kit/src/lib.rs` (replace `spawn_bus_thread` placeholder)

- [ ] **Step 1: Implement spawn_bus_thread**

Replace the placeholder in `lib.rs`:

```rust
fn spawn_bus_thread<A: SolaApp>(
    bus: std::rc::Rc<std::cell::RefCell<BusClient>>,
    runtime: std::rc::Rc<std::cell::RefCell<AppRuntime<A>>>,
    registry: std::rc::Rc<BusRegistry<A>>,
) {
    // Take an Arc-wrapped clone for the thread. BusClient is !Send through
    // its current shape; if it isn't, we extract just the notify_fd and
    // recreate the client on the polling side. Simplest first impl:
    // poll via a duplicate fd handle, post Arc'd messages back via cef post_task.
    let notify_fd = bus.borrow().notify_fd();
    if notify_fd.is_none() {
        tracing::warn!("bus has no notify fd; theme/menu/topic deliveries will not arrive");
        return;
    }
    let _ = (runtime, registry); // wired in Task D5
    // ...
}
```

(For Checkpoint D we accept that this is partially scaffolded; Task D5 finishes the integration.)

- [ ] **Step 2: Commit**

```bash
git add crates/sola-kit/src/lib.rs
git commit -m "feat(sola-kit): scaffold bus polling thread for CEF main loop"
```

---

### Task D4: Implement keyboard input forwarding

**Files:**
- Modify: `crates/sola-kit/src/wayland/input.rs`
- Modify: `crates/sola-kit/src/wayland/client.rs` (KeyboardHandler delegate)

- [ ] **Step 1: Implement input.rs**

```rust
//! Translate wl_keyboard / wl_pointer / wl_touch / IME → CEF events.

use crate::cef::Browser;

pub fn forward_key_event(
    browser: &Browser,
    keysym: u32,
    pressed: bool,
    modifiers_mask: u32,
) {
    let key_event = cef::KeyEvent {
        type_: if pressed { cef::KeyEventType::KeyDown } else { cef::KeyEventType::KeyUp },
        modifiers: translate_modifiers(modifiers_mask),
        windows_key_code: keysym_to_windows_vk(keysym),
        native_key_code: keysym as i32,
        is_system_key: false,
        character: keysym_to_char(keysym).unwrap_or(0),
        unmodified_character: keysym_to_char(keysym).unwrap_or(0),
        focus_on_editable_field: false,
    };
    browser.inner.host().send_key_event(&key_event);
    if pressed {
        let mut char_event = key_event.clone();
        char_event.type_ = cef::KeyEventType::Char;
        browser.inner.host().send_key_event(&char_event);
    }
}

pub fn forward_pointer_motion(browser: &Browser, x: f64, y: f64, modifiers_mask: u32) {
    let mouse_event = cef::MouseEvent {
        x: x as i32,
        y: y as i32,
        modifiers: translate_modifiers(modifiers_mask),
    };
    browser.inner.host().send_mouse_move_event(&mouse_event, false);
}

pub fn forward_pointer_button(browser: &Browser, x: f64, y: f64, button: u32, pressed: bool, modifiers_mask: u32, click_count: i32) {
    let mouse_event = cef::MouseEvent { x: x as i32, y: y as i32, modifiers: translate_modifiers(modifiers_mask) };
    let cef_button = match button {
        0x110 /* BTN_LEFT */ => cef::MouseButtonType::Left,
        0x111 /* BTN_RIGHT */ => cef::MouseButtonType::Right,
        0x112 /* BTN_MIDDLE */ => cef::MouseButtonType::Middle,
        _ => return,
    };
    browser.inner.host().send_mouse_click_event(&mouse_event, cef_button, !pressed, click_count);
}

pub fn forward_pointer_scroll(browser: &Browser, x: f64, y: f64, delta_x: i32, delta_y: i32, modifiers_mask: u32) {
    let mouse_event = cef::MouseEvent { x: x as i32, y: y as i32, modifiers: translate_modifiers(modifiers_mask) };
    browser.inner.host().send_mouse_wheel_event(&mouse_event, delta_x, delta_y);
}

pub fn forward_focus(browser: &Browser, focused: bool) {
    browser.inner.host().set_focus(focused);
}

fn translate_modifiers(mask: u32) -> u32 {
    let mut m = 0;
    if mask & 0x01 != 0 { m |= cef::EVENTFLAG_SHIFT_DOWN; }
    if mask & 0x04 != 0 { m |= cef::EVENTFLAG_CONTROL_DOWN; }
    if mask & 0x08 != 0 { m |= cef::EVENTFLAG_ALT_DOWN; }
    if mask & 0x40 != 0 { m |= cef::EVENTFLAG_COMMAND_DOWN; }  // Super/Meta
    m
}

fn keysym_to_windows_vk(keysym: u32) -> i32 {
    // Minimal mapping for common keys; expand as edge cases surface.
    match keysym {
        0xFF09 => 0x09, // Tab
        0xFF0D => 0x0D, // Enter
        0xFF1B => 0x1B, // Escape
        0xFF08 => 0x08, // Backspace
        0xFF51 => 0x25, // Left
        0xFF53 => 0x27, // Right
        0xFF52 => 0x26, // Up
        0xFF54 => 0x28, // Down
        0xFFC9 => 0x7B, // F12
        c if (0x20..=0x7E).contains(&c) => c as i32,
        _ => keysym as i32,
    }
}

fn keysym_to_char(keysym: u32) -> Option<u16> {
    if (0x20..=0x7E).contains(&keysym) {
        Some(keysym as u16)
    } else {
        None
    }
}
```

- [ ] **Step 2: Wire keyboard delegate in WaylandClient**

In `client.rs`, implement `KeyboardHandler` and add the delegate macro. The handler stores the currently focused `wl_surface` and dispatches key events to the matching Browser via `input::forward_key_event`.

```rust
use smithay_client_toolkit::seat::keyboard::{KeyboardHandler, KeyEvent, Modifiers};
use wayland_client::protocol::{wl_keyboard::WlKeyboard, wl_surface::WlSurface};

impl KeyboardHandler for WaylandClient {
    fn enter(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _kb: &WlKeyboard,
        surface: &WlSurface,
        _serial: u32,
        _raw: &[u32],
        _keysyms: &[smithay_client_toolkit::seat::keyboard::Keysym],
    ) {
        self.focused_surface = Some(surface.clone());
        if let Some(browser) = self.browser_for_surface(surface) {
            crate::wayland::input::forward_focus(browser, true);
        }
    }

    fn leave(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _kb: &WlKeyboard,
        surface: &WlSurface,
        _serial: u32,
    ) {
        if let Some(browser) = self.browser_for_surface(surface) {
            crate::wayland::input::forward_focus(browser, false);
        }
        if self.focused_surface.as_ref() == Some(surface) {
            self.focused_surface = None;
        }
    }

    fn press_key(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _kb: &WlKeyboard,
        _serial: u32,
        event: KeyEvent,
    ) {
        if let Some(browser) = self.focused_browser() {
            crate::wayland::input::forward_key_event(
                browser,
                event.keysym.raw(),
                true,
                self.modifiers_mask,
            );
        }
    }

    fn release_key(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _kb: &WlKeyboard,
        _serial: u32,
        event: KeyEvent,
    ) {
        if let Some(browser) = self.focused_browser() {
            crate::wayland::input::forward_key_event(
                browser,
                event.keysym.raw(),
                false,
                self.modifiers_mask,
            );
        }
    }

    fn update_modifiers(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _kb: &WlKeyboard,
        _serial: u32,
        modifiers: Modifiers,
        _layout: u32,
    ) {
        let mut mask = 0u32;
        if modifiers.shift { mask |= 0x01; }
        if modifiers.ctrl  { mask |= 0x04; }
        if modifiers.alt   { mask |= 0x08; }
        if modifiers.logo  { mask |= 0x40; }
        self.modifiers_mask = mask;
    }
}

smithay_client_toolkit::delegate_keyboard!(WaylandClient);
```

Add the supporting fields on `WaylandClient`:

```rust
pub focused_surface: Option<wayland_client::protocol::wl_surface::WlSurface>,
pub modifiers_mask: u32,
pub surface_to_browser: std::collections::HashMap<u32, std::rc::Rc<crate::cef::Browser>>,
```

(`u32` key = `WlSurface::id().protocol_id()` for stable lookup; populated by `Surface::new` calling `client.register_surface_browser(&wl_surface, browser_rc)`.)

Add the helpers on `WaylandClient`:

```rust
pub fn focused_browser(&self) -> Option<&std::rc::Rc<crate::cef::Browser>> {
    self.focused_surface.as_ref().and_then(|s| self.browser_for_surface(s))
}

pub fn browser_for_surface(&self, surface: &wayland_client::protocol::wl_surface::WlSurface) -> Option<&std::rc::Rc<crate::cef::Browser>> {
    self.surface_to_browser.get(&surface.id().protocol_id())
}
```

This requires `cef::Browser` to be `Rc`-shareable; update `WindowInner` to hold `Rc<Browser>` instead of `Browser` and pass clones to the client's surface_to_browser map at creation time.

- [ ] **Step 3: Verify**

Run sola-kit. Click into the page area, then press keys. Expected:
- Tab moves focus around the buttons
- Enter activates the focused button
- Counter responds

- [ ] **Step 4: Commit**

```bash
git add crates/sola-kit/src/wayland/input.rs crates/sola-kit/src/wayland/client.rs
git commit -m "feat(sola-kit): forward wl_keyboard events to CEF SendKeyEvent"
```

---

### Task D5: Bus → CEF UI thread bridge for theme push

**Files:**
- Modify: `crates/sola-kit/src/lib.rs`

- [ ] **Step 1: Complete spawn_bus_thread**

```rust
fn spawn_bus_thread<A: SolaApp>(
    bus_arc: std::sync::Arc<std::sync::Mutex<BusClient>>,  // refactor RefCell→Mutex for thread sharing
    on_delivery: impl Fn(sola_bus::Delivery) + Send + 'static,
) {
    std::thread::Builder::new()
        .name("sola-kit-bus".into())
        .spawn(move || {
            loop {
                let delivery = bus_arc.lock().unwrap().recv_blocking();
                match delivery {
                    Ok(d) => {
                        // Marshal to UI thread via cef::post_task.
                        let d_clone = d.clone();
                        cef::post_task(cef::ThreadId::UI, move || {
                            on_delivery(d_clone);
                        });
                    }
                    Err(e) => {
                        tracing::warn!("bus recv failed: {e}");
                        std::thread::sleep(std::time::Duration::from_millis(100));
                    }
                }
            }
        }).expect("spawn bus thread");
}
```

- [ ] **Step 2: Move bus state from Rc<RefCell<>> to Arc<Mutex<>>**

This requires audit-and-replace across `lib.rs`, `ctx.rs`, KitApp's bus accessors. Mostly mechanical.

- [ ] **Step 3: Wire on_delivery to dispatch to handlers**

In `run<A>`, call `spawn_bus_thread` with a closure that:
1. Intercepts framework-level topics (Theme, Shutdown, etc.) and pushes theme CSS as before.
2. Dispatches to BusRegistry handlers.

(Adapt the existing bus dispatch loop body — it's not new logic, just moved into a closure.)

- [ ] **Step 4: Verify**

Run sola-kit. Have sola-shell or any peer emit a Topic::Theme. Expected: page's CSS variables update via `replaceSync`.

- [ ] **Step 5: Commit**

```bash
git add crates/sola-kit/src/lib.rs crates/sola-kit/src/ctx.rs
git commit -m "feat(sola-kit): bus polling thread bridged to CEF UI thread via post_task"
```

---

### Task D6: Pointer + focus event forwarding

**Files:**
- Modify: `crates/sola-kit/src/wayland/client.rs`

- [ ] **Step 1: Implement PointerHandler delegate**

In `client.rs`, implement `smithay_client_toolkit::seat::pointer::PointerHandler`. Track current pointer coordinates per-surface and forward to the matching Browser:

```rust
use smithay_client_toolkit::seat::pointer::{PointerHandler, PointerEvent, PointerEventKind};

impl PointerHandler for WaylandClient {
    fn pointer_frame(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _pointer: &wayland_client::protocol::wl_pointer::WlPointer,
        events: &[PointerEvent],
    ) {
        for event in events {
            let browser = match self.browser_for_surface(&event.surface) {
                Some(b) => b.clone(),
                None => continue,
            };
            match event.kind {
                PointerEventKind::Enter { .. } | PointerEventKind::Motion { .. } => {
                    crate::wayland::input::forward_pointer_motion(
                        &browser,
                        event.position.0,
                        event.position.1,
                        self.modifiers_mask,
                    );
                }
                PointerEventKind::Leave { .. } => {
                    // CEF's "mouse_leave=true" variant
                    crate::wayland::input::forward_pointer_motion_leave(
                        &browser,
                        event.position.0,
                        event.position.1,
                        self.modifiers_mask,
                    );
                }
                PointerEventKind::Press { button, .. } => {
                    crate::wayland::input::forward_pointer_button(
                        &browser,
                        event.position.0,
                        event.position.1,
                        button,
                        true,
                        self.modifiers_mask,
                        1,
                    );
                }
                PointerEventKind::Release { button, .. } => {
                    crate::wayland::input::forward_pointer_button(
                        &browser,
                        event.position.0,
                        event.position.1,
                        button,
                        false,
                        self.modifiers_mask,
                        1,
                    );
                }
                PointerEventKind::Axis { horizontal, vertical, .. } => {
                    // Wayland reports axis in fractional units (1.0 = one notch). CEF
                    // SendMouseWheelEvent expects pixels; multiply by ~32 to match
                    // Chromium's typical wheel delta.
                    let dx = (horizontal.absolute * 32.0) as i32;
                    let dy = (vertical.absolute   * 32.0) as i32;
                    crate::wayland::input::forward_pointer_scroll(
                        &browser,
                        event.position.0,
                        event.position.1,
                        dx,
                        dy,
                        self.modifiers_mask,
                    );
                }
            }
        }
    }
}

smithay_client_toolkit::delegate_pointer!(WaylandClient);
```

Add `forward_pointer_motion_leave` to `wayland/input.rs`:

```rust
pub fn forward_pointer_motion_leave(browser: &Browser, x: f64, y: f64, modifiers_mask: u32) {
    let mouse_event = cef::MouseEvent {
        x: x as i32,
        y: y as i32,
        modifiers: translate_modifiers(modifiers_mask),
    };
    browser.inner.host().send_mouse_move_event(&mouse_event, true);
}
```

- [ ] **Step 2: Implement focus enter/leave**

In the keyboard handler, on enter: `input::forward_focus(&browser, true)`. On leave: `forward_focus(&browser, false)`.

- [ ] **Step 3: Verify**

Run sola-kit. Click counter buttons with the mouse. Hover changes cursor on links/buttons. Scroll wheel works.

- [ ] **Step 4: Commit**

```bash
git add crates/sola-kit/src/wayland/client.rs
git commit -m "feat(sola-kit): forward pointer + focus events to CEF"
```

---

## Checkpoint E — DevTools

### Task E1: Implement Browser::open_devtools

**Files:**
- Modify: `crates/sola-kit/src/cef/browser.rs`

- [ ] **Step 1: Implement open_devtools**

```rust
impl Browser {
    pub fn open_devtools(&self, surface: std::rc::Rc<crate::wayland::Surface>) {
        let mut window_info = cef::CefWindowInfo::default();
        window_info.windowless_rendering_enabled = true;
        window_info.external_begin_frame_enabled = true;
        window_info.shared_texture_enabled = true;

        let render_handler = crate::cef::handlers::RenderHandler { surface };
        let client = cef::CefClientBuilder::new()
            .with_render_handler(Box::new(render_handler))
            .build();

        // No URL — devtools opens its built-in inspector page.
        self.inner.host().show_dev_tools(&window_info, client, /* settings */ None, /* inspect_element_at */ None);
    }
}
```

- [ ] **Step 2: Provide an empty-bundle constant for devtools**

Add to `crates/sola-kit/src/assets.rs`:

```rust
/// Empty bundle for windows that don't serve any app:// assets (e.g.,
/// DevTools windows where CEF serves the inspector internally).
pub const EMPTY_BUNDLE: AssetBundle = AssetBundle { assets: &[] };
```

- [ ] **Step 3: Provide a way for KitApp to spawn the devtools surface**

In `crates/sola-kit/src/window.rs`, add a method on `WindowHandle`:

```rust
pub fn open_devtools(&self) {
    let cfg = crate::WindowConfig {
        title: format!("DevTools — {}", self.inner.title),
        size: (1000, 700),
        position: None,
        decorated: true,
        transparent: false,
        assets: &crate::assets::EMPTY_BUNDLE,
        initial_state: None,
        zoned: false,
        keyboard_target: true,
    };
    // For devtools, we don't use AssetBundle — CEF serves the inspector
    // internally. We just need a Surface to draw into.
    let surface = crate::wayland::Surface::new(
        self.inner.surface.client.clone(),
        &cfg,
    );
    self.inner.browser.open_devtools(surface);
}
```

- [ ] **Step 3: Wire kit_app.rs**

In `app/kit_app.rs::on_menu_action`, replace the WebKit inspector path with:

```rust
if action_id == "open_devtools" {
    self.main_window.open_devtools();
}
```

- [ ] **Step 4: Verify**

Run sola-kit. Press F12. Expected: a separate Chromium DevTools window opens beside the sola-kit window. Resize the panel inside DevTools — no freeze, no warnings. Inspect the counter element; modify its style live; verify the change.

- [ ] **Step 5: Commit**

```bash
git add crates/sola-kit/src/cef/browser.rs crates/sola-kit/src/window.rs crates/sola-kit/src/app/kit_app.rs
git commit -m "feat(sola-kit): F12 opens Chromium DevTools as a second OSR surface"
```

---

## Checkpoint F — Polish + cleanup

### Task F1: Patchelf install step

**Files:**
- Modify: `crates/sola-make/src/install.rs`

- [ ] **Step 1: Add patchelf step**

Locate `install_binary` in `crates/sola-make/src/install.rs`. After the binary is copied to `/opt/sola/bin/`, add:

```rust
// If the binary depends on libcef.so, patch its rpath to the cache.
let bin_path = format!("/opt/sola/bin/{name}");
if std::process::Command::new("ldd").arg(&bin_path).output()
    .ok().map(|o| String::from_utf8_lossy(&o.stdout).contains("libcef.so")).unwrap_or(false)
{
    let cef_release = crate::cef::release_dir();
    let status = std::process::Command::new("patchelf")
        .args(["--set-rpath", &cef_release.display().to_string(), &bin_path])
        .status();
    match status {
        Ok(s) if s.success() => eprintln!("[install] patched rpath on {name}"),
        _ => eprintln!("[install] WARN: patchelf failed; binary may not find libcef.so without LD_LIBRARY_PATH"),
    }
}
```

- [ ] **Step 2: Verify**

Run: `cargo make install sola-kit`
Expected: install reports `[install] patched rpath on sola-kit`.

Run: `ldd /opt/sola/bin/sola-kit | grep libcef`
Expected: shows `libcef.so => /home/joshua/.cache/sola/cef-<ver>/Release/libcef.so`.

Run: `unset LD_LIBRARY_PATH && /opt/sola/bin/sola-kit` from a TTY.
Expected: launches without LD_LIBRARY_PATH.

- [ ] **Step 3: Commit**

```bash
git add crates/sola-make/src/install.rs
git commit -m "feat(sola-make): patchelf rpath on installed CEF-linked binaries"
```

---

### Task F2: Update worktree CLAUDE.md

**Files:**
- Modify: `CLAUDE.md`

- [ ] **Step 1: Replace the Web Frontends section**

Replace the entire `## Web Frontends: Preact + signals + JSX` section with:

```markdown
## Web Frontends: Preact + signals + JSX (CEF-hosted)

sola-kit apps render their UI with **Preact** (`preact`, vendored at
`crates/sola-kit/web/vendor/preact/`) and reactivity via
`@preact/signals`. JSX is transformed server-side by swc — there is no
bundler, no Node, and no `tsc` in the loop. The runtime engine is CEF
(Chromium Embedded Framework) in offscreen rendering mode; sola-kit owns
the Wayland surface (via `smithay-client-toolkit`) and presents
CEF-produced dma-bufs through `zwp_linux_dmabuf_v1`.

### Pipeline at runtime

- Surface side: `crates/sola-kit/src/wayland/{client,surface,input}.rs`.
  We own the `xdg_toplevel`; CEF renders into it.
- Engine side: `crates/sola-kit/src/cef/`. `cef::Browser` is one CEF
  browser per Surface; `cef::handlers::RenderHandler::on_accelerated_paint`
  forwards the dma-buf to the surface; `cef::ipc::IpcHandler` bridges
  `window.cefQuery({...})` ↔ `KitApp::on_js_command`; `cef::scheme`
  serves `app://...` from `AssetBundle` + `swc` transform.

### CEF binaries

CEF lives at `~/.cache/sola/cef-<version>/`. The version is pinned in
`crates/sola-make/src/cef.rs::CEF_VERSION`. `sola-make install-cef`
downloads it from the Spotify CDN if missing. Installed binaries get
their rpath patched at install time so `libcef.so` is found without
`LD_LIBRARY_PATH`. Dev runs via `cargo make run` use a wrapper that
sets `LD_LIBRARY_PATH` from `target/cef-runpath`.

### Known caveats

- `no_sandbox = true` is intentional in dev; flip to false for a future
  prod mode (requires SUID `chrome-sandbox` setup in `configuration.nix`).
- Multi-WebView-per-surface composition is supported by the architecture
  (each WebView is one `cef::Browser` + dma-buf source) but not yet
  exercised by sola-kit. Future sola-browser will use it.

### Imports + transform pipeline + tsconfig

(unchanged — keep the existing Build pipeline, A component, Signals,
Slots, Lists, CSS imports, Module imports, Common pitfalls subsections.)
```

- [ ] **Step 2: Commit**

```bash
git add CLAUDE.md
git commit -m "docs(claude-md): document CEF + sctk + OSR architecture"
```

---

### Task F3: Update `~/CLAUDE.md` with NixOS deps list

**Files:**
- Modify: `~/CLAUDE.md`

- [ ] **Step 1: Add NixOS section**

Append to `~/CLAUDE.md` (the user-level CLAUDE.md, not the project's):

```markdown
## Sola CEF dependencies (configuration.nix)

Sola-kit and any future CEF-based sola apps need these packages
available at runtime. Add to `environment.systemPackages` in
`/etc/nixos/configuration.nix`:

- libGL, libgbm, mesa            # GPU + EGL/GLES
- libnss, libnspr                # NSS for crypto
- fontconfig, freetype           # font rendering
- expat, alsaLib                 # misc deps
- libdrm                         # DMA-BUF
- libxkbcommon, wayland          # input + display
- patchelf                       # for cargo make install rpath fix
```

- [ ] **Step 2: Verify**

Confirm `~/CLAUDE.md` shows the new section. (No commit — `~/CLAUDE.md` is user-private, not in the repo.)

---

### Task F4: Final cleanup

**Files:** scan for any straggler imports

- [ ] **Step 1: grep for dead imports**

```bash
grep -rn "use gtk4\|use gdk4\|use webkit6\|use glib::\|use gio::" crates/sola-kit/src/
```

Expected: empty.

If any results, delete those lines.

- [ ] **Step 2: Verify final state**

Run: `cargo make build sola-kit`
Expected: clean, no warnings.

Run: `cargo make install sola-kit && /opt/sola/bin/sola-kit`
Expected: launches; counter works; F12 opens DevTools; resize DevTools panel — no freeze, no warnings.

- [ ] **Step 3: Commit (only if anything was deleted)**

```bash
git add -u
git commit -m "chore(sola-kit): remove dead webkit/gtk imports"
```

---

## Final state

After all tasks:
- `crates/sola-kit/` has no gtk4/webkit6 dependencies.
- sola-kit launches as a CEF-backed app, renders the Preact storybook, accepts keyboard + mouse, theme push works, F12 opens DevTools (no freeze on splitter resize).
- `cargo make install sola-kit` is a one-step deploy that handles CEF download (if missing) + rpath patching.
- The branch `sola-kit-preact` has 30+ commits, one per task, with no merge to master.

If at any point the user says to merge to master, refer them back to the project CLAUDE.md and the saved memory `feedback_master_merge_permission.md`. Do not merge unless they expressly say so.
