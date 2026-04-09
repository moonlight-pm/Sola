/// Wayland listening socket.
///
/// Creates a Unix socket that Wayland clients connect to. The socket name
/// (e.g., `wayland-0`) is set in `$WAYLAND_DISPLAY` so clients know where
/// to connect. Each incoming connection is registered with the Wayland
/// Display as a new client.
///
/// See: https://docs.rs/smithay/0.7.0/smithay/wayland/socket/struct.ListeningSocketSource.html
use smithay::reexports::calloop::LoopHandle;
use smithay::wayland::socket::ListeningSocketSource;

use crate::Sola;
use crate::error::SocketError;
use crate::wayland::new_client_state;

/// Set up the Wayland listening socket and register it with the event loop.
///
/// Returns the socket name (e.g., "wayland-0") so the caller can set
/// `$WAYLAND_DISPLAY` for child processes.
pub fn listen(loop_handle: &LoopHandle<'static, Sola>) -> Result<String, SocketError> {
    let listener =
        ListeningSocketSource::new_auto().map_err(|e| SocketError::Bind(e.to_string()))?;
    let socket_name = listener.socket_name().to_string_lossy().into_owned();

    loop_handle
        .insert_source(listener, |client_stream, _, sola| {
            if let Err(err) = sola
                .display_handle
                .insert_client(client_stream, new_client_state())
            {
                tracing::error!(?err, "failed to accept Wayland client");
            }
        })
        .map_err(|e| SocketError::EventSource(e.to_string()))?;

    tracing::info!(%socket_name, "Wayland socket listening");
    Ok(socket_name)
}
