/// Wayland listening socket.
///
/// Creates or adopts a Unix socket that Wayland clients connect to. The socket
/// name (e.g., `wayland-0`) is set in `$WAYLAND_DISPLAY` so clients know where
/// to connect.
///
/// Supports two modes:
/// - **Fresh:** Create a new socket bound to "wayland-0" (normal startup).
/// - **Inherited:** Adopt an FD passed via `--wayland-fd` (restart after execv).
use std::io;
use std::os::unix::io::{AsRawFd, FromRawFd, RawFd};
use std::os::unix::net::UnixListener;
use std::path::PathBuf;

use smithay::reexports::calloop::generic::Generic;
use smithay::reexports::calloop::{
    EventSource, Interest, LoopHandle, Mode, Poll, PostAction, Readiness, Token, TokenFactory,
};

use crate::State;
use crate::error::SocketError;
use crate::wayland::new_client_state;

/// Set up the Wayland listening socket and register it with the event loop.
///
/// If `inherited_fd` is Some, adopts that FD as the listening socket (restart
/// path). Otherwise creates a fresh socket pinned to "wayland-0".
///
/// Returns the socket name and the raw FD (for preserving across future restarts).
pub fn listen(
    loop_handle: &LoopHandle<'static, State>,
    inherited_fd: Option<RawFd>,
) -> Result<(String, RawFd), SocketError> {
    let listener = if let Some(fd) = inherited_fd {
        let listener = unsafe { UnixListener::from_raw_fd(fd) };
        listener
            .set_nonblocking(true)
            .map_err(|e| SocketError::Bind(format!("set_nonblocking on inherited fd: {e}")))?;
        tracing::info!(fd, "adopting inherited Wayland socket");
        listener
    } else {
        let runtime_dir = std::env::var("XDG_RUNTIME_DIR")
            .map_err(|_| SocketError::Bind("XDG_RUNTIME_DIR not set".into()))?;
        let socket_path = PathBuf::from(&runtime_dir).join("wayland-0");

        // Remove stale socket file if present.
        let _ = std::fs::remove_file(&socket_path);

        let listener = UnixListener::bind(&socket_path)
            .map_err(|e| SocketError::Bind(format!("bind {}: {e}", socket_path.display())))?;
        listener
            .set_nonblocking(true)
            .map_err(|e| SocketError::Bind(format!("set_nonblocking: {e}")))?;
        tracing::info!(path = %socket_path.display(), "Wayland socket listening (fresh)");
        listener
    };

    let raw_fd = listener.as_raw_fd();
    let socket_name = "wayland-0".to_string();

    let source = SocketSource {
        inner: Generic::new(listener, Interest::READ, Mode::Level),
    };

    loop_handle
        .insert_source(source, |client_stream, _, state| {
            match state
                .display_handle
                .insert_client(client_stream, new_client_state())
            {
                Ok(_) => tracing::info!("new Wayland client connected"),
                Err(err) => tracing::error!(?err, "failed to accept Wayland client"),
            }
        })
        .map_err(|e| SocketError::EventSource(e.to_string()))?;

    Ok((socket_name, raw_fd))
}

/// Calloop event source wrapping a UnixListener for Wayland client connections.
struct SocketSource {
    inner: Generic<UnixListener>,
}

impl EventSource for SocketSource {
    type Event = std::os::unix::net::UnixStream;
    type Metadata = ();
    type Ret = ();
    type Error = io::Error;

    fn process_events<F>(
        &mut self,
        readiness: Readiness,
        token: Token,
        mut callback: F,
    ) -> io::Result<PostAction>
    where
        F: FnMut(Self::Event, &mut Self::Metadata) -> Self::Ret,
    {
        self.inner.process_events(readiness, token, |_, listener| {
            loop {
                match listener.accept() {
                    Ok((stream, _)) => callback(stream, &mut ()),
                    Err(e) if e.kind() == io::ErrorKind::WouldBlock => break,
                    Err(e) => return Err(e),
                }
            }
            Ok(PostAction::Continue)
        })
    }

    fn register(
        &mut self,
        poll: &mut Poll,
        token_factory: &mut TokenFactory,
    ) -> smithay::reexports::calloop::Result<()> {
        self.inner.register(poll, token_factory)
    }

    fn reregister(
        &mut self,
        poll: &mut Poll,
        token_factory: &mut TokenFactory,
    ) -> smithay::reexports::calloop::Result<()> {
        self.inner.reregister(poll, token_factory)
    }

    fn unregister(&mut self, poll: &mut Poll) -> smithay::reexports::calloop::Result<()> {
        self.inner.unregister(poll)
    }
}
