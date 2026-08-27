# Sola notifications

**Date:** 2026-08-25  
**Status:** **Frozen** — implemented on `sola-browser`; installed 2026-08-26 (desk smoke pending)  
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
| Pile | Menubar `lucide/bell` + count in the right cluster (same family as mail unread). Hidden at 0. Click opens a list under the chip (menu overlay). Session-only; cap 30. |
| Pile click | Same as card click (raise source). Pile × / Clear removes. |
| Replace | Same `app_id` + `tag` replaces an in-flight banner. |
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
  `source` = host, `tab_id` set).
- `NotificationActivate` with `tab_id` → `SetActiveTab`.

Denied / default: constructor is a no-op. Permission `"default"` until
the user chooses.

## Out of scope

Do Not Disturb, grouping, persistence across session, action buttons,
sound, image/icon from the page, `org.freedesktop.Notifications`,
promoting remaining `AppToast` senders (launch fail / exit).

## Implementation status

| Item | Status |
|------|--------|
| Freeze | **this document** |
| Bus topics | **done** (`AppNotification`, `NotificationActivate`) |
| Shell HUD + pile | **done** |
| Browser intercept + permission | **done** |
| Workspaces done → notification | **done** |
| Dogfood | **reinstalled** 2026-08-26 `bus`+`shell`+`browser`+`workspaces` (release, bus first) after Subscribe/renderer breaks; desk smoke pending |

## Decision log

| Date | Choice |
|------|--------|
| 2026-08-25 | Split whisper (menubar `AppToast`) vs notification (desk card) |
| 2026-08-25 | Drop-from-bar + missed-pile chip; click raises source |
| 2026-08-25 | Browser via JS intercept, not a D-Bus notification server |
| 2026-08-25 | No action buttons / sound in v1 |
