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

## Edit

Menubar **Edit** (Cut / Copy / Paste / Select All) is Super+X / C / V / A. The shell binds those chords globally and routes them back as menu actions — the page never sees Super, so the wrapper must handle the action (same pipe as sola-browser). Paste reads the Wayland clipboard in chrome and inserts once into the focused field.

## Links

`target=_blank`, ⌘-click, and `window.open` to another site open in **sola-browser** (raises the existing window, or launches it). Slack itself — channels, threads, sign-in — stays in this window. `mailto:` and `javascript:` links are ignored.

## Huddles / microphone

Slack huddles open with `window.open('about:blank')` and then write the huddle UI into that popup. The wrapper has no OS window for that, so CEF creates a windowless popup and this app paints it (replacing the channel view until the huddle closes).

Starting the huddle then asks for the mic (and camera if you turn it on) with an Allow / Block overlay. The choice is stored at `~/.config/sola/wrapper/<id>/media.json`. Playback of huddle audio does not need this prompt.

## JavaScript dialogs

`alert()`, `confirm()`, and `prompt()` show as a kit dialog over the page
(same overlay as Allow / Block). Leave-page confirms use Leave / Stay.

## Notifications

A page that calls `Notification.requestPermission()` gets an Allow / Block dialog in the wrapper window. The choice is stored at `~/.config/sola/wrapper/<id>/notifications.json`. After **Allow**, `new Notification(title, { body })` is a Sola desk card (top-right under the menubar), not a banner over the page. Click the card to raise this wrapper. Sites you have not allowed cannot notify.

## Not in this pass

Bitwarden fill, downloads, tab chrome, throwaway `--url` windows, PWA install.
