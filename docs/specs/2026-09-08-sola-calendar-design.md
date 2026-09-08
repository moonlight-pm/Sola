# sola-calendar — kit-native calendar

**Date:** 2026-09-08  
**Branch:** sola-calendar  
**Status:** implemented (partial) — first-pass kit app: month / week / day, local store, Google Calendar API + iCloud CalDAV  
**Gaps:** no invites / RSVP; no alerts; no generic CalDAV (Fastmail / Nextcloud); Google needs a Desktop OAuth client ID (Sola does not ship a Cloud project); Apple repeating events are display-only (series edit later); no drag-resize; no timezone picker (local); not in Settings; menubar clock is still a date grid, not this app

## Goal

Ship `crates/sola-calendar`: a bog-standard kit calendar. See a month, open a day, create an event, connect Google Calendar and iCloud.

## Decisions (locked)

| Topic | Choice |
|---|---|
| UI stack | sola-kit / iced. Familiar calendar grammar (sidebar calendars, month default, week/day, inspector — not a modal) |
| Week start | Sunday (same as the menubar clock card) |
| Local | Always **On This Computer**. Events live in `~/.local/state/sola/calendar/store.json` |
| Google | Calendar API v3. Authorization Code + PKCE. Desktop OAuth client ID is stored in-app (env `SOLA_GOOGLE_CALENDAR_CLIENT_ID` or the Connect field). Redirect `http://127.0.0.1:8765/oauth` |
| Apple | iCloud CalDAV (`https://caldav.icloud.com`) with Apple ID + app-specific password |
| Accounts | In-app (Calendars sidebar). Not Settings v1 |
| Secrets | Age-encrypted on disk via `sola_core::Encrypted` (`~/.config/sola/key`) |
| Process | In-process worker (mail/spotify pattern), not a bus daemon |
| Recurrence | Google: `singleEvents=true` instances. Apple: RRULE expanded in the visible window; series edit is later |
| Location | `crates/sola-calendar`. App id `sola-calendar`. Launcher **Calendar** (`lucide/calendar`) |

## Non-goals (v1)

- Meeting invites, guests, RSVP
- Alarms / notifications
- Attachments
- Generic CalDAV hosts (the Apple path is iCloud-shaped)
- Settings panel for accounts
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

Today is an accent disc on the day number. Calendar color is a 10px disc, not a thick stripe. Empty first-run: the month is already the product; the inspector invites **New Event** or connect Google / iCloud.

## Persistence

| Path | Role |
|---|---|
| `~/.config/sola/calendar/settings.json` | View, selected day, default calendar, Google client id |
| `~/.local/state/sola/calendar/store.json` | Calendars, events, accounts (secrets encrypted) |
