# Web apps

A web app is a website that Browser runs as an application of its own: a
launcher entry, a window with the app's name and menus, and no location
bar. It runs on the Browser profile it was made from, so it is signed in
wherever that profile is. Web apps take the place of the separate Wrapper
application, which is gone. The examples below use Slack.

## Make one

Open the site in Browser, signed in, at the page the app should start on.
Then choose **Browser → Wrap This Page…** in the titlebar, or **Wrap This
Page…** in the page's right-click menu. Both work only on an `http://` or
`https://` page that is not already an app's.

A sheet asks for the app's **Name**, suggested from the page title: the
site's part of it (*Slack* from "general - Acme - Slack"), else the
title, else the site's domain. **Wrap** (or Enter) turns the page into the
app in place: no reload and no new sign-in. Its window takes the app's
name and menus, its row in the bar says the app's name, and the launcher
lists it. **Cancel**, or a click beside the sheet, leaves the page as it
was.

- The app id is generated (`web-` and a uuid); there is nothing to type.
- The icon is the page's favicon, saved as
  `~/.local/share/sola/icons/<app_id>.png`. A page without one gets a
  globe.
- The app starts at the page's address at the moment you wrapped it.
- The app is an entry in the application catalog (`Application`, kind
  `web`), which the bus keeps in `~/.config/sola/state.yaml`. See
  [the session manager](session.md#web-apps).

## The app's window

The titlebar shows the app's name and its menus:

| Menu | Items |
|---|---|
| *App name* | Open in Browser · Reload (⌘R) · Close Window (⌘W) · Remove Web App… · Quit *name* (⌘Q) |
| Edit | Cut · Copy · Paste · Select All (⌘X / ⌘C / ⌘V / ⌘A) |
| Window | The Window menu every Sola application has |

- **Open in Browser** opens the app's current page as a Browser page
  beside it, on the same profile.
- **Close Window** closes this window of the app; **Quit** *name* closes
  all of them. Browser itself quits only when those were its last
  windows.

The page's own dialogs open in the app's window: JavaScript dialogs,
HTTP sign-in, camera and microphone, notifications, and file uploads. Its
notifications carry the app's name; clicking one brings its window
forward.

Links stay in the app's window while they stay on the app's site. The
site is the last two parts of the host name, so `app.slack.com` and
`files.slack.com` are the same site. A link you click to another site
opens as a Browser page instead. Redirects, such as the round trip of a
sign-in, stay in the window. A page opened in a new window
(`target=_blank`, ⌘-click) stays in the app when it is on the app's site,
else it opens as a Browser page. A popup the page opens with
`window.open` (a call's window, say) is the app's.

## Launch

Choose the app's name in the launcher. Its command is
`/bin/sola-browser --app <app_id>`. If Browser is running, the app's
window comes forward, or opens at its start page. Otherwise Browser
starts (with its saved pages) and opens it. Launching it again brings the
same window forward.

Browser's session keeps app windows as it keeps pages: after a restart
they come back as the app, in their place in the bar.

## Remove

**Remove Web App…** in the app's menu asks once (**Remove** / **Cancel**),
then takes the app out of the launcher, deletes its icon, and closes its
windows. The sign-in stays in the Browser profile.

Settings → Applications does not list web apps and cannot edit them.

## Limits

- An app's window has no location bar, no right-click menu, no find bar,
  no vault or downloads button, and no Developer Tools. **Open in
  Browser** for those.
- The app always runs on the profile it was wrapped from; it cannot move
  to another.
- Clicking a sign-in button that leaves the site ("Sign in with Google")
  opens the sign-in as a Browser page. Signing in there signs the app in
  too, since it is the same profile; reload the app afterwards.
- No per-app zoom, no badge counts on the icon, no way to change an app's
  name, start page, or icon, and no way to wrap a page into an app that
  already exists.
