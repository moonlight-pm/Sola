# Web apps

A web app is a website that Browser runs as an application of its own: a
launcher entry, a window with the app's name and menus, and no location
bar. It runs on the Browser profile it was made from, so it is signed in
wherever that profile is. The examples below use Slack.

## Make one

Open the site in Browser, signed in, at the page the app should start on.
Then choose **Browser → Wrap This Page…**, or **Wrap This Page…** in the
page's right-click menu. Both appear on `http://` and `https://` pages
only.

A sheet asks for the app's **Name**, suggested from the page title (the
site's part of it: *Slack* from "general - Acme - Slack"). **Wrap** turns
the page into the app in place: no reload and no new sign-in. Its window
takes the app's name and menus, its row in the shell's tab bar says the
app's name, and the launcher lists it.

- The app id is generated (`web-` and a UUID); there is nothing to type.
- The icon is the page's favicon, saved at
  `~/.local/share/sola/icons/<app_id>.png`. A site without one gets a globe.
- The app starts at the page's address at the moment you wrapped it.

## The app's window

The titlebar shows the app's name and its menus:

| Menu | Items |
|---|---|
| *App name* | Open in Browser · Reload (Super+R) · Close Window (Super+W) · Remove Web App… · Quit *name* (Super+Q) |
| Edit | Cut · Copy · Paste · Select All (Super+X / C / V / A) |
| Window | The same Window menu as every Sola app |

**Open in Browser** opens the app's current page as a Browser page beside
it.

Links stay in the app's window while they stay on the app's site
(another subdomain counts as the same site). A link you click to another
site opens as a Browser page instead. Redirects, such as the round trip of
a sign-in, stay in the window.

## Launch

Choose the app's name in the launcher. Its command is
`sola-browser --app <app_id>`. If Browser is running, the app's window
comes forward, or opens at its start page; otherwise Browser starts and
opens it. Launching it again brings the same window forward.

Browser's session keeps app windows like pages: after a restart they
come back as the app, in their place in the tab bar.

## Remove

**Remove Web App…** in the app's menu asks once, then takes the app out of
the launcher and closes its windows. The sign-in stays in the Browser
profile.

Settings → Applications does not list web apps and cannot edit them.

## Limits

- The app always runs on the profile it was wrapped from; it cannot move
  to another.
- Clicking a sign-in button that leaves the site ("Sign in with Google")
  opens the sign-in as a Browser page. Signing in there signs the app in
  too, since it is the same profile; reload the app afterwards.
- No per-app zoom, no badge counts on the icon, and no way to wrap a page
  into an app that already exists.
