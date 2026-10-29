# Preview

Preview (`sola-preview`) is a simple image viewer: a **Recent** list on
the left, the selected image fitted on the right, and **Copy** for its
bytes.

Preview does not ship on Oath: `pkg:sola` does not carry `sola-preview`.
The launcher still lists **Preview**, and picking it there fails (the top
row says it could not launch). The rest of this note is how the app works
where it is installed.

## Open

- Launcher → **Preview**
- `sola-preview /path/to.png` (several paths open several entries in
  **Recent**; the last one is shown)
- While Preview is running, an `OpenImage` bus message with
  `app_id=sola-preview` shows that image and puts it at the top of
  **Recent**.

Preview's window floats by default. Its titlebar has the **Preview** menu
(Quit Preview, ⌘Q).

Super+Shift+3/4/5 copy to the clipboard; they do **not** open Preview.
`solactl compositor screenshot` writes a PNG and prints the path; it does
not open Preview either. MIME and `solactl open` send image files to
**Paint**.

## Copy

The header has **Copy** and **Copy path**.

- **Copy** — the file's bytes on the system clipboard, offered as the
  image type they are (`image/png`, JPEG, GIF, WebP, or BMP; up to 32 MiB).
  Paste into the browser or any Wayland client that accepts images.
  Closing Preview drops the clipboard.
- **Copy path** — the file's absolute path as text.

An image is not copied automatically. Click **Copy**.

## Limits

- No zoom.
- Each open is a new process (no single instance).
- **Recent** lasts only as long as the process and holds 64 images.
- The clipboard source lives in this process.
