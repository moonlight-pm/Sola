# Clipboard Design

## Problem

Sola apps (WebKit WebViews) have no access to the system clipboard. `navigator.clipboard` is sandboxed inside the WebView and doesn't participate in Wayland's `wl_data_device` protocol. Copy/paste between apps (or even within a single app's terminal) doesn't work.

## Design

The clipboard flows through the bus. The compositor is the Wayland clipboard authority. Sola apps read/write clipboard via the bus, with sola-app providing a transparent helper API.

### Bus Topic

```
Clipboard(ClipboardPayload)
```

```rust
struct ClipboardPayload {
    /// MIME type (e.g. "text/plain", "image/png")
    mime_type: String,
    /// Inline text data — used for text/* MIME types
    text: Option<String>,
    /// Filename in ~/.cache/sola/clipboard/ — used for binary data
    file: Option<String>,
}
```

- **Text clipboard**: `text` is set, `file` is `None`. Data inline in the bus message.
- **Binary clipboard**: `file` is set (UUID filename), `text` is `None`. Data in `~/.cache/sola/clipboard/<uuid>`.
- Multiple MIME types from one copy operation: multiple `Clipboard` messages, one per MIME type.

### Cache Directory

- Location: `~/.cache/sola/clipboard/`
- Files named by UUID (e.g. `a1b2c3d4-...`)
- Flushed on desktop start by the sola process manager
- Multiple files can coexist

### Compositor (sola-compositor)

**Outbound (Wayland client copies):**
- Implement `SelectionHandler::new_selection` to detect when a Wayland client (including XWayland via sola-x) sets the clipboard
- Read the data from the `wl_data_source`
- For text MIME types: emit `Clipboard` with inline text
- For binary MIME types: write to `~/.cache/sola/clipboard/<uuid>`, emit `Clipboard` with filename

**Inbound (sola app copies):**
- Listen for `Clipboard` bus messages
- Set the data as the Wayland selection so non-sola Wayland clients can paste from it

### sola-app Framework

**Automatic bus handling:**
- The framework's bus event loop intercepts `Clipboard` messages
- Caches the latest clipboard state in `AppCtx`
- Passes the event through to `on_bus_event` so apps can react if needed

**API:**
```rust
// Copy — emits Clipboard on the bus
ctx.set_clipboard(mime_type: &str, data: &[u8]);

// Paste — reads from cached clipboard state
ctx.get_clipboard(mime_type: &str) -> Option<Vec<u8>>;
```

- `set_clipboard` for text types sends inline. For binary, writes the cache file and sends the filename.
- `get_clipboard` for text returns from the cached message. For binary, reads the cache file.

### Terminal (example consumer)

Replace `navigator.clipboard` calls with IPC round-trip:
- **Copy**: JS sends `clipboard_set` command with selected text. Rust calls `ctx.set_clipboard("text/plain", text.as_bytes())`.
- **Paste**: JS sends `clipboard_get` command. Rust calls `ctx.get_clipboard("text/plain")`, replies with the text.

### Process Manager (sola)

On startup, flush `~/.cache/sola/clipboard/` to clean stale files from previous sessions.

## Scope

This design covers text clipboard end-to-end. Binary clipboard (images) uses the same path and file mechanism but compositor-side reading of binary data sources from Wayland clients is deferred — the infrastructure is in place for when it's needed.
