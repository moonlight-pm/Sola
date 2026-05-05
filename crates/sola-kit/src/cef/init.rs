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
