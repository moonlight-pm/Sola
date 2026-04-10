/// Error types for sola-x.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("event loop: {0}")]
    EventLoop(String),

    #[error("display: {0}")]
    Display(String),

    #[error("xwayland: {0}")]
    XWayland(String),

    #[error("socket: {0}")]
    Socket(String),
}
