# Preview

Preview (`sola-preview`) is a simple image viewer. Launch it from the
launcher or with `sola-preview /path`. Super+Shift+3/4/5 copy to the
clipboard; they do **not** open this app.

## Open

- Launcher → **Preview**
- `sola-preview /path/to.png`
- When Preview is already running, an `OpenImage` message with
  `app_id=sola-preview` replaces the main view and lands in **Recent**.

`solactl compositor screenshot` writes a PNG and prints the path. It does
**not** open Preview.

MIME and `solactl open` send image files to **Paint**.

## Copy

The header has **Copy** and **Copy path**.

- **Copy** — the image bytes on the system clipboard (`image/png` for
  screenshots). Paste into Slack, the browser, or any Wayland client that
  accepts images. Closing Preview drops the clipboard.
- **Copy path** — the file’s absolute path as text.

An image is not copied automatically. Click **Copy**.

## Limits

- No zoom.
- Each open is a new process (no single instance).
- The clipboard source lives in this process.
