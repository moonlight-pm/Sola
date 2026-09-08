# sola-calendar — kit-native calendar

**Date:** 2026-09-08  
**Branch:** sola-calendar  
**Status:** implemented (partial) — first-pass kit app: month / week / day, local store, Google Calendar API, iCloud / generic CalDAV, ICS/webcal URLs. **Installed** `bus`+`settings`+`calendar` release 2026-09-08 (unsmoked)  
**Gaps:** no invites / RSVP; no alerts; Apple/CalDAV repeating events are display-only (series edit later); URL feeds are read-only; no drag-resize; no timezone picker (local); menubar clock is still a date grid, not this app

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
- Alarms / notifications
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
