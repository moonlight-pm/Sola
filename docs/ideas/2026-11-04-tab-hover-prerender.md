# Render a tab while the pointer is over it

**Parked (Joshua, 2026-11-04: "if i have a mouse over a tab in the desktop bar,
lets render that window even if its not show, that way its already rendering if
i click it"; then "maybe we'll do that another time").** Status: `idea`.

Hovering gives a head start of a few hundred milliseconds before a click, and it
costs one window at a time.

## What is known

- A hidden Projects tab draws nothing today: on a private desk with the shell, a
  tab streaming output while hidden cost no terminal draws and was current
  within about a second of being shown (checked at 1.2 s; the time to current
  was not measured). Its buffer until then is its last painted frame.
- `sola-river` hides a window with `proxy.hide()` (`client/manage.rs`); the
  compositor is rivulet, River 0.4.5 ported to C, in Oath's repo.
- The tab bar has no hover message yet: rows only style themselves on hover.
- Browser pauses a hidden page itself (`was_hidden`, from `TabWindow.shown`), so
  it may gain the most, and it could act on a hint with no help from the
  compositor.

## Open

- Can a hidden window present at all? If rivulet sends a hidden surface no frame
  callbacks, winit holds its redraw, and forcing a present risks blocking the
  UI thread (kit apps use wgpu's `AutoVsync`). Not tested.
- How long a shown tab takes to become current. Measure this first: it says what
  the idea can buy.
- A change in rivulet is Oath's call, and a move of `pkg:rivulet` ends apps.
