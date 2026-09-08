# sola-calendar — kit-native calendar

**Date:** 2026-09-08  
**Branch:** sola-calendar  
**Status:** implemented (partial) — first-pass kit app: month / week / day, local store, Google Calendar API, iCloud / generic CalDAV, ICS/webcal URLs. **Installed** `bus`+`settings`+`calendar` release 2026-09-08 (unsmoked)  
**Gaps:** no invites / RSVP; no alerts; Google needs a Desktop OAuth client ID (Sola does not ship a Cloud project); Apple/CalDAV repeating events are display-only (series edit later); URL feeds are read-only; no drag-resize; no timezone picker (local); menubar clock is still a date grid, not this app

## Goal

Ship `crates/sola-calendar`: a bog-standard kit calendar. See a month, open a day, create an event, connect Google Calendar and iCloud.

## Decisions (locked)

| Topic | Choice |
|---|---|
| UI stack | sola-kit / iced. Familiar calendar grammar (sidebar calendars, month default, week/day, inspector — not a modal) |
| Week start | Sunday (same as the menubar clock card) |
| Local | Always **On This Computer**. Events live in `~/.local/state/sola/calendar/store.json` |
| Google | Calendar API v3. Authorization Code + PKCE. Desktop OAuth client ID in Settings (env `SOLA_GOOGLE_CALENDAR_CLIENT_ID` or the client ID field). Redirect `http://127.0.0.1:8765/oauth`. Multiple Google accounts allowed |
| Apple | iCloud CalDAV (`https://caldav.icloud.com`) with Apple ID + app-specific password |
| URL | ICS / iCal / `webcal://` feed (GET, read-only). `webcal://` becomes `https://` |
| CalDAV | Generic CalDAV (Fastmail, Nextcloud, …): server URL + username + password; same PROPFIND/REPORT path as iCloud |
| Accounts | **Settings → Calendar.** Persistent `Topic::CalendarConfig` (Google OAuth client ID + any mix of account kinds). Calendar consumes it; local “On This Computer” stays in the app |
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
│  disc …  │  month grid / week / day     │ or connect  │
│ Google   │                              │             │
│ Apple    │                              │             │
└──────────┴──────────────────────────────┴─────────────┘
```

Today is an accent disc on the day number. Calendar color is a 10px disc, not a thick stripe. Empty first-run: the month is already the product; the inspector invites **New Event**. Accounts are **Settings → Calendar**.

## Persistence

| Path | Role |
|---|---|
| `~/.config/sola/calendar/settings.json` | View, selected day, default calendar |
| `~/.local/state/sola/calendar/store.json` | Calendars + events (visibility). Accounts live on `Topic::CalendarConfig` |
