# Web apps — wrap a page in the browser

**Date:** 2026-10-23
**Status:** **Accepted** 2026-10-23 (Joshua: "go ahead") — his decisions
W1–W5 and the calls marked *proposed*. Built as Phase 12 on `next`
(12a–12e); smoked on canto except a signed-in site and a huddle popup.
**Supersedes:** the separate `sola-wrapper` process (one CEF process and
profile per app, [wrapper freeze](2026-08-21-sola-wrapper-design.md)) and
the wrapper form in Settings → Applications.
**Related:** [Next](2026-10-21-next-tab-bar-design.md) (a window per page,
§ The wrapper: a window per popup), plan
[`plans/2026-10-21-next-tab-bar-plan.md`](../plans/2026-10-21-next-tab-bar-plan.md)
Phase 12.

## Why

Setting up a web app today means Settings → Applications → a form (id,
label, icon, URL), then signing in again in the app's own profile. Joshua
wants it easy: you are already on the site, signed in, at the right page —
make *that* the app.

## Decided (Joshua, 2026-10-23)

| | Decision |
|--|--|
| W1 | A web app **runs in the browser**, on the browser profile it was wrapped from: already signed in, one CEF stack instead of one per app. |
| W2 | You **add one from the browser**: "Wrap This Page…" on the page. |
| W3 | A web app has **its own menus** (its window's titlebar) and is **removed from there**. Not in Settings; not under the flower. |
| W4 | **No app id to type:** a new app's id is generated (a UUID). |
| W5 | **No migration:** existing wrapper apps are not carried over; Joshua starts fresh on his other machine when `next` lands there. |

## What a web app is (proposed)

- An Applications catalog entry of kind `web` (today's `wrapper` kind,
  renamed in the UI only): a generated app id (`web-<uuid>`), name, icon,
  start URL, and the browser profile it runs on.
- **Its windows are the browser's**, with the app's identity: the app's
  Wayland app id, name and icon, so the shell's bar, the launcher, Super+Tab
  and desk cards treat it as its own app. No location bar, no browser
  menus — the titlebar shows the app's name and its menus.
- It runs under its own profile whatever the workspace's profile (the one
  exception to slice 5's rule).

## Wrap This Page… (proposed)

- In the browser's **Browser** menu (and the page's right-click menu), on any
  http(s) page.
- A small sheet over the page: **Name** (the site's name, from the page
  title), **Icon** (the page's favicon; a monogram when it has none), and
  **Wrap**. No app id to type: it is generated (W4).
- **Wrap** turns that page into the app in place: same page, no reload, no
  sign-in — the window changes to the app's (its row in the shell's bar
  now says the app's name with its icon), and the launcher lists it.
- The URL it starts at next time is the page's URL at that moment.

## The app's window (proposed)

| Menu | Items |
|--|--|
| *App name* | Open in Browser (this page as a browser page) · Reload ⌘R · **Remove Web App…** · Quit *name* ⌘Q |
| Edit | Cut · Copy · Paste · Select All |
| Window | (the kit's) |

- **Links** inside the app's site stay in its window; a link the user
  follows to another site opens as a browser page. Redirects stay in the
  window (a sign-in's round trip); a sign-in *started* by a click ("Sign
  in with Google") opens as a browser page, which signs the app in too —
  it is the same profile.
- **Popups** it opens (a huddle, a same-site `window.open`) are more
  windows of the app — its name, its id, a row each in the bar.
- **Notifications, microphone / camera, dialogs:** as the browser's, under
  the app's name (desk cards say *Slack*, not *Browser*).
- **Remove Web App…** asks once, then takes the app out of the launcher
  and closes its windows. Its sign-in stays in the profile (the browser
  still has it).
- Closing the app's last window closes the app. The browser process keeps
  running while any window — page or app — is open.

## Launching (proposed)

- The catalog entry's command is `sola-browser --app <id>`. A running
  browser opens the app's window, or brings it up if open; otherwise the
  browser starts with just that window.
- The browser's session keeps app windows like pages and restores them.

## Retiring the wrapper

- No migration (W5): existing `wrapper` entries are not converted.
- `sola-wrapper` retires outright (its popup windows of 2026-10-23 with
  it) and leaves the Oath packages.
- Settings → Applications keeps command apps; its wrapper fields go.

## Mechanics (proposed)

- **Kit:** `TabWindows` opens a window under another app id (the tab's
  `app_id` and the window's Wayland app id); `menu::publish_for` sends an
  `AppMenuPayload` for another app id without replacing the process's own
  (the shell needs it for shortcuts); the titlebar takes per-window menus
  (`WindowFrame::menus`, exists).
- **Shell:** pairing groups a process's tabs and windows by `(pid,
  app_id)`, so one process can hold windows of several apps.
- **Browser:** a registry of hosted apps (tab → app); page windows open
  with the app's id and chrome; menu actions for hosted ids; notifications
  under the app's id; `--app <id>` and an `OpenApp` handoff; the wrap sheet
  writes the catalog entry (`Topic::Application`).

## Not now

App-specific settings (per-app zoom, badge counts on the icon), moving an
app to another profile, wrapping a page into an existing app.
