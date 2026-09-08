# sola-calendar

Kit-native calendar. **Partial.** Launcher row is **Calendar** (`lucide/calendar`).

Month is the default view (Sunday-start, same as the menubar clock). Week and Day list the day’s events. **On This Computer** is always there. Google Calendar and iCloud are optional accounts.

**Installed** `calendar`+`shell` release 2026-09-08 (unsmoked).

## Use

- **‹ ›** and **Today** move the visible range. **1 / 2 / 3** switch Month / Week / Day. **t** jumps to today. **n** or **New Event** creates on the selected day.
- Click a day to inspect it. Double-click a day (or **New Event**) opens the editor. Save writes local events immediately; Google and iCloud go through the provider.
- Click a calendar disc to show or hide it. Double-click sets the default calendar for new events.
- **Accounts → Google Calendar…** signs in with OAuth (sola-browser). **iCloud Calendar…** takes an Apple ID and an [app-specific password](https://appleid.apple.com). Hover **×** on a connected account disconnects it.

## Google

Sola does not ship a Google Cloud OAuth client. Create a **Desktop** client, enable **Google Calendar API**, and add this redirect:

`http://127.0.0.1:8765/oauth`

Paste the client ID in the connect pane, or set `SOLA_GOOGLE_CALENDAR_CLIENT_ID`. Tokens land encrypted under `~/.local/state/sola/calendar/store.json`.

## iCloud

Apple ID + app-specific password. CalDAV talks to `caldav.icloud.com`. Repeating events show on each day in the window; editing or deleting one day of a series is not in this pass.

## Files

| Path | Role |
|------|------|
| `~/.config/sola/calendar/settings.json` | View, default calendar, Google client ID |
| `~/.local/state/sola/calendar/store.json` | Calendars, events, accounts (secrets age-encrypted) |

## Not in this pass

Invites, alerts, generic CalDAV (Fastmail / Nextcloud), Settings panel accounts, drag-create, time zone picker, menubar clock showing these events.
