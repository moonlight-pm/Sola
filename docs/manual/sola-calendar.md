# sola-calendar

Kit-native calendar. **Partial.** Launcher row is **Calendar** (`lucide/calendar`).

Month is the default view (Sunday-start, same as the menubar clock). Week and Day list the day’s events. **On This Computer** is always there. Optional accounts: Google Calendar, iCloud, generic CalDAV, and ICS/webcal URLs. Add as many of each as you want.

**Installed** `bus`+`settings`+`calendar` release 2026-09-08 (unsmoked).

## Use

- **‹ ›** and **Today** move the visible range. **1 / 2 / 3** switch Month / Week / Day. **t** jumps to today. **n** or **New Event** creates on the selected day.
- Click a day to inspect it. Double-click a day (or **New Event**) opens the editor. Save writes local events immediately; Google, iCloud, and CalDAV go through the provider. URL feeds are view-only.
- Click a calendar disc to show or hide it. Double-click sets the default calendar for new events.
- Accounts are **Settings → Calendar**. The Calendar sidebar lists them; it does not add or remove them.

## Google

In **Settings → Calendar**, paste a Google OAuth **Desktop** client ID (or set `SOLA_GOOGLE_CALENDAR_CLIENT_ID`), Save, then **+ Google** and **Sign in with Google**. Enable **Google Calendar API** and add this redirect:

`http://127.0.0.1:8765/oauth`

Sola does not ship a Cloud project. Tokens ride `Topic::CalendarConfig` (age-encrypted on disk).

## iCloud

**Settings → Calendar → + iCloud.** Apple ID + [app-specific password](https://appleid.apple.com). CalDAV talks to `caldav.icloud.com`. Repeating events show on each day in the window; editing or deleting one day of a series is not in this pass. Add more than one iCloud account with **+ iCloud** again.

## URL

**Settings → Calendar → + URL.** Paste an `https://` or `webcal://` ICS/iCal feed (public holidays, school calendars, …). `webcal://` is fetched as `https://`. Name is optional (host or the feed’s `X-WR-CALNAME` is used). URL calendars are **read-only**.

## CalDAV

**Settings → Calendar → + CalDAV.** Server URL, username, and password (often an app-specific password). Discovers event calendars on that host (Fastmail, Nextcloud, …).

## Files

| Path | Role |
|------|------|
| `~/.config/sola/calendar/settings.json` | View, default calendar |
| `~/.local/state/sola/calendar/store.json` | Calendars and events |
| `Topic::CalendarConfig` (bus, age-encrypted secrets) | Accounts — **Settings → Calendar** |

## Not in this pass

Invites, alerts, authenticated ICS URLs, drag-create, time zone picker, menubar clock showing these events.
