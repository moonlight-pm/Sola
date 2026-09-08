# sola-calendar

Kit-native calendar. **Partial.** Launcher row is **Calendar** (`lucide/calendar`).

Month is the default view (Sunday-start, same as the menubar clock). Week and Day list the day’s events. **On This Computer** is always there. Optional accounts: Google Calendar, iCloud, generic CalDAV, and ICS/webcal URLs. Add as many of each as you want.

**Installed** `bus`+`settings`+`calendar` release 2026-09-08 (unsmoked).

## Use

- **‹ ›** and **Today** move the visible range. **1 / 2 / 3** switch Month / Week / Day. **t** jumps to today. **n** or **New Event** creates on the selected day.
- Click a day to inspect it in the right pane (always open; drag its splitter to resize). Double-click a day (or **New Event**) opens the editor there: title first, then date / time / all-day, then calendar, location, and notes. Escape discards an unsaved draft back to the day. Save writes local events immediately; Google, iCloud, and CalDAV go through the provider. URL feeds are view-only.
- Click a calendar disc (or the name) to show or hide its events on the board. Double-click sets the default calendar for new events. The pencil aliases the label (field selects on open) and shows a colour swatch like browser tab groups; the original provider name appears only while editing. Eye-off in that row hides the calendar from the sidebar and turns it off.
- Under the calendars list, **Today** and **Tomorrow** list the next events (click one to open it; **+N more** opens that day).
- Drag the left rail divider to widen the sidebar.
- Accounts, re-showing a hidden calendar, and editing its label or colour are **Settings → Calendar**.

## Google

**Settings → Calendar → + Google → Sign in with Google.** The browser opens; pick the Google account. Tokens ride `Topic::CalendarConfig` (age-encrypted on disk). Add another Google account with **+ Google** again.

Sola ships a Desktop OAuth client (Google still requires that client’s secret on the token POST). First sign-in may warn the app isn’t verified — Advanced → continue. Redirect is `http://127.0.0.1:8765/oauth`. Override with `SOLA_GOOGLE_CALENDAR_CLIENT_ID` and `SOLA_GOOGLE_CALENDAR_CLIENT_SECRET` if you use your own Cloud project.

## iCloud

**Settings → Calendar → + iCloud.** Apple ID + [app-specific password](https://appleid.apple.com). CalDAV talks to `caldav.icloud.com`. Repeating events show on each day in the window; editing or deleting one day of a series is not in this pass. Add more than one iCloud account with **+ iCloud** again.

## URL

**Settings → Calendar → + URL.** Paste an `https://` or `webcal://` ICS/iCal feed (public holidays, school calendars, …). `webcal://` is fetched as `https://`. Name is optional (host or the feed’s `X-WR-CALNAME` is used). URL calendars are **read-only**.

## CalDAV

**Settings → Calendar → + CalDAV.** Server URL, username, and password (often an app-specific password). Discovers event calendars on that host (Fastmail, Nextcloud, …).

## Files

| Path | Role |
|------|------|
| `~/.config/sola/calendar/settings.json` | View, default calendar, sidebar width, inspector width |
| `~/.local/state/sola/calendar/store.json` | Calendars and events |
| `Topic::CalendarConfig` (bus, age-encrypted secrets) | Accounts and calendar shelf (hidden / alias / colour) — **Settings → Calendar** |

## Not in this pass

Invites, alerts, authenticated ICS URLs, drag-create, time zone picker, menubar clock showing these events.
