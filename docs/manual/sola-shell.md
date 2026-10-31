# Shell

The Shell (`sola-shell`) draws the desk's own surfaces: the top row, the
bar, the launcher, the keyboard shortcuts overlay, notification cards and
the screenshot marquee. It also decides where each window goes
(workspaces and zones) and owns the desktop's keyboard shortcuts.

## The desk

- **The top row** runs across the top of the output: a row of islands
  (pills 28 px tall) a 10 px gap in from the edges, with nothing behind
  them.
- **The bar** is the column on the left, 240 px wide, under the top row.
  It lists the windows of the workspace you are on.
- **Windows** fill the area right of the bar, in zones.

Every gap is 10 px: around the edges, between the bar and the windows,
and between windows. What shows through the gaps and the windows' rounded
corners is the ground: the background picture, or black without one
([Theme](theme.md#colors-from-the-background-picture)).

## Keyboard

| Chord | Action |
|-------|--------|
| Super+Space | Open the launcher |
| Super+K | Keyboard shortcuts overlay (press again to close) |
| Super+H | Hide the focused app |
| Super+L | Lock the screen (needs a password: [Security](security.md#locking)) |
| Super+Q | Close the focused app: every window of it |
| Super+Backtick | Show the next window in the bar (Super+Shift+Backtick: the previous) |
| Super+1 … Super+9 | Show workspace 1 to 9 |
| Super+Ctrl+1 … Super+Ctrl+9 | Send the focused window to workspace 1 to 9 |
| Super+Tab | Next workspace (Super+Shift+Tab: the previous) |
| Super+Keypad 0 | Move the focused window to Full |
| Super+Keypad 4 | Move the focused window to the left half |
| Super+Keypad 6 | Move the focused window to the right half |
| Super+Keypad Multiply | Float the focused window |
| Super+Keypad Enter | Cinema on or off for the focused window |
| Super+Shift+3 | Screenshot of the whole output, to the clipboard |
| Super+Shift+4 | Screenshot of a selection, to the clipboard |
| Super+Shift+5 | Screenshot of the focused window, to the clipboard |
| Escape | Close the open launcher, overlay, menu or panel; cancel a selection |
| Play/Pause, Next, Previous | Media players (MPRIS) |
| Mute, Volume Up, Volume Down | The default output: mute toggles, volume steps 5% (up to 100%) |

The media keys work whatever has focus; the Shell runs
`solactl media <action>` for them. Escape belongs to the Shell only while
one of its surfaces is open; otherwise it goes to the focused window.

The keypad chords work with NumLock on or off. Sola turns NumLock on for
each keyboard at session start, so the pad types digits; press NumLock to
switch it to navigation keys.

While the launcher or the shortcuts overlay is up, every other shortcut
except Super+K is ignored; while the screenshot marquee is up, every
shortcut but Escape is. Any other shortcut first closes an open menu or
panel, except the screenshot chords, which copy the screen as it is.

The focused app's menu shortcuts that use Super (Super+C, Super+T, …) are
taken by the Shell while that app has focus and handed to it as the menu
choice, so a shortcut and a click on the menu item do the same thing.

### Focus

Focus follows the pointer: when the pointer rests on another window for
200 ms, that window takes the keyboard. It does not come forward. Only a
moving pointer leads focus: when a keyboard switch, a new window or a
close leaves the pointer resting on another window, focus stays where it
went until you move the pointer. A click on a window focuses it and brings
it forward. The Shell's own surfaces never take focus from the pointer.

### Shortcuts overlay

Super+K, or **Keyboard Shortcuts** in the flower menu, opens a card of
every shortcut: the Shell's (groups Shell, Workspaces, Capture and
Window), then the focused app's menu items (in groups named after its
menus). Type to filter by name, group or chord; ↑ and ↓ move, Enter or a
click runs the item. Escape or Super+K closes it, and so does a click on
the pad around the card. **Exit Sola**, **Restart Computer** and **Shut Down**
are left out, so a filter and Enter cannot end the session.

## The top row

Left to right:

- **The flower**, in its own circle, opens the flower menu. It is Sola's
  amber unless the look recolours it
  ([Theme](theme.md#shell-tokens)).
- **Agent islands**, one per Projects session that asks you something or
  has finished ([below](#agent-islands)).
- A **message** in the middle of the row, for five seconds, when there is
  one: a launch that failed ("Failed to launch …"), an app that exited
  with an error ("… exited (code N)", "… killed (signal N)"),
  "Screenshot copied".
- On the right, an island per group: mail, the bell, the mouse battery
  and Bluetooth; the volume spectrum; CPU, GPU and MEM; RX and TX; the
  clock.

Click an item for its panel; its island lifts while the panel is open.
Click it again, press Escape, or click a window to close the panel.

### Flower menu

| Item | What it does |
|------|--------------|
| Launch Application… | Opens the launcher |
| Settings… | Brings Settings forward, or starts it |
| Keyboard Shortcuts | Opens the shortcuts overlay (Super+K) |
| Lock Screen | Locks the screen (Super+L; [Security](security.md#locking)) |
| Exit Sola | Ends the desktop session (`Shutdown` on the bus); on Oath the console then has the screen, and `oath desk` brings the desk back |
| Restart Computer | Restarts the machine |
| Shut Down | Powers the machine off |

Restart Computer and Shut Down have no confirmation and no shortcut. On
Oath they run `sudo /bin/oath set host:local power=reboot` (or
`power=halt`), then `sudo /bin/oath apply --confirm host:local`. Sola has
no password prompt, so they work only when sudo lets you run those
commands without one. A failure shows **Could not restart the computer**
or **Could not shut down**.

### Clock and calendar

The clock reads `HH:MM Day YYYY-MM-DD` (for example
`14:05 Thu 2026-10-29`) and updates every ten seconds. Its panel is a
month: six weeks from the Sunday on or before the 1st, the days of the
months either side faint, today on an accent circle. ‹ and › step the
month; **Today** comes back to this one (dim while this month shows). The
panel always opens on this month.

Under the month, **Today** lists today's events from Calendar: a bar in
the event's calendar colour, the time (or "all day") and the title, with
the count at the right. An event that has ended dims; one under way is
brighter. Click one and Calendar shows it, starting Calendar first if it
is closed. Past five events, **N more in Calendar** opens Calendar. With
none, it says **Nothing today**. The list is what Calendar last
published, so it is there with Calendar closed; it appears once Calendar
has run.

### CPU, GPU, MEM, RX and TX

Each is an LED graph of the last 12 seconds, a column a second, five dots
tall. A dot lights once its column reaches its middle. CPU, GPU and MEM
are drawn in the theme's text colour, its warning colour from 75% and its
danger colour from 90%; RX is in the accent and TX in the success colour,
each scaled to its own recent peak. GPU shows only for an NVIDIA GPU that
NVML can read.

Click one for its panel: the value in large type, then the last 60
seconds as columns tinted the same way, with the peak.

- **CPU**: the model and its cores and threads; per-thread load as a
  column per thread (a gap between threads, two between cores); the top
  processes with LED bars; load averages and uptime.
- **GPU**: the card and its memory; VRAM as a row of LEDs; the top
  processes by GPU and by VRAM; temperature, power and fan.
- **MEM**: total memory; a row of LEDs (used lit, cache dim, free dark)
  with the three figures; the top processes by memory; swap and
  pressure.
- **RX** and **TX**: the rate, the interface; its address and the bytes
  moved this session.

### Volume

The volume island is a 12-band LED spectrum of what the default output is
playing (green at the bottom to amber at the top), dimmed while the
output is muted or at zero. It shows while PipeWire answers. Click it for
the volume panel: **Output** and **Input**, each with its level, a slider,
a mute button and its devices. Click a device to make it the default
(`wpctl set-default`).

### Mail, the bell, the mouse and Bluetooth

- **Mail**: an envelope and the inbox's unread count, in the accent,
  while Mail has a window and there is unread mail. Click it to bring Mail
  forward.
- **The bell**: the missed notifications and their count
  ([Notifications](#notifications)).
- **The mouse**: a Lamzu mouse on its 4K receiver (`3554:f510`) shows its
  battery: a mouse glyph and a percent, in the warning colour at 20% and
  below, the danger colour at 10% and below, with a charging glyph while
  it charges. A sleeping mouse does not answer, so the Shell asks every
  ten seconds until it does (the chip appears soon after you first move
  the mouse); after that it asks once a minute. The number dims after ten
  minutes without an answer (asleep, off, or out of range), and the chip
  goes when the receiver is unplugged. The Shell needs read and write
  access to the receiver's hidraw node; without it there is no chip. Other
  receivers and the wired cable are not read. The chip has no panel.
- **Bluetooth** shows while BlueZ has an adapter: dim and crossed out
  while it is off, its connected glyph while a device is connected. Its
  panel has a switch for Bluetooth, the connected devices
  (**Disconnect**), the paired ones not connected (**Connect**), and
  **Add device**, which searches and lists named devices nearby (**Pair**)
  until **Done**. A device that needs a PIN or a passkey asks in the
  panel. The Shell reaches BlueZ on the system bus, at
  `DBUS_SYSTEM_BUS_ADDRESS` when it is set, else
  `/run/dbus/system_bus_socket`. On Oath that is `svc:dbus-system`, and
  BlueZ is `svc:bluetooth`; the `desk` profile has both, and a machine
  installed before them adds them with `oath new` (Oath's manual,
  Services).

## Agent islands

When a Projects session asks you something (a permission, a question) or
finishes its turn, an island for it appears in the top row after the
flower, the newest first: a diamond for a question, a check for a
finished turn, then the project's name and the tab's in parentheses
("Sola (root)"), and for a question what it wants ("wants to run …"). A
project on another computer goes by its folder alone, without "on
<computer>". When the islands reach the middle of the row, a **+N**
island lists the rest; click one there for its card.

Click an island for its card, under it: who it is and how long ago, the
question or the session's last message, and then:

- a question's answers as buttons, and a field for your own answer;
- for a finished turn, a field to tell it what to do next.

Return sends the field. What you pick or type is typed into the session,
as you would type it, and the island goes. **Show** brings up the
session's tab without answering. Escape, or a click on the island again,
closes the card and leaves the island.

An island also goes when the session moves on, and when you look at it
yourself: its tab is the one its project window shows and that window has
the keyboard. While the desk is fading idle you are away, so a session in
the focused tab gets an island too.

## The bar

The bar lists every window on the workspace in one order of its own, not
grouped by zone. Each row is the app's icon and the window's title (the
app's name while it has none).

- **The focused window's row** is the bright pill, in the theme's
  highlight colour.
- **A window its zone is showing** is lifted a step while that zone is on
  screen, and in full ink without the lift while it is not (Full while the
  halves are up, or the halves while Full is).
- **Windows behind** are muted.
- **An app hidden** with Super+H says *hidden* on its rows.
- **Activity**: a window that is working shows a turning ring in place of
  its icon, and one waiting on a decision a diamond, so the title does not
  move. Projects marks a project's window by its tabs (a decision waiting
  outranks work), and Design marks a project's window while an agent works
  in one of its canvases.

Click a row to show its window in its zone and focus it (its workspace
and zone come on screen with it, and a hidden app comes back). Hovering a
row shows a red close dot: it closes that window as the window's own
close dot would. Drag a row to move it in the list; the window stays in
its zone and nothing on screen changes. Super+Backtick steps through the
list in this order.

While an app starts from the launcher, a faint row with its name and a
turning ring stands where its window will land; the window takes its
place when it opens (or the row goes after 20 seconds, or when the launch
fails).

A new window's row goes right after the row of the window that had focus:
a browser page opened with a link, a Terminal tab, a window from the
launcher. Windows opened behind it from the same window stay in the order
you opened them. A dialog has no row: it rides above the window it
belongs to.

The bar's layout (the workspaces, and each window's place and zone) is
saved in `~/.config/sola/shell/tabs.yaml`. Windows that come back within
two minutes of the Shell starting take their saved places, and a window
whose app restarts within 30 seconds takes the place it left.

### The head and the workspace menu

The head of the bar names the workspace you are on. A diamond beside the
name means a window on another workspace waits on a decision; a dot, that
a Mail window on another workspace has unread mail.

Click the head for the workspace menu: every workspace (the current one
checked, Super+1 to Super+9 beside the first nine; click one to show it),
then, for the current one:

- **New Workspace** adds "Workspace N" after the current one and shows
  it.
- **Rename…** turns the menu into a field: Return renames, Escape
  cancels. An empty name keeps the old one.
- **Move Left** and **Move Right** move it in the order (dimmed at the
  ends).
- **Delete** removes it. Its windows join the workspace before it (the
  next one, for the first), in the same zones, at the end of its bar. The
  last workspace cannot be deleted.

## Workspaces

Each workspace has its own windows and its own bar; only the workspace
you are on is on screen. A fresh desk starts with one, **Main**.

- **Super+1 … Super+9** shows a workspace; its last focused window takes
  the keyboard.
- **Super+Ctrl+1 … Super+Ctrl+9** sends the focused window there, into
  the same zone, at the end of that workspace's bar, shown in its zone.
  You stay where you are.
- **Super+Tab** and **Super+Shift+Tab** step through the workspaces,
  wrapping.

Showing a window that is on another workspace (from a notification, an
agent island, Mail's chip) shows that workspace.

Each workspace has a browser profile of its own, which goes by the
workspace, not its name. Browser pages on a workspace share its
sign-ins, and a page sent to another workspace reopens there under that
workspace's profile. A web app keeps its own profile wherever it is
([Browser](sola-browser.md#profiles)).

## Zones

A window is in one of four zones:

| Zone | Where it is |
|------|-------------|
| Full | The whole area right of the bar |
| Left, Right | Each half of that area |
| Floating | Its own rectangle, above the tiles |

A tiled zone shows one window at a time; the others in it wait off
screen until you show them. Full and the halves take turns: showing a
window in Full puts Full on screen and takes the halves away, and showing
one in a half brings both halves back. A half keeps its half of the area
when the other one is empty. Every floating window on the workspace shows,
the last one raised on top.

A new window joins the zone in front: Full while Full is on screen (or
when neither half has a window), otherwise the half that last had focus.
Scope, Preview and gamescope float from the start, and
so does an app whose window you floated, until you tile it again.

- **Super+Keypad 0, 4 and 6** move the focused window to Full, Left or
  Right. It shows there and keeps the keyboard; its row stays where it is.
- **Super+Keypad Multiply** floats it, shrunk 50 px in on each side from
  its tile. Its app's windows then come back floating at that rectangle,
  and a float you move or resize is remembered for its app. Moving a
  window out of Floating forgets the rectangle.
- **Super+Keypad Enter** toggles Cinema: the window covers the whole
  output, over the top row and the bar, as a fullscreen window. Press it
  again to leave; a float goes back to its rectangle.

Closing the window a zone shows brings back the one it showed before, so
closing a page you just opened returns to the page it came from. When the
focused window closes, the keyboard goes to the window now showing in its
zone, else to the window in front.

## Titlebars

Every Sola window draws its own frame: a titlebar with the red close dot,
the app's menus (the first under the app's name in bold), the **Window**
menu, and the window's title at the right. A menu that does not fit is
left out. The top row has no app menus. A focused window's frame has a
lit rim.

Windows of other programs (XWayland, other Wayland apps) have no Sola
titlebar. The compositor gives them the same rim, and rounded corners
while they are tiled (a floating one keeps square corners); the
shortcuts work on them as on any window.

### Window menu

An app's last menu is the **Window** menu (an app can replace it). It
holds **Hide** (Super+H), **Cycle Windows** (Super+Backtick),
**Float**, **Left**, **Right**, **Full** and **Cinema**, so each of those
chords also has a mouse path. The Shell acts on the focused window.

## Launcher

Super+Space opens the launcher; type to filter the apps and press Enter
(or click) to open one. An app that is already running comes forward
instead of starting again: its last used window is shown in its zone, on
its workspace, and takes the keyboard. A hidden app is shown again rather
than started twice. **Terminal** always opens a new tab, in a window of
its own, right after the focused window. **Projects** and **Browser** ask
first (below). Escape closes the launcher; so does a click outside the
card.

Super+V pastes into the launcher's field (an address for Browser, say),
and Super+C / X / A copy, cut and select in it, whatever app was focused
when it opened. The same holds in the Super+K overlay.

### Projects

Projects opens a project, not another Projects. Every project is a folder
of `~/Projects`; picking Projects turns the launcher into a list of those
folders, the field scoped to "Projects". Type to filter.

- A project that is open says **Open on** its workspace; picking it shows
  that workspace and its window.
- Any other folder opens in the workspace you are on, right after the
  focused window. Closing a project's window closes the project: its
  sessions end and Projects forgets it.
- **New project…** asks for a name (or takes the one you typed): Enter
  makes `~/Projects/<name>` and opens it the same way.
- **On another computer…** asks for a computer: type its name (or
  `user@name`, or a `~/.ssh/config` alias); the last eight you opened are
  listed, latest first. Enter connects over ssh and lists that computer's
  `~/Projects`, the field scoped to its name; picking one opens it in
  Projects as "<folder> on <computer>", and **New project** makes one
  there. Its tabs run there and work as tabs here do
  ([Projects on another computer](solactl.md#projects-on-another-computer)).
  The listing never asks for a password: it needs a key or an ssh agent
  that logs you in, and tmux on that computer. A computer you have not
  reached before is trusted the first time; one whose key changed is
  refused. If it cannot be listed, the reason shows above the computers.
- Escape goes back a step: a computer's projects to the computers, the
  computers to this one's projects, those to the apps; again closes the
  launcher.

If Projects is not running, it starts and opens the project.

### Browser

Picking Browser turns the launcher into a field scoped to "Browser": type
an address or what to search for, as in the browser's location bar.

- With nothing typed, your six latest visits are listed.
- The first row is what Enter does: open the address, or go straight to
  the top result of the search (Kagi). A search also offers Kagi's
  results page.
- Under them, the pages you have visited that match: those whose site
  starts with what you typed first, then those whose site, title or
  address holds it.
- Picking a row opens it in a new page window right after the focused
  window; the browser starts if it is not running.
- Escape goes back to the apps; again closes the launcher.

## Hide

Super+H does not close the app. Every window of the focused app drops off
the screen (the compositor stops drawing it) and the keyboard goes to the
most recent window on the workspace; a zone showing one of its windows
shows its next window instead. The process keeps running.

To bring it back: click one of its rows in the bar (or reach it with
Super+Backtick), or pick it in the launcher. Clicking one of its
notifications, the mail chip or an event in the clock's panel brings it
back too, and so does the app showing one of its own windows.

Hiding the last app on screen leaves the top row, the bar and the ground.

## Notifications

A notification arrives as a card at the top right, under the top row: the
app's icon, the source, the title and the text. Click it to bring its app
forward (the app is told which notification it was); × dismisses it. Up
to three cards show at once. A card leaves after six seconds and goes to
the missed list; a newer card pushes the oldest out the same way.
Calendar's reminders are the exception: they stay until you click or
dismiss them (up to six). A notification that updates an earlier one
(the same tag) replaces it rather than stacking.

The bell in the top row appears while anything is missed, with the
count, in the accent until you open its panel. The panel, **Notifications**,
groups the missed items by app, newest first:

- An app with up to four items lists each of them; one with more is a
  single row with a count. Click it to expand (the newest 30, then how
  many more).
- Click an item to bring its app forward. × on an item dismisses it, and
  × on an app's row dismisses all of that app's items.

There is no Clear All. With nothing missed the panel says **Nothing
missed.**

## Screenshots

Super+Shift+3, 4 and 5 copy a PNG to the clipboard and show
**Screenshot copied** in the top row. They write no file and open no
viewer. The clipboard is offered at once, so you can paste right away;
the paste waits until encoding finishes.

- **Super+Shift+3** copies the whole output.
- **Super+Shift+4** first freezes the output as it is (open menus, panels
  and text selections stay in the still), then shows a marquee on that
  still at full brightness, with no dim. Drag a rectangle to copy it;
  Escape, or a drag too small to be a rectangle, cancels. The crop comes
  from the freeze, not from a second capture.
- **Super+Shift+5** copies the focused window's own buffer. The window
  does not come forward. `solactl compositor screenshot --app` does the
  same but writes a PNG file
  ([solactl](solactl.md#compositor-solactl-compositor)).

The keyboard returns to the window that had it.

## The idle fade

The desk's surfaces (the windows, the bar and the top row) are drawn over
the background picture at the desk's opacity: 90% at first, 50–100% in
Settings → Appearance → Desk. The fade and the dim are set in Settings →
Appearance → Idle, and both are on at first:

- **Fade the windows when idle**: after **Start after** minutes with no
  input (5 at first, up to 60), everything fades into the picture over
  **Fade over** minutes (30 at first, up to 120).
- **Then dim the background**: once only the picture shows, it dims after
  **Dim after** minutes (60 at first, up to 240) over **Dim over**
  minutes (30 at first, up to 120), to a tenth of its light.

Any input brings it all back within a second. The compositor draws both
([Compositor](compositor.md)).

The flower and the agent islands stay at the desk's opacity while
everything else fades, so a session waiting on you can still be seen and
answered. Only the dim that follows covers them. Once the fade is full,
the Shell tells apps their tab windows are off screen (the browser rests
its pages) until you are back. Nothing fades while a window is in Cinema
or an app keeps the screen awake (a video playing).

## Limits

- Hide works per app: every window of the app goes.
- There is one output layout: Full and two equal halves.
- The Window menu and the shortcuts overlay also list **Top**,
  **Bottom**, **Top Middle**, **Full Middle**, **Bottom Middle** and
  **Middle Right**, with keypad chords. On this layout they do nothing,
  and the Shell does not take those chords.
- Notification cards cannot be answered in place; a click goes to the
  app.
