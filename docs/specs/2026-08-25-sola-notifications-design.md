# Sola notifications

**Date:** 2026-08-25  
**Status:** **Frozen** — implemented on `sola-browser` (KenHerbert desk card 2026-08-27) and `sola-wrapper` (smoked 2026-08-29). Pile UX (count on the bell / no Clear / unseen accent) **installed** `shell` release 2026-09-03. Grouped pile (no cap 20; ≤4 list as rows; tag replace) + Super+Tab unseen count marks (bell/focus ack) **installed** `kit`+`shell` release 2026-09-03.  
**Related:** [design language](../manual/design-language.md); [shell iced](2026-05-22-sola-shell-iced-port-design.md); [workspaces](2026-08-13-sola-agent-terminal-design.md)

## Intent

The menubar toast (`Topic::AppToast`) is chrome feedback: same 13pt face
as menus, 28px bar, 5s, no click. That is the wrong object for “pay
attention.” Notifications occupy the desk, carry identity, and leave a
missed pile. Browser `Notification` leaves the page and becomes the same
object.

## Two objects

| Object | Job | Surface |
|--------|-----|---------|
| **Whisper** | Shell talking about itself | Menubar toast (`AppToast`) |
| **Notification** | An event you would miss if you were not staring at the bar | Desk card + missed pile |

Whispers stay: `Opening Terminal…`, screenshot path, launch failure,
process exit. Notifications: Workspaces done-while-unfocused, web
`Notification`, later mail that is not the unread chip.

## Product rules

| Rule | Choice |
|------|--------|
| Card | Graphite kit popover chrome. Icon + source (chrome, muted) / title (`ui_medium`) / one line body. Quiet ×. |
| Origin | Tight overlay under the menubar, trailing edge with the clock cluster. Enters by dropping out of the bar (~180ms ease-out). |
| Click | Raises the source app (and the tab, for browser). Removes from live and pile. |
| × | Dismiss without raising. Not missed. |
| Expire | ~6s with no interaction → retract into the bar → missed pile. |
| Stack | Max 3 live cards; older live overflow goes straight to the pile. |
| Pile | Menubar `lucide/bell` + pile count in the right cluster (same glyph+numeral rhythm as the mail unread chip). Hidden at 0. Accent only while the pile is unseen; **clicking the bell returns it to normal chrome** (panel may stay open). Click opens a list under the chip (menu overlay). Session-only; **no item cap**. Overlay sizes to **visible groups**, up to the usable area under the menubar; the list scrolls if it still overflows. |
| Pile grouping | Missed items group by `app_id`. Up to 4 from one app list as rows (bell count matches). Five or more collapse to an app header + count; click to expand (newest 30; “N more”). Header × dismisses that app’s pile, not a global Clear. Same (`app_id`, `tag`) replaces in the pile too. Super+Tab does **not** drain the pile. |
| Pile click | Same as card click (raise source) and closes the panel. Pile × dismisses one row. No Clear. |
| Switcher mark | Super+Tab shows a kit `count_mark` on the app icon: **unseen** live + pile for that `app_id`. Opening the pile or raising/focusing the app (not FFM hover) marks those items seen — the pile stays until × / activate. A later notification badges again. Mail uses `MailStatus` inbox unread (not the pile). Hidden at 0; `99+` at 100. Super+Tab does **not** drain the pile. |
| Replace | Same `app_id` + `tag` replaces an in-flight banner **and** a missed pile row. |
| Keyboard | Overlay does **not** steal focus. Pile is a menu panel (Escape dismisses). |
| Sound | Not this slice. |
| Actions | `Notification.actions` not this slice. |
| D-Bus | Not this slice. First-party bus + browser JS intercept. `org.freedesktop.Notifications` later if other Linux apps need a daemon. |

## Architecture

```
app / browser
  → Topic::AppNotification   (ephemeral)
  → sola-shell
       live overlay "notify" (tight Frame, parked 2×2)
       missed pile (menubar chip + menu panel)
  click
  → raise app
  → Topic::NotificationActivate  (ephemeral; browser selects the tab)
```

`TopicKind` variants **append** after `MailStatus` (postcard Subscribe
stability).

```
AppNotification { id, app_id, source, title, body, tag, tab_id, url }
NotificationActivate { id, app_id, tab_id, url }
```

Shell overlay: fifth daemon window (`WindowKind::Notify`), same park /
live-Resized / Composition dance as menu / launcher. Live Frame is the
card stack rect (not the full usable area) so empty space click-through
is a non-issue. Never `Focus` this overlay.

## Browser

Sola is the DE — Chromium’s Linux libnotify path has no daemon here.

- Inject `window.Notification` in every frame (same console-bridge as clipboard).
- `requestPermission()` → kit dialog in chrome → persist per origin in
  `profiles/<uuid>/notifications.json`.
- `new Notification(title, { body, tag })` with `permission === "granted"`
  → helper IPC → chrome emits `AppNotification` (`app_id=sola-browser`,
  `source` = host, `tab_id` set). The wrap must **not** construct
  Chromium’s native `Notification` (or `ServiceWorkerRegistration.showNotification`):
  that paints an in-page banner over the web view. The dummy must **not**
  use `Notification.prototype` (`title`/`body` accessors throw). Persist
  and look up grants with a canonical origin (`https://host`, not
  `https://host/`). The only surface is the shell desk card (tight overlay,
  screen top-right under the menubar).
- `NotificationActivate` with `tab_id` → `SetActiveTab`.

**Wrapper** (`sola-wrapper <id>`): same CEF inject and helper IPC. Chrome drains the queue, shows the Allow / Block overlay, and emits `AppNotification` with `app_id` = the wrapper id (so click raises Slack, not Browser). Grants live in `~/.config/sola/wrapper/<id>/notifications.json`.

Denied / default: constructor is a no-op. Permission `"default"` until
the user chooses.

## Out of scope

Do Not Disturb, persistence across session, action buttons, sound,
image/icon from the page, `org.freedesktop.Notifications`, promoting
remaining `AppToast` senders (launch fail / exit). Pile grouping by
`app_id` is in.

## Implementation status

| Item | Status |
|------|--------|
| Freeze | **this document** |
| Bus topics | **done** (`AppNotification`, `NotificationActivate`) |
| Shell HUD + pile | **done** (grouped; ≤4 list as rows; 5+ collapse; tag replace; Super+Tab unseen count mark) |
| Browser intercept + permission | **done** (no Native ctor; dummy must not inherit `Notification.prototype`; origin keys canonicalized) |
| Wrapper intercept + permission | **smoked** 2026-08-29 (same inject; emit with wrapper `app_id`; grants under wrapper data dir) |
| Workspaces done → notification | **done** (title `{project} · {tab}`, body `grok is done`; waiting is `needs attention`) |
| Dogfood | KenHerbert Allow → displayed + top-right desk card (`install browser` 2026-08-27). Workspaces cards on this merge. |

## Decision log

| Date | Choice |
|------|--------|
| 2026-08-25 | Split whisper (menubar `AppToast`) vs notification (desk card) |
| 2026-08-25 | Drop-from-bar + missed-pile chip; click raises source |
| 2026-08-25 | Browser via JS intercept, not a D-Bus notification server |
| 2026-08-25 | No action buttons / sound in v1 |
| 2026-09-02 | Pile cap 20 (drop oldest); no Clear; chip highlight returns on click; panel grows to usable height |
| 2026-09-03 | Count on the bell (same glyph+numeral rhythm as mail unread) |
| 2026-09-02 | Bell accent is unseen-only; click acknowledges (normal menubar fg) |
| 2026-09-03 | Drop pile cap 20. Group by app; overlay height follows collapsed groups. Super+Tab count mark (live+pile; Mail unread). Raise does not drain. |
| 2026-09-03 | Switcher mark is unseen attention, not pile length. Bell open or raise-focus acks the badge; pile stays. FFM does not ack. |
| 2026-09-03 | Bell vs list: ≤4 from one app list as rows; 5+ collapse. Same tag replaces in the pile. |
