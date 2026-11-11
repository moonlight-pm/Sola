# Web apps

A web app is a website that Browser runs as an application of its own: a
launcher entry, a window with the app's name and menus, and no location
bar. It runs on the Browser profile it was made from, so it is signed in
wherever that profile is; one window can run another workspace's instead
([below](#another-workspaces-sign-in)). Web apps take the place of the separate Wrapper
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
| *App name* | New Window (⌘N) · Adopt Workspace (only on a workspace whose profile the window does not run) · Open in Browser · Copy Link · Reload (⌘R) · Close Window (⌘W) · Remove Web App… · Quit *name* (⌘Q) |
| Edit | Cut · Copy · Paste · Select All (⌘X / ⌘C / ⌘V / ⌘A) |
| Window | The Window menu every Sola application has |

- **Open in Browser** opens the app's current page as a Browser page
  beside it, on the same profile.
- **Copy Link** copies the address of the page in this window, since a web
  app has no location bar.
- **Close Window** closes this window of the app; **Quit** *name* closes
  all of them. Browser itself ends only when those were its last windows. Opening the app again brings back every window it had when you quit it, at the pages they showed, each on the profile it ran.

The page's own dialogs open in the app's window: JavaScript dialogs,
HTTP sign-in, camera and microphone, notifications, and file uploads. Its
notifications carry the app's name; clicking one brings its window
forward.

Links stay in the app's window while they stay on the app's site. The
site is the apex domain, the name registered under its public suffix, so
`app.slack.com` and `files.slack.com` are the same site, and `a.co.uk` and
`b.co.uk`, or `a.github.io` and `b.github.io`, are not.

A link you click to another site opens as a Browser page instead, unless
another web app starts on that site: then it opens as a window of that app,
on that app's profile, with its other windows in the bar. The same goes for a link that opens a new window (`target=_blank`,
⌘-click) from any page, a Browser page included, and for a link from outside
Browser: Mail, a terminal, Projects, `solactl open`. **Open in Browser** and the
launcher's Browser field always open a Browser page.

Redirects, such as the round trip of a sign-in, stay in the window. A popup
the page opens with `window.open` (a call's window, say) is the app's while
it is blank or on the app's site; on another site it is a Browser page, with
its address in sight.

A Browser page the app sent to another site (a link you clicked, or such a
popup) becomes a window of the app once it is back on the app's site: the
same window, page and sign-in, now under the app's name and in its group.
So a sign-in that leaves the site ("Continue with SSO", "Sign in with
Google") shows its own address while it is away, and ends in the app.

## Launch

Choose the app's name in the launcher. Its command is
`/bin/sola-browser --app <app_id>`. If Browser is running, the app's
window comes forward, or opens at its start page. Otherwise Browser
starts (with its saved pages) and opens it, or brings forward a window it
restored. Once Browser is up, launching the app again [opens another
window](#several-windows).

Browser's session keeps app windows as it keeps pages: after a restart
they come back as the app, in their place in the bar.

## Several windows

A web app works like Browser pages or Terminal: open as many windows onto it
as you like.

- The launcher opens another window instead of bringing one forward, and
  **New Window** (⌘N) in the app's menu does too. A new window starts at the
  app's start page: ⌘N's signed in as the window it was pressed in, the
  launcher's as the app ([below](#another-workspaces-sign-in)).
- One window is a row of its own, named for the app. With two or more on a
  workspace they group in the bar under the app's name, each named by its
  page's title. Super+Backtick steps through them.
- After a restart they all come back, each at its own page, in their place.
- A link to its site from another app or a Browser page opens here
  ([above](#the-apps-window)).

## Another workspace's sign-in

Each workspace has a Browser profile of its own
([Profiles](sola-browser.md#profiles)), and each window of a web app runs
one. A window the launcher opens runs the profile the app was made from;
one ⌘N opens runs the profile of the window it was pressed in. On a
workspace whose profile the window does not run, the app's menu has
**Adopt Workspace**:

- The window reloads at its page under the profile of the workspace
  showing: signed in as whoever is signed in to the site there, else at
  the site's sign-in.
- It keeps that profile wherever it goes, through a restart and a Quit,
  until another workspace's Adopt Workspace.
- Its links on the app's site, and its popups, stay on that profile.
- The item is not there while the window runs the profile of the workspace
  showing.

So one app signs in as two people on two workspaces: on the second, open
the app, choose **Adopt Workspace**, and sign in.

## Remove

**Remove Web App…** in the app's menu asks once (**Remove** / **Cancel**),
then takes the app out of the launcher, deletes its icon, and closes its
windows. The sign-in stays in the Browser profile.

**Settings → Applications → Web apps** lists them too, with the site
each starts on. Open one there to rename it (**Save**; its menus take the
new name) or **Remove** it (no question asked; its windows close, the
sign-in stays).

## Limits

- An app's window has no location bar, no find bar, no vault or
  downloads button, and no Developer Tools. **Open in Browser** for
  those. A right-click opens the page's right-click menu
  ([Browser](sola-browser.md#page-menu)) without **Wrap This Page…**,
  **Inspect Element**, or **Developer Tools**.
- A window the launcher opens, and a link from another app, start on the
  profile the app was wrapped from; only a window moves to another, by
  Adopt Workspace (and ⌘N from it keeps it).
- Clicking a sign-in button that leaves the site ("Sign in with Google")
  opens the sign-in as a Browser page, which joins the app when it comes
  back ([above](#the-apps-window)). The window you clicked it in still shows
  the sign-in: reload it, or close it.
- No per-app zoom, no badge counts on the icon, no way to change an app's
  start page or icon (its name changes in Settings, [above](#remove)), and
  no way to wrap a page into an app that already exists.
