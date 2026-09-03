# sola-preview

Image viewer. Super+Shift+3/4/5 copy to the clipboard; they do **not**
open this app. Launch Preview from the launcher or `sola-preview /path`.

**Partial.** **Copy** (image bytes) **installed** `kit` + `preview` +
`browser` + `wrapper` (debug, 2026-09-01).

## Open

- Launcher → **Preview**
- `sola-preview /path/to.png`
- Already running: `OpenImage` with `app_id=sola-preview` replaces the
  main view and lands in **Recent**

`solactl compositor screenshot` writes a PNG and prints the path. It does
**not** open Preview.

MIME / `solactl open` image files go to **Paint**.

## Copy

The header has **Copy** and **Copy path**.

- **Copy** — image bytes on the system clipboard (`image/png` for
  screenshots). Paste into Slack, the browser, or any Wayland client that
  accepts images. Closing Preview drops the clipboard.
- **Copy path** — the file’s absolute path as text.

A shot is not copied automatically. Click **Copy**.

## Limits

- No zoom.
- Each open is a new process (no single-instance).
- Clipboard source lives in this process.
