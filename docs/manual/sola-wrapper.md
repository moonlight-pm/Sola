# Website wrappers

A website wrapper runs a website as its own Sola app, with its own
launcher name, icon, window, and login. It is not Browser: there are no
tabs, location bar, or vault. The examples below use Slack.

## Add

Settings → **Applications** → **+ Add application** → check **Web wrapper**.

- `app_id`: a stable id (`slack`). It becomes the Wayland and bus `app_id`.
- **label**: the launcher and window title.
- **icon**: a pack name (`simpleicons/slack`) or a filesystem path.
- **url**: an `http://` or `https://` start URL.

The command is set to `/opt/sola/bin/sola-wrapper <app_id>` and cannot be edited.

## Launch

Choose the wrapper’s label in the launcher, or run:

```bash
/opt/sola/bin/sola-wrapper slack
```

A second launch raises the existing window. Cookies live under `~/.config/sola/wrapper/<id>/`, not in a Browser profile.

## Edit

Menubar **Edit** (Cut / Copy / Paste / Select All) is Super+X / C / V / A. The shell binds those chords globally and routes them back as menu actions. The page never sees Super, so the wrapper handles the action (the same path Browser uses). Paste reads the compositor clipboard in the chrome: an image (`image/png` and siblings) is injected as a `File` paste into the focused field (Slack’s composer); otherwise the text is inserted once.

## Links

`target=_blank`, ⌘-click, and `window.open` to another site open in **Browser** (raising the existing window, or launching it). Slack itself (channels, threads, sign-in) stays in this window. `mailto:` and `javascript:` links are ignored.

## Huddles and microphone

Slack huddles open with `window.open('about:blank')` and then write the huddle UI into that popup. The popup gets a window of its own — a row in the shell's tab bar, named for the page — and comes up in front; the channel stays in the app's window. Closing the popup's window ends the popup (Slack leaves the huddle); when the page closes it, its window goes. Closing the app's own window quits the app.

Starting the huddle then asks for the microphone (and the camera if you turn it on) with an Allow / Block overlay. The choice is stored at `~/.config/sola/wrapper/<id>/media.json` and applied to Chromium’s camera and microphone site settings. Playback of huddle audio does not need this prompt.

USB cameras show up through V4L2 (`/dev/video*`). The seat user can open those nodes (logind `uaccess`). Add your user to the `video` group if a non-seat tool cannot. A webcam’s built-in microphone (a LifeCam or similar) is a normal PipeWire source; the shell volume chip lists it.

## JavaScript dialogs

`alert()`, `confirm()`, and `prompt()` show as a kit dialog over the page
(the same overlay as Allow / Block). Leave-page confirms use Leave / Stay.

## Notifications

A page that calls `Notification.requestPermission()` gets an Allow / Block dialog in the wrapper window. The choice is stored at `~/.config/sola/wrapper/<id>/notifications.json`. After **Allow**, `new Notification(title, { body })` is a Sola desk card (top-right under the menubar), not a banner over the page. Click the card to raise this wrapper. Sites you have not allowed cannot notify.

## Limits

Website wrappers have no Bitwarden fill, downloads, tab chrome, throwaway `--url` windows, or PWA install.
