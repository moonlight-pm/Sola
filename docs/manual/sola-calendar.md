# Calendar

Calendar is Sola's native calendar app, for events on this computer and
in your online calendars. Its launcher row is **Calendar**
(`lucide/calendar`).

Month is the default view (Sunday-start, the same as the menubar clock). Week and Day list the day’s events. **On This Computer** is always there. Accounts are optional: Google Calendar, iCloud, generic CalDAV, and ICS/webcal URLs. Add as many of each as you want.

## Use

- **‹ ›** and **Today** move the visible range. **1 / 2 / 3** switch Month / Week / Day. **t** jumps to today. **n** or **New Event** creates an event on the selected day.
- Click a day to inspect it in the right pane (always open; drag its splitter to resize). Double-click a day (or **New Event**) to open the editor there: title first, then date / time / all-day, then calendar, location, and notes. Escape discards an unsaved draft and returns to the day. Save writes local events immediately; Google, iCloud, and CalDAV events go through the provider. URL feeds are view-only.
- Click a calendar disc (or the name) to show or hide its events on the board. Double-click sets the default calendar for new events. The pencil gives the calendar an alias (the field is selected on open) and shows a colour swatch like Browser tab groups; the original provider name appears only while editing. Eye-off in that row hides the calendar from the sidebar and turns it off.
- Under the calendars list, **Today** and **Tomorrow** list the next events (click one to open it; **+N more** opens that day).
- Drag the left rail divider to widen the sidebar.
- Accounts, re-showing a hidden calendar, and editing its label or colour are in **Settings → Calendar**.
- **Remind me** is at the bottom of an open event (including read-only ones). Checking it adds one reminder: 30 minutes before, or 9:00 on the morning of an all-day event. The chips add the other quick sets (at time, 5, 10, 15, and 30 minutes, 1 and 2 hours, 1 day, 1 week; for all-day events: morning of, 1 day before, 2 days before, 1 week before). × drops one. The last × unchecks Remind me.

## Google

**Settings → Calendar → + Google → Sign in with Google.** Browser opens; pick the Google account. Tokens travel on `Topic::CalendarConfig` (age-encrypted on disk). Add another Google account with **+ Google** again.

Sola ships a Desktop OAuth client (Google still requires that client’s secret on the token POST). The first sign-in may warn that the app isn’t verified; choose Advanced → continue. The redirect is `http://127.0.0.1:8765/oauth`. Override the client with `SOLA_GOOGLE_CALENDAR_CLIENT_ID` and `SOLA_GOOGLE_CALENDAR_CLIENT_SECRET` if you use your own Cloud project.

## iCloud

**Settings → Calendar → + iCloud.** Enter your Apple ID and an [app-specific password](https://appleid.apple.com). CalDAV talks to `caldav.icloud.com`. Repeating events show on each day in the window. Editing or deleting one day of a series is not supported. Add more than one iCloud account with **+ iCloud** again.

## URL

**Settings → Calendar → + URL.** Paste an `https://` or `webcal://` ICS/iCal feed (public holidays, school calendars, …). `webcal://` is fetched as `https://`. The name is optional (the host or the feed’s `X-WR-CALNAME` is used). URL calendars are **read-only**.

## CalDAV

**Settings → Calendar → + CalDAV.** Enter the server URL, username, and password (often an app-specific password). Calendar discovers the event calendars on that host (Fastmail, Nextcloud, …).

## Reminders

Calendar has to be open. When a reminder comes due, a card stays on the desk until you dismiss it (×) or open it. Opening it shows that event. If Calendar was quit, launching it still shows a reminder whose event ended within the last 12 hours.

## Files

| Path | Role |
|------|------|
| `~/.config/sola/calendar/settings.json` | View, default calendar, sidebar width, inspector width |
| `~/.local/state/sola/calendar/store.json` | Calendars and events |
| `~/.local/state/sola/calendar/reminders.json` | Reminders for events on this computer |
| `Topic::CalendarConfig` (bus, age-encrypted secrets) | Accounts and calendar shelf (hidden / alias / colour), set in **Settings → Calendar** |

## Limits

Calendar does not support invites, provider alarms (Google / iCloud / CalDAV), authenticated ICS URLs, drag-to-create, a time zone picker, or showing these events in the menubar clock.
