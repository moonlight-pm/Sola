# The bus

Sola Bus (`sola-bus`) carries facts between Sola's processes. A process
publishes a typed *topic*, such as "the window list is now this" or "this
app's menu is this", and the bus copies it to every connected process that
subscribed to that kind of topic. Nothing on the bus is a request: there
are no replies, request ids, or timeouts. Work that needs an answer goes
through the call plane, `sola-call` ([Call plane](call.md)).

Some topics are *sticky*: the bus keeps the latest value and hands it to
every process that subscribes later. Some sticky topics are also
*persistent*: the bus writes them to disk and restores them when it starts.
This is how a restarted shell finds the open windows and their tabs, and
how the theme and the background picture survive a reboot. Where the bus
sits among Sola's processes is in [Architecture](architecture.md).

| Item | Value |
|---|---|
| Process | `sola-bus`. On Oath, PID 1 runs it as the service `svc:sola-bus` |
| Socket | `$SOLA_BUS_PATH`, else `sola-bus` in the runtime directory: `$XDG_RUNTIME_DIR`, else `/run/user/<uid>` when that directory exists, else `/tmp` |
| Framing | 4-byte little-endian length, then one postcard-encoded message |
| Topics | 63 kinds, fixed when Sola is built: 26 ephemeral, 19 sticky, 18 persistent ([`topics.rs`](../../crates/sola-bus/src/topics.rs)) |
| Disk | `~/.config/sola/state.yaml` and per-topic YAML files |
| Rust client | `sola_bus::BusClient`; kit apps use `sola_kit::app` |
| Log | `sola.log` in Sola's log directory, lines tagged `[bus]`. On Oath that is `~/.local/state/sola/` (`$XDG_STATE_HOME/sola`; `SOLA_LOG_DIR` overrides it) |

At startup the bus deletes any file left at the socket path and binds a new
socket; if the bind fails, it logs the error and exits. The bus does no
authentication: any process that can open the socket can subscribe to
everything, publish any topic under any app id, and overwrite or retract a
record another app published. The socket is protected only by the
permissions of the runtime directory.

## Wire format

A frame is a 4-byte unsigned length, little-endian, followed by that many
bytes holding one `Message` encoded with postcard 1.x. Both directions use
the same framing. There is no maximum frame size, version field, or
checksum. When a frame does not decode, the bus logs a read error and
closes the connection.

Postcard writes the `Message` fields in this order, with no names or tags.
Varints are postcard's unsigned varints (7 bits per byte, low bits first).

| Field | Type | Encoding | Meaning |
|---|---|---|---|
| `id` | UUIDv7 | varint length (16), then 16 bytes | Unique per message; embeds a millisecond timestamp. The bus does not order by it. |
| `topic` | string | varint length, then UTF-8 | A `Topic` variant name such as `Windows` or `Theme` (case-sensitive), or a control name |
| `payload` | optional bytes | `0`, or `1` + varint length + bytes | The topic's payload, itself postcard-encoded; absent for topics without one (`Shutdown`, `MouseLeft`) |
| `sticky` | bool | one byte | `1` stores the message as a record; `0` on a sticky topic is a retraction |
| `source` | string | varint length, then UTF-8 | The publisher's app id, filled in by `BusClient`; provenance only |
| `keys` | list of strings | varint count, then each string | The values of the topic's key fields as text (`window_id` 42 is `"42"`); empty for unkeyed topics |

Payloads are positional postcard with no field names, so a payload decodes
only as the exact type it was encoded from. A process built from another
`sola-bus` source reads only the topics whose payloads kept their layout;
only `Application` and `MailConfig` also accept older layouts. Keep every
process on the bus on one build.

Three topic names that start with `$` are instructions to the bus. It
never forwards or answers them.

| Topic | Payload | Effect |
|---|---|---|
| `$identify` | postcard string: the app id | Names this connection |
| `$subscribe-names` | postcard list of strings: topic names | Replaces this connection's subscription set |
| `$subscribe` | postcard list of integers: topic numbers | The same, for a bus from before `$subscribe-names`; ignored from a connection that has sent `$subscribe-names` |

`$subscribe-names` names each kind as its messages do (`TabLayout`), so a
bus and a client from different builds agree on what was asked for. The
bus skips names it does not know, a newer client's topics, and applies the
rest.

`$subscribe` carries each kind as its index in a list frozen at the last
build that sent only numbers (`subscribe::NUMBERED`, 64 names). That
build's numbers were the kinds' order in the enum `sola-bus` generates,
which groups them by retention, so adding a topic renumbered others; the
frozen list keeps those numbers, including `CustomTheme`'s, a topic since
removed. The bus skips numbers it cannot place and applies the rest; when
nothing usable remains, the connection keeps its previous set. A kind
added after the freeze has no number, and a bus from before names never
receives a subscription to it.

## Connecting and identifying

A client connects and may send frames at once. There is no greeting, and
the bus sends nothing until the client subscribes. `BusClient` sends
`$identify` with its app id first on every connection.

When a connection identifies, the bus adds the app id to its roster and
publishes `ClientConnected`; when an identified connection closes, it
publishes `ClientDisconnected`. Identifying again with the same id does
nothing; with a different id, the bus replaces the roster entry and
publishes `ClientConnected` again. App ids are not checked for uniqueness.
A connection that never identifies still works but is not in the roster.

## Subscribing

Until a connection sends `$subscribe`, the bus delivers nothing to it. Each
`$subscribe` replaces the previous set of kinds. For every kind that is new
in the set, the bus immediately sends every stored record of that kind (the
sticky replay), in no defined order, and, for `ClientConnected`, one
message per identified connection, including the subscriber's own if it has
identified. Kinds already in the set are not replayed, so sending the same
set again does nothing.

Subscription is by kind only, with no filter on keys, payload, or source. A
client subscribed to `MenuAction` receives every app's menu actions and
keeps those whose `app_id` is its own.

`BusClient::subscribe` sends two frames: `$subscribe-names`, then
`$subscribe` with the kinds the frozen list has. A current bus applies the
first and ignores the second; a bus from before names logs `broadcast
dropping unknown topic` for the first and applies the second.

## Publishing

Any frame whose topic is not a control name is a publication. The bus:

1. Stores it as a record if its `sticky` flag is set, or deletes the record
   if it is a [retraction](#retraction).
2. Queues it for every connection subscribed to its kind. For sticky and
   persistent topics the publisher receives its own message too, so it sees
   its write through the same path as everyone else; for ephemeral topics
   the publisher is skipped.
3. Writes it to disk if the topic is persistent.

A message whose topic name is unknown, or whose payload does not decode,
reaches no one, and the bus logs a warning. If it has `sticky` set, it is
still stored, replacing any record with the same topic and keys, and
replays to no one.

`BusClient::emit(topic)` fills in the envelope: a fresh `id`, the encoded
payload, `sticky` from the topic's retention, `source` from the client's
app id, and `keys` from the payload's key fields. `BusClient::retract(topic)`
sends the same message with `sticky` false; for an ephemeral topic it logs
a warning and sends nothing.

## Sticky and persistent topics

| Retention | Kept by the bus | On disk | Replayed to new subscribers |
|---|---|---|---|
| Ephemeral | No | No | No |
| Sticky | Latest value of each record, in memory | No | Yes |
| Persistent | Latest value of each record, in memory | Yes | Yes |

The bus stores sticky messages by *record*: the topic name plus the key
values. An unkeyed topic has one record. A keyed topic has one per key:
`SetAppMenu` is keyed by `app_id`, so each app's menu is its own record. A
new message for a record replaces it, whoever sent it. The bus trusts the
`sticky` flag; `BusClient` sets it from the topic's retention.

A record outlives the connection that wrote it: an app's records stay after
it exits, until something retracts them or, for sticky topics, the bus
restarts. Replay sends the stored message as it arrived, with its original
`id` and `source`; records restored from disk carry the source `sola-bus`.

### Retraction

A message with `sticky` false on a sticky or persistent topic is a
retraction. The bus deletes the record named by its topic and keys, deletes
it from disk if the topic is persistent, and delivers the message to
subscribers, the publisher included, so they can drop their copies. A
subscriber tells an update from a retraction by the `sticky` flag.

### Persistence

Persistent records live in `$XDG_CONFIG_HOME/sola` when `XDG_CONFIG_HOME` is
an absolute path, otherwise `~/.config/sola`.

| Topic | File | Layout |
|---|---|---|
| `SessionApps`, `MailConfig`, `CalendarConfig`, `Background`, `Appearance`, `Zones`, `TerminalConfig`, `MonitorConfig` | `state.yaml` | A top-level key named after the topic, holding the payload |
| `FloatGeometry`, `Application`, `TerminalSession` | `state.yaml` | A top-level key holding a list, one entry per record |
| `Theme` | `theme/current.yaml` | The whole file |
| `TabLayout` | `shell/tabs.yaml` | The whole file |
| `CalendarAgenda` | `calendar/agenda.yaml` | The whole file |
| `PaintSession` | `paint.yaml` | The whole file |
| `BrowserConfig`, `BrowserHistory` | `browser.yaml`, `browser/history.yaml` | The whole file |
| `BrowserTab` | `browser/tabs/<id>.yaml` | One file per record |

The bus rewrites a file each time one of its records changes (a temporary
file renamed into place), and a retraction removes the entry or deletes the
file. It does not write a record whose key would make an unsafe file name
(empty, or containing `/`, NUL, or `..`); it logs a warning instead.

The bus reads these files only at startup, skipping with a warning anything
that is not a persistent topic or does not match its schema. An edit made
while the bus runs is not seen, and the next write of that topic replaces
it. Unknown keys in `state.yaml` survive rewrites, but a `state.yaml` that
does not parse is treated as empty and replaced whole on the next write.

`Encrypted` fields (mail and calendar passwords and tokens) cross the socket
in clear and are written to disk encrypted with age, using the key in
`~/.config/sola/key`. If that key is lost, records holding encrypted fields
no longer load; the bus skips them with a warning.

## Topic reference

*Acted on by* lists the processes that use a topic. Many apps (Shell,
Settings, Monitor, Paint, Calendar, and others) subscribe to every
kind and ignore most. "Apps with a window per tab" are Terminal, Browser
(its pages and web apps), Paint, Projects and Design, which do it through
the kit's `TabWindows`.

### Windows, input, and the compositor

| Topic | Payload | Published by | Acted on by | Kept |
|---|---|---|---|---|
| `Windows` | Every window: `window_id`, `app_id`, `title`, `pid` (when known) | `sola-river` | Shell, `sola-session`, Settings; kit apps, to learn their own window ids | Sticky |
| `OutputGeometry` | The output's logical `width` and `height` | `sola-river`, when the compositor reports the output size and after a bus reconnect | Shell, Browser | Sticky |
| `WindowGeometry` | One window's `window_id`, `x`, `y`, `width`, `height` | `sola-river`, when the window's size or position changes; retracted when it closes | Shell | Sticky by `window_id` |
| `WindowParent` | A dialog's or picker's `window_id` and the `parent` window it belongs to | `sola-river`, from the compositor's `parent` event; retracted when the window closes or loses its parent | Shell (no row in the bar; composed above its parent) | Sticky by `window_id` |
| `WindowFloating` | `window_id`, `floating` | Shell; `sola-river` re-sends its floating windows after a bus reconnect | `sola-river` (move, resize, shadow); kit apps (a float's frame resizes from its edges) | Sticky by `window_id` |
| `WindowChrome` | `window_id`; `tiled` (in a tiled zone), `cinema` (covering the output), `rim` (its app published no menu, so it draws no frame of its own) | Shell; retracted when the window closes | `sola-river` (tiled state, the rim, rounded corners); kit apps (no frame in Cinema) | Sticky by `window_id` |
| `Composition` | Window ids to show, bottom to top | Shell | `sola-river` | Ephemeral |
| `Frame` | One window's `window_id`, `x`, `y`, `width`, `height`, and `fullscreen` flag | Shell | `sola-river` | Ephemeral |
| `Focus` | The `window_id` to give the keyboard | Shell | `sola-river` | Ephemeral |
| `AppHidden` | An `app_id` left out of `Composition` | Shell ([Hide](sola-shell.md#hide)); retracted to show it again | Shell | Sticky by `app_id` |
| `CloseWindow` | A `window_id` | Shell (a bar row's stoplight, for a window that is not a tab) | `sola-river`: asks that window to close, as its own close button would | Ephemeral |
| `MouseEntered` | The `window_id` the pointer entered; `still` when the pointer did not move (the window under it changed) | `sola-river` | Shell | Ephemeral |
| `MouseLeft` | None | `sola-river` | Shell | Ephemeral |
| `MouseClicked` | The `window_id` clicked or touched | `sola-river` | Shell | Ephemeral |
| `RegisteredChords` | Chords (`keysym`, `modifiers`) to route to the bus, not the focused window | Shell | `sola-river` | Sticky |
| `Chord` | A registered chord was pressed: `keysym`, `modifiers` | `sola-river` | Shell, Browser, Design | Ephemeral |
| `ChordReleased` | A registered chord was released: `keysym`, `modifiers` | `sola-river` | Browser, Design (Super held, for Super+click) | Ephemeral |

What `sola-river` does with these is in [Compositor](compositor.md).

### The bar, tabs, and workspaces

| Topic | Payload | Published by | Acted on by | Kept |
|---|---|---|---|---|
| `Tab` | A window an app opens as a tab, announced just before it opens: `key` (`<app_id>/<the app's tab id>`, stable across restarts), `app_id`, `pid`, `seq` (how many windows that process has opened, this one included), `window_id` (once the shell has said, so a restarted shell pairs it directly), `background` (opened behind the tab its zone shows, without focus) | Apps with a window per tab; retracted when the tab closes | Shell (pairs a process's tabs with its new windows, in order) | Sticky by `key` |
| `TabWindow` | A tab's `key`, its `window_id`, and `shown` (its window is on screen) | Shell; sent again when `shown` changes, retracted when the window closes | The tab's app (a browser page that is not shown stops painting) | Sticky by `key` |
| `TabActivity` | A tab's `key` and its `activity`: `Idle`, `Working`, or `Waiting` (needs a decision) | Projects (a project window with an agent working or waiting), Design (a project window an agent is working in); sent first when the window opens, retracted when the tab closes | Shell (the mark in the row's icon slot) | Sticky by `key` |
| `ShowTab` | A tab's `key` | Browser, Paint, Projects, Design (a notification click, a dialog in a page behind, `tab.focus`) | Shell: shows the tab's window in its zone, on its workspace, and focuses it | Ephemeral |
| `ShowWindow` | A `window_id` | Calendar (an app bringing its own window forward) | Shell: as for `ShowTab` | Ephemeral |
| `CloseTab` | A tab's `key` | Shell (a bar row's stoplight) | The app that opened the tab: closes its window as its own close button would | Ephemeral |
| `TabLayout` | The bar as last saved: `zones`, its runs in order (each a `zone`, its `apps` as tab keys, or app ids for windows that are not tabs, `shown`, the index of the tab the zone shows, and its `workspace`); `workspaces` (`id`, `name`); `showing`; `full` (the workspaces whose Full zone is on screen) | Shell | Shell, at its next start: returning windows take their places | Persistent |
| `Workspaces` | The `workspaces` in order (`id`, `name`) and the one `showing` | Shell | Browser (a new page window opens under the showing workspace's profile) | Sticky |
| `WindowWorkspace` | `window_id`, `workspace` | Shell; retracted when the window closes | Browser (a page whose workspace has another profile reopens under it) | Sticky by `window_id` |
| `FloatGeometry` | A floating app's last rectangle: `app_id`, `x`, `y`, `width`, `height` | Shell | Shell | Persistent by `app_id` |

The bar is described in [Shell § The bar](sola-shell.md#the-bar), its zones in [Shell § Zones](sola-shell.md#zones).

### Apps, session, and menus

| Topic | Payload | Published by | Acted on by | Kept |
|---|---|---|---|---|
| `LaunchApp` | `app_id`, `command` | Shell (launcher, the Projects chooser), Browser (`--app` when no browser is running) | `sola-session` | Ephemeral |
| `LaunchResult` | `app_id`, `command`, `ok`, `error` | `sola-session` | Shell | Ephemeral |
| `UserAppExited` | `app_id`, `command`, exit `code` or `signal` | `sola-session` | Shell | Ephemeral |
| `CloseApp` | An `app_id` | Shell (Super+Q); kit apps closing themselves (a single-window app's close button, the Browser's last page) | `sola-session` (stops it), `sola-river` (asks its windows to close), the named app (quits), Browser (closes a web app's windows) | Ephemeral |
| `SessionApps` | Open apps: `app_id`, `command` | `sola-session` | `sola-session`, at its next start | Persistent |
| `Application` | A launcher entry: `app_id`, `label`, `command`, `icon`, `kind`, `url`, `profile` | Settings; Browser (web apps) | Shell, Settings, Browser | Persistent by `app_id` |
| `SetAppMenu` | `app_id` and its menus: labels and items (action id, label, shortcut, disabled, checked) or dividers | Each app with a menu, at start | Shell (registers the focused app's Super shortcuts; an app with no menu gets the compositor's rim) | Sticky by `app_id` |
| `MenuAction` | `app_id`, `action_id` of a chosen item or shortcut | Shell (a Super shortcut of the focused app's menu); a kit app's titlebar Window menu; a second launch of Terminal or Projects, to the running one | The app named by `app_id`; the Shell for Window-menu actions, which it applies to the focused window | Ephemeral |
| `Shutdown` | None | Shell (Exit Sola, then the shell exits) | `sola-session` (stops the apps it started, then exits), `sola-river` (exits) | Ephemeral |

Launching and closing are described in [Session manager](session.md) and
[Applications](applications.md). App menus appear in each window's
titlebar; the kit draws them from the app's own copy, not from the bus.

### The desk's look

| Topic | Payload | Published by | Acted on by | Kept |
|---|---|---|---|---|
| `Theme` | The active theme ([Theme and toolkit](theme.md)) | Shell (at start when no saved theme exists, and the colors rewritten from the picture while *Adapt colors to the background* is on) | Every kit app; `sola-river` (the rim's colors) | Persistent |
| `Background` | `path` (Sola's copy under `~/.config/sola/background/`), `set_at` (unix ms, so setting a file of the same name again is a change) | Paint (File → Set as Background), Browser (Use as Desktop Background), Settings → Appearance (a kept picture), `solactl background set`; retracted by Settings → Appearance (Clear) and `solactl background clear`, for the black ground | `sola-river` (draws it behind the windows), Shell (the theme's colors), Settings | Persistent |
| `Appearance` | `adapt_theme` (the theme's colors follow the picture), `strength` (0–100, default 60), `variation` (which of the picture's schemes), `colors` (your `window`, `panels`, `highlight`, `accent` as `#rrggbb`), `customs` (your looks: a `variation` and `colors` each), `custom` (the look in use, or none), `opacity` (the desk at rest, 50–100 %, default 90), `fade_idle` (default on), `fade_after_min` (default 5), `fade_over_min` (default 30), `dim_idle` (default on), `dim_after_min` (default 60), `dim_over_min` (default 30), `light` (web pages are told light; off, dark; default off), `lock_idle` (lock once the idle fade is complete; default off), `pictures` (each kept picture's path → the look it was last worn with: `adapt_theme`, `strength`, `variation`, `colors`, `customs`, `custom`) | Settings → Appearance; Shell (`pictures`, and a kept picture's look put back when it is set again) | Shell (the theme's colors; `lock_idle`); `sola-river` (the desk's opacity, the idle fade and dim); Browser (`light`: every page's `prefers-color-scheme`) | Persistent |
| `ThemeVariations` | `path` (the picture), `choices` (per scheme: `name`, and `base`, `raised`, `accent`, and `source`, the picture's color it comes from, or for a complement the turned color itself) | Shell | Settings (the scheme cards) | Sticky |
| `IdleKeep` | `x`, `y`, `w`, `h`: output-local logical pixels, the top row's flower and agent islands | Shell | `sola-river` (that part stays at the desk's rest during the fade) | Sticky |
| `IdleFade` | `fading` (no input for the fade's delay), `out` (the fade is full: the windows are out of sight) | `sola-river` | Projects (a focused session's island shows); Shell (on `out`, reports every tab window off screen in `TabWindow`) | Sticky |

The fade and the dim are described in [Compositor § The idle fade](compositor.md#the-idle-fade).

### App state

| Topic | Payload | Published by | Acted on by | Kept |
|---|---|---|---|---|
| `MailConfig` | Mail accounts, identities, filter rules | Settings | Mail, Settings | Persistent |
| `MailStatus` | `inbox_unread` | Mail; retracted when Mail quits | Shell (the unread count in the top row) | Sticky |
| `CalendarConfig` | Calendar accounts and the calendar list | Settings, Calendar | Calendar, Settings | Persistent |
| `CalendarAgenda` | `days`, today through the next week: each a `date` (`YYYY-MM-DD`) and its `events` (`id`, `title`, `color`, `all_day`, `start`, `end` in unix seconds, `location`) | Calendar | Shell (the clock's panel, also while Calendar is closed) | Persistent |
| `TerminalSession` | One Terminal tab: `id`, tmux session, working directory, order, pane layout | Terminal | Terminal | Persistent by `id` |
| `PaintSession` | The open images' paths and the selected one | Paint | Paint | Persistent |
| `Zones`, `TerminalConfig`, `MonitorConfig`, `BrowserConfig`, `BrowserHistory`, `BrowserTab` | Declared only | Nothing | Nothing | Persistent (`BrowserTab` by `id`) |

No current app publishes or reads the last six; the bus still restores
their files if they exist.

### Handoff, notifications, and presence

| Topic | Payload | Published by | Acted on by | Kept |
|---|---|---|---|---|
| `OpenUrl` | `url`, `activate` | Nothing in Sola; `solactl emit` can | Browser (a new page window, in front if `activate` is set; ignores a message from itself); Shell (starts Browser with the URL when it is not running) | Ephemeral |
| `OpenImage` | `path`, `activate`, optional `app_id` (absent means Paint) | A second Paint launch, handing its paths to the running one | Paint, Preview (each only when addressed) | Ephemeral |
| `AppToast` | `text` | Browser (after Use as Desktop Background); `solactl emit` can | Shell (the top row, 5 seconds) | Ephemeral |
| `AppNotification` | `id`, `app_id`, `source`, `title`, `body`, optional `tag`, `tab_id`, `url` | Browser (pages and web apps), Calendar (reminders), Projects | Shell (a card on the desk) | Ephemeral |
| `NotificationActivate` | The clicked notification's `id`, `app_id`, `tab_id`, `url` | Shell | The app named by `app_id`: Browser (and its web apps), Calendar, Projects | Ephemeral |
| `AgentIsland` | A session's `key` (its pane), `name`, `project`, `state` (`Waiting` or `Done`), `title`, `body`, `choices`, `reply`, `since` | Projects; retracted when the session works again, closes, is answered, or is looked at | Shell (its island in the top row) | Sticky by `key` |
| `AgentIslandAction` | An island's `key` and what was done: `Choose(index)`, `Reply(text)`, `Show` | Shell | Projects (types it into the session, or shows its tab) | Ephemeral |
| `ClientConnected` | An app id | `sola-bus` on `$identify`; replayed as the roster | No app; Monitor shows it | Ephemeral |
| `ClientDisconnected` | An app id | `sola-bus` when an identified connection closes | No app; Monitor shows it | Ephemeral |

## Ordering and delivery

- Each connection is read on its own thread, so one connection's messages
  are handled in the order sent. Storing, queueing, and persisting happen
  under one lock, and every subscriber receives messages in the order the
  bus handled them. Disk writes hold up all traffic until they finish.
- Delivery is at most once. Each connection has an outgoing queue of 256
  messages, written by its own thread. The bus never waits for a
  subscriber: when a queue is full, the message is dropped for that
  subscriber and the bus logs `dropped (queue full)`. There is no
  acknowledgement, retry, or redelivery.
- Sticky replay uses the same queue, and replayed records that do not fit
  are dropped without a log line.
- An ephemeral message reaches only the connections subscribed when the bus
  handles it.
- `BusClient` reads into an unbounded in-memory channel on its own thread;
  `emit` writes on the caller's thread.

## When the bus restarts

A new `sola-bus` process starts with only what is on disk: persistent
records come back; sticky records, the roster, and every subscription are
gone. `BusClient` does not reconnect by itself; the program that owns it
does:

| Program | Reconnects |
|---|---|
| Kit apps | Every 250 ms from the kit's bus poller, then re-sends the subscription given to `BusSetup` |
| `sola-river` | Every 20 ms, then re-subscribes and re-publishes `OutputGeometry`, `Windows`, each `WindowGeometry`, and its floating windows |
| `sola-session` | Blocks, retrying every second, then re-subscribes |

On every connection, `BusClient::connect` sends `$identify` and then the
messages queued while it had no connection. On every connection after the
first, it then re-publishes its *sticky outbox*: the last value of every
sticky or persistent record this client published and has not retracted.
Each process rebuilds its own records as it reconnects, so a client that
subscribes early receives some records by replay and the rest as live
messages. When two processes wrote the same record, the last to reconnect
wins. A message published after the bus died but before the client noticed
fails with a write error and is not queued; ephemeral ones are lost.

## Using the bus from Rust

In a kit app:

- `BusSetup::new(app_id)` takes the subscription (`.subscribe(kinds)`), the
  app menu (`.app_menu(...)`, `.window_menu()`), and call-plane methods
  (`.calls(owner, methods)`, see [Call plane](call.md)). `.install()`
  connects (retrying every 250 ms until the bus answers), identifies,
  subscribes, publishes `SetAppMenu` (and keeps it for the window's
  titlebar), and stores the client process-wide.
- `sola_kit::app::bus_subscription()` is an iced `Subscription` of
  `Arc<Message>`. A thread polls the client every 8 ms and reconnects after
  a bus restart; while no iced subscription is attached it buffers up to 512
  messages, dropping the oldest. The client has one receiver: use this or
  `try_recv`, not both.
- `sola_kit::app::bus()` returns the client behind a mutex, for `emit` and
  `retract`; it panics before `install` (`try_bus()` returns an `Option`).
  `set_bus_kinds(kinds)` changes the set re-sent after a reconnect.
- `sola_kit::apply_theme_update` applies a `Theme` message
  ([Theme and toolkit](theme.md)); `sola_kit::is_self_quit` recognises a
  `MenuAction` with action `quit` for the app, or a `CloseApp` naming it.

```text
// main(), before building the iced application:
sola_kit::app::startup(APP_ID);
BusSetup::new(APP_ID)
    .subscribe(&[TopicKind::Theme, TopicKind::MenuAction, TopicKind::CloseApp])
    .app_menu("Example", [("quit", "Quit Example", KeyCode::Q.meta())])
    .install();

// subscription():  sola_kit::app::bus_subscription().map(Msg::Bus)
// update(), Msg::Bus(m):
if sola_kit::apply_theme_update(&m, &mut self.theme) { return Task::none(); }
if sola_kit::is_self_quit(&m, APP_ID) { return iced::exit(); }
if let Some(topic) = Topic::parse(&m) { /* … */ }

// Publishing:
if let Ok(mut b) = sola_kit::app::bus().lock() {
    let _ = b.emit(Topic::AppToast(AppToast { text: "Saved".into() }));
}
```

Without the kit, use `sola_bus::BusClient` directly:

| Method | Does |
|---|---|
| `BusClient::new()` | An unconnected client; `send` and `emit` queue until it connects |
| `set_app_id(id)` | Sets `source` for later publications and sends `$identify` |
| `connect()`, `connect_to(path)`, `connect_blocking(interval)` | Opens the socket and does the reconnect work above; the last retries until it succeeds |
| `is_connected()` | False once the reader thread has seen the socket close |
| `subscribe(kinds)`, `emit(topic)`, `retract(topic)`, `send(message)` | Sends `$subscribe`, a typed publication or retraction, or a raw `Message` |
| `recv()`, `recv_timeout(d)`, `try_recv()` | The next message, waiting without limit, up to `d`, or not at all; `None` when nothing arrives or the connection is gone |
| `notify_fd()`, `drain_notify()` | A descriptor that becomes readable when messages arrive, for event loops such as calloop |

Decode a message with `Topic::parse(&message)`, which returns `None` for an
unknown topic or a bad payload.

## Watching and poking the bus

[Monitor](sola-monitor.md) shows live traffic and every stored record.
`solactl emit` publishes one topic ([solactl](solactl.md)):

```bash
solactl emit AppToast '{"text":"hello"}'
solactl emit OpenUrl '{"url":"https://example.com","activate":true}'
solactl emit CloseApp '"sola-preview"'
```

The payload is the topic's JSON form, the shape Monitor shows; a topic
without a payload takes any JSON value, such as `null`. In JSON,
`Encrypted` fields are `age1enc:` ciphertext. `solactl` connects as
`solactl`, publishes with `BusClient::emit`, waits 50 ms, prints
`emitted <Topic>`, and exits. A sticky or persistent topic is stored (and
written to disk) like any other publication and stays after `solactl`
exits. `solactl` has no general retract; `solactl background clear`
retracts `Background`. An unknown topic (the error lists the valid names),
invalid JSON, a payload that does not match, or an unreachable bus exits
with status 3.

To log every message the bus handles, give `sola-bus` a `RUST_LOG` that
includes `sola_bus::traffic=trace` (for example
`RUST_LOG=info,sola_bus::traffic=trace`). Each message adds a line with its
connection, topic, `sticky` flag, source, and keys to `sola.log`. Nothing
rotates `sola.log` on Oath, so turn the trace off again.

## Limits

- Topics are compiled in. An app cannot define one at run time, and
  processes built from different `sola-bus` sources can misread each
  other's subscriptions and payloads.
- No authentication or per-app permissions: any process can overwrite or
  retract any record.
- Delivery is at most once; a full queue drops messages.
- Sticky records do not survive a bus restart unless their owners
  re-publish them.
- A `state.yaml` that does not parse is replaced on the next persistent
  write, losing its other entries.
- No request/reply; use [the call plane](call.md).
