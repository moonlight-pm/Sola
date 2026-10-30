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
This is how a restarted shell finds the open windows and app menus, and how
the theme survives a reboot. Where the bus sits among Sola's processes is
in [Architecture](architecture.md).

| Item | Value |
|---|---|
| Process | `sola-bus`, started and supervised by `sola` ([Process manager](process-manager.md)) |
| Socket | `$SOLA_BUS_PATH`, else `$XDG_RUNTIME_DIR/sola-bus` (`/tmp/sola-bus` without `XDG_RUNTIME_DIR`) |
| Framing | 4-byte little-endian length, then one postcard-encoded message |
| Topics | 44 kinds, fixed when Sola is built ([`topics.rs`](../../crates/sola-bus/src/topics.rs)) |
| Disk | `~/.config/sola/state.yaml` and per-topic YAML files |
| Rust client | `sola_bus::BusClient`; kit apps use `sola_kit::app` |
| Log | `/opt/sola/log/sola.log` |

At startup the bus deletes any file left at the socket path and binds a new
socket; if the bind fails, it exits and logs the error. The bus does no
authentication: any process that can open the socket can subscribe to
everything, publish any topic under any app id, and overwrite or retract a
record another app published. The socket is protected only by the
permissions of `$XDG_RUNTIME_DIR`.

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
only as the exact type it was encoded from, and every process on the bus
must be built from the same `sola-bus` source. Only `Application` and
`MailConfig` also accept older layouts.

Two topic names that start with `$` are instructions to the bus. It never
forwards or answers them.

| Topic | Payload | Effect |
|---|---|---|
| `$identify` | postcard string: the app id | Names this connection |
| `$subscribe` | postcard list of `TopicKind` | Replaces this connection's subscription set |

A `TopicKind` travels as a varint: its index in the enum that `sola-bus`
generates from the topic list. The generator groups kinds by retention
(ephemeral, then sticky, then persistent), keeping source order within each
group, so the index is not the position in `topics.rs`, and adding a topic
can renumber others. When the list does not decode, the bus reads it as
plain integers, skips indices it does not know, and applies the rest; when
nothing usable remains, the connection keeps its previous set.

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
sticky replay) and, for `ClientConnected`, one message per identified
connection, including the subscriber's own if it has identified. Kinds
already in the set are not replayed, so sending the same set again does
nothing.

Subscription is by kind only, with no filter on keys, payload, or source. A
client subscribed to `MenuAction` receives every app's menu actions and
keeps those whose `app_id` is its own.

When the set includes `AppNotification`, `NotificationActivate`, or
`CalendarConfig`, `BusClient::subscribe` sends two frames: the set without
those kinds, then the full set, which replaces the first.

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
| `Zones`, `SessionApps`, `MailConfig`, `TerminalConfig`, `MonitorConfig`, `CalendarConfig` | `state.yaml` | A top-level key named after the topic, holding the payload |
| `FloatGeometry`, `Application`, `TerminalSession` | `state.yaml` | A top-level key holding a list, one entry per record |
| `Theme` | `theme/current.yaml` | The whole file |
| `CustomTheme` | `theme/presets/<name>.yaml` | One file per record |
| `BrowserConfig`, `BrowserHistory` | `browser.yaml`, `browser/history.yaml` | The whole file |
| `BrowserTab` | `browser/tabs/<id>.yaml` | One file per record |
| `PaintSession` | `paint.yaml` | The whole file |

The bus rewrites a file each time one of its records changes (a temporary
file renamed into place), and a retraction removes the entry or deletes the
file. It does not write a record whose key would make an unsafe file name
(empty, or containing `/`, NUL, or `..`), or a `CustomTheme` whose name is
not lowercase letters joined by single hyphens; it logs a warning instead.

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

*Acted on by* lists the processes that use a topic. Several apps (Shell,
Settings, Monitor, and others) subscribe to every kind and ignore most.

### Windows, input, and the compositor

| Topic | Payload | Published by | Acted on by | Kept |
|---|---|---|---|---|
| `Windows` | Every window: `window_id`, `app_id`, `title`, `pid` | `sola-river` | Shell, `sola-session`, Settings, Arcade; kit apps with a floating titlebar | Sticky |
| `OutputGeometry` | Output `width` and `height` | `sola-river`, when River reports the output size and after a bus reconnect | Shell, Arcade | Sticky |
| `WindowGeometry` | One window's `x`, `y`, `width`, `height` | `sola-river`; retracted when the window closes | Shell, Arcade | Sticky by `window_id` |
| `WindowFloating` | `window_id`, `floating` | Shell; `sola-river` re-sends its set after a bus reconnect | `sola-river`; kit apps with a floating titlebar | Sticky by `window_id` |
| `Composition` | Window ids to show, bottom to top | Shell | `sola-river` | Ephemeral |
| `Frame` | One window's position, size, and `fullscreen` flag | Shell | `sola-river` | Ephemeral |
| `Focus` | The `window_id` to focus | Shell; Paint, Design, Arcade (to raise themselves) | `sola-river` | Ephemeral |
| `AppHidden` | An `app_id` left out of `Composition` | Shell ([Hide](sola-shell.md#hide)); retracted to show it again | Shell | Sticky by `app_id` |
| `MouseEntered` | The `window_id` the pointer entered; `still` when the pointer did not move (the window under it changed), sent again without it on the next move | `sola-river` | Shell | Ephemeral |
| `MouseLeft` | None | `sola-river` | Shell | Ephemeral |
| `MouseClicked` | The `window_id` clicked | `sola-river` | Shell | Ephemeral |
| `RegisteredChords` | Chords (`keysym`, `modifiers`) to route to the bus, not the focused window | Shell | `sola-river` | Sticky |
| `Chord` | A registered chord was pressed | `sola-river` | Shell, Browser, Design | Ephemeral |
| `ChordReleased` | A registered chord was released | `sola-river` | Shell, Browser, Design | Ephemeral |

What `sola-river` does with these is in [Compositor](compositor.md).

### Apps, session, menus, and layout

| Topic | Payload | Published by | Acted on by | Kept |
|---|---|---|---|---|
| `LaunchApp` | `app_id`, `command` | Shell (launcher), Arcade | `sola-session` | Ephemeral |
| `LaunchResult` | `app_id`, `command`, `ok`, `error` | `sola-session` | Shell, Arcade | Ephemeral |
| `UserAppExited` | `app_id`, `command`, exit `code` or `signal` | `sola-session` | Shell, Arcade | Ephemeral |
| `CloseApp` | An `app_id` | Shell, a kit app's floating-titlebar close button, Arcade | `sola-session` (stops it), `sola-river` (closes its windows), the named app (quits) | Ephemeral |
| `CloseWindow` | A `window_id` | Shell (a tab bar row's stoplight, for a window that is not a tab) | `sola-river`: asks that window to close, as its own close button would | Ephemeral |
| `CloseTab` | A tab's `key` | Shell (a tab bar row's stoplight) | The app that opened the tab: closes its window as its own close button would | Ephemeral |
| `AgentIsland` | A session's `key` (its pane), `name`, `project`, `state` (waiting / done), `title`, `body`, `choices`, `reply`, `since` | Projects | Shell (its island in the top row) | Sticky by `key` |
| `AgentIslandAction` | An island's `key` and what was done: `Choose(index)`, `Reply(text)`, `Show` | Shell | Projects (types it into the session, or shows its tab) | Ephemeral |
| `Background` | `path` (Sola's copy under `~/.config/sola/background/`), `set_at` (ms) | Paint (Set as Background), Browser (Use as Desktop Background), Settings (a kept picture), `solactl background` | sola-river (draws it behind the windows) | Persistent; retracted for the black ground |
| `Appearance` | `adapt_theme` (the theme's colors follow the background picture), `strength` (0–100, default 60), `variation` (which scheme), `fade_idle` (default on), `fade_after_min` (default 5), `fade_over_min` (default 30), `dim_idle` (dim the picture once the fade is full; default on), `dim_after_min` (default 60), `dim_over_min` (default 30), `opacity` (the desk's at rest, %, 50–100, default 90), `colors` (`window` / `panels` / `highlight` / `accent` → `#rrggbb`: the look in use's), `customs` (your looks: `variation` + `colors` each), `custom` (the one in use, or none), `light` (web pages are told light; off, dark), `lock_idle` (lock when the idle fade is complete; off), `pictures` (each kept picture's path → the look it was last worn with: `adapt_theme`, `strength`, `variation`, `colors`, `customs`, `custom`) | Settings (Appearance); Shell (`pictures`, and a kept picture's look when it is set again) | Shell (rewrites `Theme`'s colors from the picture, or puts the greys back); sola-river (the desk's opacity, the idle fade); sola-browser (`light`: every page's `prefers-color-scheme`) | Persistent |
| `ThemeVariations` | `path` (the picture), `choices` (`name`, `base`, `raised`, `accent`, `source` — the picture's color — per scheme) | Shell | Settings (the scheme cards) | Sticky |
| `IdleKeep` | `x`, `y`, `w`, `h` (output-local logical pixels: the top row's flower and agent islands) | Shell | sola-river (the idle fade leaves it uncovered) | Sticky |
| `IdleFade` | `fading` (the desk is idle and fading into the picture), `out` (the fade is full: the windows are out of sight) | sola-river | Projects (a focused session's tab is not being looked at: its island shows); the shell (`out`: every tab window is reported off screen in `TabWindow`) | Sticky |
| `SessionApps` | Open apps: `app_id`, `command` | `sola-session` | `sola-session`, at its next start | Persistent |
| `Application` | A launcher entry: `app_id`, `label`, `command`, `icon`, `kind`, `url`, `profile` | Settings; Browser (web apps); Arcade (a running game's label, retracted when it stops) | Shell, Settings, Arcade, Browser | Persistent by `app_id` |
| `Shutdown` | None | Shell (Exit Sola); `sola` on SIGINT, SIGTERM, or SIGHUP | `sola`, `sola-session`, `sola-river` | Ephemeral |
| `SetAppMenu` | `app_id` and its menus: labels and items (action id, label, shortcut, disabled, checked) or dividers | Each app with a menu, at start | Shell (menubar) | Sticky by `app_id` |
| `MenuAction` | `app_id`, `action_id` of a chosen item or shortcut | Shell | The app named by `app_id` | Ephemeral |
| `Zones` | A zone per `app_id` (`Left`, `Right`, `Top`, `Bottom`, `TopMiddle`, `BottomMiddle`, `FullMiddle`, `MiddleRight`, `Fullscreen`, `Cinema`, `Float`) | Shell | Shell | Persistent |
| `FloatGeometry` | A floating app's last rectangle | Shell | Shell | Persistent by `app_id` |

Launching and closing are described in [Session manager](session.md) and
[Applications](applications.md).

### Theme and app state

| Topic | Payload | Published by | Acted on by | Kept |
|---|---|---|---|---|
| `Theme` | The active theme ([Theme and toolkit](theme.md)) | Kit; Shell at start when no saved theme exists | Every kit app | Persistent |
| `CustomTheme` | A theme preset: `name`, `theme` | Kit | Kit | Persistent by `name` |
| `MailConfig` | Mail accounts, identities, filter rules | Settings | Mail, Settings | Persistent |
| `CalendarConfig` | Calendar accounts and the calendar list | Settings, Calendar | Calendar, Settings | Persistent |
| `MailStatus` | `inbox_unread` | Mail; retracted when Mail quits | Shell (menubar count) | Sticky |
| `TerminalConfig` | Sidebar width, collapsed state, active tab | Terminal | Terminal | Persistent |
| `TerminalSession` | One tab: `id`, tmux session, working directory, order, pane layout | Terminal | Terminal | Persistent by `id` |
| `PaintSession` | Open file paths and the selected one | Paint | Paint | Persistent |
| `MonitorConfig`, `BrowserConfig`, `BrowserHistory`, `BrowserTab` | Declared only | Nothing | Nothing | Persistent (`BrowserTab` by `id`) |

No current app publishes or reads the last four; the bus still restores
their files if they exist.

### Handoff, notifications, and presence

| Topic | Payload | Published by | Acted on by | Kept |
|---|---|---|---|---|
| `OpenUrl` | `url`, `activate` | Browser, re-sending a URL handed to it so the shell raises it | Browser (new tab; ignores its own echo); Shell (raises Browser if running and `activate` is set, otherwise starts it with the URL) | Ephemeral |
| `OpenImage` | `path`, `activate`, optional `app_id` (absent means Paint) | A second Paint launch, handing paths to the running one | Paint, Preview (each only when addressed) | Ephemeral |
| `AppToast` | `text` | Nothing in Sola; `solactl emit` can | Shell (menubar, 5 seconds) | Ephemeral |
| `AppNotification` | `id`, `app_id`, `source`, `title`, `body`, optional `tag`, `tab_id`, `url` | Browser (pages and web apps), Calendar, Projects | Shell | Ephemeral |
| `NotificationActivate` | The clicked notification's `id`, `app_id`, `tab_id`, `url` | Shell | The app named by `app_id` | Ephemeral |
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

`sola` restarts `sola-bus` when it exits or its binary changes. The new
process starts with only what is on disk: persistent records come back;
sticky records, the roster, and every subscription are gone. `BusClient`
does not reconnect by itself; the program that owns it does:

| Program | Reconnects |
|---|---|
| Kit apps | Every 250 ms from the kit's bus poller, then re-sends the subscription given to `BusSetup` |
| `sola-river` | Every 20 ms, then re-subscribes and re-publishes `OutputGeometry`, `Windows`, each `WindowGeometry`, and its floating windows |
| `sola-session` | Blocks, retrying every second, then re-subscribes |
| `sola` | Once a second, then re-subscribes to `Shutdown` |

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
  subscribes, publishes `SetAppMenu`, and stores the client process-wide.
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
exits; there is no command to retract it. An unknown topic (the error lists
the valid names), invalid JSON, a payload that does not match, or an
unreachable bus exits with status 3.

To log every message the bus handles, start `sola` with `RUST_LOG`
including `sola_bus::traffic=trace` (for example
`RUST_LOG=info,sola_bus::traffic=trace`). Each message adds a line with its
connection, topic, `sticky` flag, source, and keys to
`/opt/sola/log/sola.log`.

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
