# sola-wrapper

Wrap a website as its own Sola app (own launcher name, icon, window, and login). Not the product browser — no tabs, omnibox, or vault.

## Add

Settings → **Applications** → **+ Add application** → check **Web wrapper**.

- `app_id` — stable id (`slack`). Becomes the Wayland / bus `app_id`.
- **label** — launcher and window title
- **icon** — pack name (`simpleicons/slack`) or a filesystem path
- **url** — `http://` or `https://` start URL

Command is synthesized as `/opt/sola/bin/sola-wrapper <app_id>` and is not editable.

## Launch

Launcher → the wrapper’s label, or:

```bash
/opt/sola/bin/sola-wrapper slack
```

A second launch raises the existing window. Cookies live under `~/.config/sola/wrapper/<id>/` (not sola-browser’s profile).

## Copy and paste

Super+X / Super+C / Super+V / Super+A (Cut / Copy / Paste / Select All) are the same chords as sola-browser. The shell routes them through the wrapper’s **Edit** menu; copy writes the system clipboard from the page selection, and paste inserts once into the focused field without emptying the clipboard.

## Not in this pass

Bitwarden fill, downloads, tab chrome, opening other origins in sola-browser, throwaway `--url` windows, PWA install.
