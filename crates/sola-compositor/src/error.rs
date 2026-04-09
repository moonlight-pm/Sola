/// Error types for the sola compositor.
///
/// Each error enum covers a specific subsystem. Smithay's internal error
/// types are often generic or non-Send, so we convert them to descriptive
/// strings at the boundary rather than wrapping them directly.
use std::io;
use std::path::PathBuf;

use smithay::backend::drm::DrmNode;
use thiserror::Error;

/// Top-level compositor error — returned from `run()`.
#[derive(Debug, Error)]
pub enum CompositorError {
    #[error("session: {0}")]
    Session(#[from] SessionError),

    #[error("gpu: {0}")]
    Gpu(#[from] GpuError),

    #[error("device: {0}")]
    Device(#[from] DeviceError),

    #[error("input: {0}")]
    Input(#[from] InputError),

    #[error("udev: {0}")]
    Udev(#[from] io::Error),

    #[error("event loop: {0}")]
    EventLoop(String),

    #[error("wayland display: {0}")]
    Display(String),
}

/// Session management errors (libseat).
#[derive(Debug, Error)]
pub enum SessionError {
    #[error("failed to open libseat session: {0}")]
    Open(String),
}

/// GPU discovery and renderer errors.
#[derive(Debug, Error)]
pub enum GpuError {
    #[error("no GPU found for seat '{seat}'")]
    NotFound { seat: String },

    #[error("failed to resolve DRM node from {path}: {reason}")]
    NodeResolution { path: PathBuf, reason: String },

    #[error("failed to create GPU manager: {0}")]
    ManagerCreation(String),

    #[error("failed to register GPU {node:?}: {reason}")]
    Registration { node: DrmNode, reason: String },
}

/// DRM device errors.
#[derive(Debug, Error)]
pub enum DeviceError {
    #[error("failed to open device {path}: {reason}")]
    Open { path: PathBuf, reason: String },

    #[error("failed to create DRM device from {path}: {reason}")]
    DrmInit { path: PathBuf, reason: String },

    #[error("failed to create GBM device from {path}: {source}")]
    GbmInit { path: PathBuf, source: io::Error },

    #[error("failed to initialize DRM output on {node:?}: {reason}")]
    OutputInit { node: DrmNode, reason: String },

    #[error("failed to register event source for {node:?}: {reason}")]
    EventSource { node: DrmNode, reason: String },
}

/// Input subsystem errors.
#[derive(Debug, Error)]
pub enum InputError {
    #[error("failed to assign libinput to seat '{seat}'")]
    SeatAssign { seat: String },

    #[error("failed to register input event source: {0}")]
    EventSource(String),
}
