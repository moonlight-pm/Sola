# Calendar

Calendar (`sola-calendar`, in Oath's `pkg:sola`) is Sola's own calendar,
for events on this computer and in your online calendars, in one window.
Its launcher row is **Calendar** (`lucide/calendar`).

Month is the default view; Week and Day list each day's events. Weeks
start on Sunday, as in the top row's clock. **On This Computer** is always
there. Accounts are optional: Google Calendar, iCloud, generic CalDAV, and
ICS or webcal feeds, as many of each as you want, added in
**Settings → Calendar**.

## The board

- The bar over the board names the range shown (so does the window's
  title). At its right: **Month / Week / Day**, **‹ Today ›**, and **+**
  for a new event on the selected day.
- The month is six weeks, a hairline apart; today's date sits on an
  accent circle. Events are chips in their calendar's colour (a bar and a
  light fill); a day at least 110 px wide shows their times. A day with
  more events than fit ends in **+N more**. Week lists each day's events
  as cards; Day lists them with their times and places.
- Click a day to select it. Double-click a day (or press **+**) to start
  a new event there.

## Layouts

What shows beside the board depends on the window's width, so Calendar
works in a Full zone and in a half tile alike.

- From 700 px wide, the calendars sidebar is at the left. Drag its divider
  to widen it.
- When at least 900 px are left beside the sidebar, the inspector is a
  pane at the right: the selected day, or the open event. Drag its
  divider to resize it.
- Narrower (a half tile), the board fills the window and an open event
  shows on a card over the board's right edge; × or Escape closes it.

Calendar remembers the view and both widths in
`~/.config/sola/calendar/settings.json`.

## Events

- The editor has the title first, then the date, the times, and **All
  day** (and **Through** for the last day of an event over several days),
  then the calendar, the location, and notes. **Save** writes it.
  **Delete** removes an event you can edit. Escape discards an unsaved
  draft and returns to the day.
- Events on this computer are saved at once; Google, iCloud, and CalDAV
  events are saved through the provider. Feeds are read-only, and so are
  the repeating events of iCloud and CalDAV calendars; a read-only event
  opens for reading.
- A location that is a link opens in Browser.
- Online calendars sync when Calendar starts, when you move to another
  range, and on **Calendar → Refresh** (⌘R). Nothing syncs on a timer.

## The sidebar

- **Calendars** lists the calendars shown in the sidebar. Click a
  calendar's disc (or its name) to show or hide its events on the board.
  Double-click one to make it the default for new events.
- Its pencil edits the calendar in place: a name of your own (the field
  is selected when it opens; the provider's name shows under it while you
  edit), a colour swatch that opens a picker, and eye-off, which hides the
  calendar from the sidebar and turns it off. Re-showing a hidden calendar
  is in **Settings → Calendar → Calendars**.
- Under the calendars, **Today** and then **Tomorrow** list their events,
  up to six each (click one to open it; **+N more** selects that day).
  Select another day on the board and the second list is that day.

## The top row's clock

The clock's panel in the top row lists today's events from the calendars
shown; click one to open it in Calendar, which starts if it is not
running (see [the shell](sola-shell.md)). Calendar sends the panel today
and the next seven days whenever they change, through the persistent bus
topic `CalendarAgenda`, so the panel keeps the last list after Calendar
quits.

## Keys

| Key | Action |
|-----|--------|
| **1** / **2** / **3** | Month / Week / Day |
| **t** | Today |
| **n** | New event on the selected day |
| **←** / **→** | Previous / next month, week, or day |
| Escape | Close the open event, or leave editing a calendar |

| Menu | Items |
|---|---|
| Calendar | New Event (⌘N), Refresh (⌘R), Quit Calendar (⌘Q) |
| View | Month, Week, Day, Go to Today |
| Window | The Window menu every Sola application has |

## Opening an invite

Open a `.ics` file (**Open** on an invite in Mail,
`solactl open <file.ics>`, or `sola-calendar <file.ics>`) and Calendar
comes forward with the event as a new draft, on its date. The **Calendar** row lists
only calendars you can edit and starts on your default. Change anything,
then **Save** adds the event there; Escape discards it. A line under the
title names the file.

- Only one Calendar runs. Opening a file while Calendar is open hands it
  to that window (over `$XDG_RUNTIME_DIR/sola/calendar.sock`).
- A repeating event is added once: its next date in the coming year (its
  first if the series has ended). The line under the title says which.
- A file with several events adds the first; the line says how many were
  left out.
- A cancellation (`METHOD:CANCEL` or `STATUS:CANCELLED`) adds nothing and
  says so.
- An all-day event over several days keeps its length (the **Through**
  row shows the last day).
- Outlook's Windows time-zone names (`Pacific Standard Time`, …) are read
  as their real zones, not as local time.

## Reminders

**Remind me** is at the bottom of an open event, read-only ones included.
Checking it adds one reminder: 30 minutes before, or 9:00 on the morning
of an all-day event. The chips add others (at the time, 5, 10, 15, and
30 minutes, 1 and 2 hours, 1 day, 1 week before; for all-day events the
morning of, 1 day, 2 days, and 1 week before). × drops one; the last ×
unchecks Remind me.

Reminders are kept on this computer, so a provider's sync does not touch
them. Calendar has to be running to deliver one. When a reminder comes
due, a card stays on the desk until you dismiss it (×) or open it;
opening it shows the event. Launched after it was quit, Calendar still
delivers a reminder whose event ended within the last 12 hours.

## Google

**Settings → Calendar → Add account → Google → Sign in with Google.**
Browser opens Google's sign-in; pick the account. Add another Google
account the same way.

Sola does not ship a Google OAuth client. Sign-in needs a Desktop OAuth
client of your own Google Cloud project, with the Calendar API: set
`SOLA_GOOGLE_CALENDAR_CLIENT_ID` and `SOLA_GOOGLE_CALENDAR_CLIENT_SECRET`
in the environment Settings and Calendar start with (see
[the session manager](session.md#environment-and-working-directory)).
Without them, sign-in stops with "a Google OAuth Desktop client ID is
required". Google may warn that the app is not verified; choose Advanced,
then continue. The sign-in returns to `http://127.0.0.1:8765/oauth`.

## iCloud

**Settings → Calendar → Add account → iCloud.** Enter your Apple ID and
an [app-specific password](https://appleid.apple.com). Calendar talks
CalDAV to `caldav.icloud.com`. Repeating events show on each of their
days and are read-only. Add more than one iCloud account the same way.

## CalDAV

**Settings → Calendar → Add account → CalDAV.** Enter the server URL, the
username, and the password (often an app-specific password). Calendar
finds the event calendars on that server (Fastmail, Nextcloud, …).
Repeating events are read-only, as on iCloud.

## Feeds

**Settings → Calendar → Add account → Calendar URL.** Paste an `https://`
or `webcal://` ICS feed (public holidays, school calendars, …).
`webcal://` is fetched as `https://`. The name is optional (the feed's
`X-WR-CALNAME`, else its host). Feeds are **read-only**.

## Files

| Path | What |
|------|------|
| `~/.config/sola/calendar/settings.json` | View, default calendar, sidebar and inspector widths |
| `~/.local/state/sola/calendar/store.json` | Calendars and events |
| `~/.local/state/sola/calendar/reminders.json` | Reminders, for events of every calendar |
| `~/.config/sola/state.yaml` (`CalendarConfig`) | Accounts, and each calendar's name, colour, and whether it is hidden, set in **Settings → Calendar**; passwords and tokens age-encrypted with `~/.config/sola/key` |
| `~/.config/sola/calendar/agenda.yaml` (`CalendarAgenda`) | Today and the next seven days, for the clock's panel |

## Limits

- **t**, **n**, **1**–**3**, and the arrow keys act even while you type
  in a field: an **n** typed in an event's title starts a new, empty
  event in its place.
- No replies to invites (accept / decline), and no adding a whole
  repeating series from a file.
- No provider alarms (Google, iCloud, CalDAV), no feeds that need a
  password, no drag-to-create, and no time-zone picker.
- An event over several days shows as a chip on each day, not one bar
  across them.
