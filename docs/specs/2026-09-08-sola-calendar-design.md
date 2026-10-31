# sola-calendar — kit-native calendar

**Date:** 2026-09-08  
**Branch:** sola-calendar  
**Status:** implemented (partial) — first-pass kit app: month / week / day, local store, Google Calendar API, iCloud / generic CalDAV, ICS/webcal URLs. **Installed** `bus`+`settings`+`calendar` release 2026-09-08 (unsmoked). Reminders **smoked** `shell`+`calendar` release 2026-10-12. On `next` 2026-10-26: the board in Next's look (`ui/board.rs`), a layout by window width (the inspector floats as a card over the board in a half tile — still not a modal), `Topic::CalendarAgenda` for the menubar clock, and Calendar in `pkg:sola` on Oath; applied and smoked on canto  
**Gaps:** no RSVP (an opened `.ics` becomes a new draft — 2026-10-26; one date of a series); no provider alarm sync (reminders are local); Apple/CalDAV repeating events are display-only (series edit later); URL feeds are read-only; no drag-resize; no timezone picker (local); an event over several days is a chip per day, not one bar; reminder delivery needs Calendar running

## Goal

Ship `crates/sola-calendar`: a bog-standard kit calendar. See a month, open a day, create an event. Accounts (Google, iCloud, CalDAV, ICS/webcal URL) live in **Settings → Calendar**.

## Decisions (locked)

| Topic | Choice |
|---|---|
| UI stack | sola-kit / iced. Familiar calendar grammar (sidebar calendars, month default, week/day, inspector — not a modal) |
| Week start | Sunday (same as the menubar clock card) |
| Local | Always **On This Computer**. Events live in `~/.local/state/sola/calendar/store.json` |
| Google | Calendar API v3. Authorization Code + PKCE. Shipped Desktop OAuth client (`GOOGLE_CALENDAR_CLIENT_ID` + secret; env `SOLA_GOOGLE_CALENDAR_CLIENT_ID` / `SOLA_GOOGLE_CALENDAR_CLIENT_SECRET` override). Google's token endpoint requires the Desktop secret even with PKCE. Redirect `http://127.0.0.1:8765/oauth`. Multiple Google accounts allowed |
| Apple | iCloud CalDAV (`https://caldav.icloud.com`) with Apple ID + app-specific password |
| URL | ICS / iCal / `webcal://` feed (GET, read-only). `webcal://` becomes `https://` |
| CalDAV | Generic CalDAV (Fastmail, Nextcloud, …): server URL + username + password; same PROPFIND/REPORT path as iCloud |
| Accounts | **Settings → Calendar.** Persistent `Topic::CalendarConfig` (any mix of account kinds; Google uses the shipped client; `calendars` shelf for hidden / alias / colour). Empty shelf on an accounts-only publish means leave prefs. Calendar consumes it; local “On This Computer” stays in the app |
| Secrets | Age-encrypted on disk via `sola_core::Encrypted` (`~/.config/sola/key`) |
| Process | In-process worker (mail/spotify pattern), not a bus daemon |
| Recurrence | Google: `singleEvents=true` instances. Apple: RRULE expanded in the visible window; series edit is later |
| Location | `crates/sola-calendar`. App id `sola-calendar`. Launcher **Calendar** (`lucide/calendar`) |

## Non-goals (v1)

- Meeting invites, guests, RSVP
- Provider alarms (VALARM / Google reminders). Local reminders are in — see below
- Attachments
- Authenticated ICS URLs (plain GET only)
- Menubar clock showing events from this app
- Drag-create / drag-resize
- Time zone picker (display in local; store UTC / all-day dates)

## Layout

```
┌──────────┬──────────────────────────────┬─────────────┐
│ Calendars│  ‹ ›  Today   September 2026 │ Inspector   │
│  disc On │              Month Week Day +│ day / event │
│  disc …  │  month grid / week / day     │             │
│ Today    │                              │             │
│ Tomorrow │                              │             │
└──────────┴──────────────────────────────┴─────────────┘
```

Today is an accent disc on the day number, pinned to the top-start of the cell (not centered). Weekday headers use full names (Sunday–Saturday). Month cells list every event that fits; “+N more” only when the rest would overflow. The calendars rail is a kit split (drag to widen): calendars, then Today / Tomorrow quickview. Accounts stay in **Settings → Calendar** (not the calendar sidebar). Hide-from-sidebar also deactivates; Settings re-shows and edits label / colour. The inspector is always open on a second kit split (no Close); read-only events are a reading pane; writable ones a caption-row editor (title, when, calendar disc, location, notes). Location/notes HTML `<a href>` and bare URLs are kit prose links. Calendars can take a local alias and colour (sidebar pencil + swatch, same pattern as browser tab groups); the original name shows only while editing; sync does not overwrite them. Calendar color is a 10px disc, not a thick stripe. Empty first-run: the month is already the product; the inspector shows the selected day. Account connect lives in **Settings → Calendar**.

## Persistence

| Path | Role |
|---|---|
| `~/.config/sola/calendar/settings.json` | View, default calendar, sidebar width, inspector width |
| `~/.local/state/sola/calendar/store.json` | Calendars + events (visibility / hidden). Accounts and shelf prefs live on `Topic::CalendarConfig` |
| `~/.local/state/sola/calendar/reminders.json` | Local reminder offsets per event id. Sync does not rewrite this file |

## Reminders

The inspector (read-only and editable) has **Remind me**. Checking it adds one reminder. Quick sets add more; × removes one. Timed presets: at time, 5 / 10 / 15 / 30 minutes, 1 hour, 2 hours, 1 day, 1 week. The first check is 30 minutes before. All-day presets: morning of (9:00 local), 1 day before, 2 days before, 1 week before. The first check is morning of.

Offsets are minutes before the anchor (event start, or 9:00 local on the start date when all-day). They are stored by event id, including read-only and remote events. A new event's reminders are written when the save returns the provider id.

Calendar polls while it is open and emits `AppNotification` (`source` Calendar, `tag` `sola-persist:<event>:<offset>`, `url` = event id). The shell keeps that card up until × or click. Click raises Calendar and opens the event. If Calendar was quit, opening it still delivers a reminder whose event ended within the last 12 hours, and a reminder whose event is outside the visible sync window still fires from the last known start. Hidden calendars do not fire.
