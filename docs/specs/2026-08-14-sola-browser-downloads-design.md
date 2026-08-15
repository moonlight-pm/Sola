# sola-browser downloads

**Date:** 2026-08-14  
**Status:** **Frozen** — implemented and dogfooded on `naturalethic/browser-polish`  
**Related:** [profiles](2026-08-10-sola-browser-profiles-design.md) (`shared/` downloads index); [manual](../manual/sola-browser.md)

## Intent

A click that starts a file download should produce a file in `~/Downloads`
and be visible in chrome: what is in flight, and what already finished.

## Product rules

| Rule | Choice |
|------|--------|
| Start | Auto-save. No Save dialog in this slice. |
| Folder | `~/Downloads` (create if missing). Name collision → `file (1).ext`. |
| Indicator | Toolbar download icon always visible (right of vault / cards). |
| While in progress | Accent on the icon + a thin progress hairline on the button. Panel does **not** auto-open. |
| After finish (unseen) | Quiet accent on the icon until the panel is opened. |
| Panel | Click the icon. Same top-right card as vault / cards (mutually exclusive). |
| In-progress row | Filename, percent / bytes, **Cancel**. |
| Completed row | Click opens the file (`xdg-open`). **Remove** drops the row only (file stays). |
| Failed row | Shown as failed; Remove dismisses. No retry in this slice. |
| Canceled | Row disappears. Partial file on disk is left alone. |
| Persistence | Completed + failed survive quit in `~/.local/share/sola/browser/shared/downloads.json`. Shared across profiles. In-progress is session-only. |
| Not this slice | Save-as, custom folder, show-in-folder, delete-file, clear-all, pause/resume, dangerous-type warnings. |

## Architecture

Chrome owns the list and the JSON. Each profile helper runs CEF
`DownloadHandler` and reports over the existing control socket.

```
page click
  → helper OnBeforeDownload  (unique path under ~/Downloads, no dialog)
  → helper OnDownloadUpdated (throttled Progress; Complete / Canceled / Failed)
  → FromEngine::Download     (any helper, including parked)
  → router DownloadsHandle
  → chrome Tick applies + persists terminals
```

Cancel is `Cmd::CancelDownload { profile_id, id }` → that helper’s
`DownloadItemCallback::cancel`. Chrome also drops the row immediately.

Download ids collide across helpers, so chrome keys live items as
`(profile_id, cef_id)` and persists a uuid.

## Surfaces

- **Icon** — `lucide/download`. Idle = muted. In progress = accent + hairline.
  Unseen complete = accent, no hairline. Open panel = accent wash (same as vault).
- **Panel** — title **Downloads**. Flat rows (no nested cards). Long
  hash names use middle-ellipsis. In-progress first, then completed
  (newest first). Empty: **Nothing downloaded yet.**

## Out of scope

History UI, bookmarks, save-as, Finder / “show in folder”, delete-from-disk,
download shelf, `chrome://downloads`.

## Implementation status

| Item | Status |
|------|--------|
| Freeze | **this document** |
| Helper `DownloadHandler` + IPC | **done** |
| Chrome list + persist + panel + icon | **done** |
| Dogfood | local 2026-08-15 |

## Decision log

| Date | Choice |
|------|--------|
| 2026-08-14 | Auto-save to `~/Downloads`; no dialog |
| 2026-08-14 | Permanent toolbar icon; progress on the icon; panel click-to-open |
| 2026-08-14 | Cancel + open-on-click + remove-from-list; no show-in-folder (no Finder) |
| 2026-08-14 | Persist completed/failed in shared `downloads.json` |
| 2026-08-15 | Flat panel rows + middle-ellipsis for hash filenames |
