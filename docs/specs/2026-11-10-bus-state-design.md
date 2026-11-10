# Bus state: what the bus keeps, for whom, and for how long

**Status:** decided 2026-11-10, building on branch `bus-state`.

**Decided by Joshua (2026-11-10):** after a review of the bus ("Sticky
messages are suppose to be 'general' information every app should need --
not messages for specific apps right? Perhaps we need to investigate the
overall bus architecture again"), the hygiene, lossless delivery and owned
records below ("do A+B+D --- whatever those address are the answers to your
decisions"), and for a writer's records: kept across a restart, removed when
it ends for good.

## Why

Remote Desktop's Window menu showed two zones instead of eight. The bus had
dropped the zone layout from its replay. Then the review found that this was
one case of a general problem.

On novus with 86 windows open (census of the live bus, one connection per
kind, so nothing was dropped):

| Records | Count | Share | Who reads them |
|---|---|---|---|
| One per window or tab: `WindowGeometry` 86, `WindowChrome` 79, `WindowWorkspace` 77, `Tab` 72, `TabWindow` 72, `WindowTab` 72, `TabActivity` 38 | 496 | 86% | Usually one reader: the shell, river, or an app that wants only its own windows |
| One per app: `KeyActions` 29, `SetAppMenu` 19, `Application` 12 | 60 | 10% | The shell, Settings, every kit app for keys |
| One per desk | 22 | 4% | Many |

- **Replay loses records.** A subscribe replays every record of the kinds
  asked for into the client's 256-message queue with `try_send`, dropping
  the overflow silently, often with the trailing `Replayed`. Eleven apps and
  the shell subscribe to every kind (578 records). Only Monitor does so on
  purpose.
- **The shell can hang its windows.** It holds the window list until
  `Replayed` (`sola-shell/src/app/bus.rs`), with no fallback. A shell that
  restarts on a busy desk can lose the marker and never place windows.
- **Records outlive their writers.** The bus does nothing when a client goes.
  Remote Desktop's menu and key actions stayed after it quit; a removed web
  app's key actions stay on disk; the shell sends `floating: false` for a
  window that closed instead of retracting it.
- **The bus is used as private storage.** `TabLayout`, `FloatGeometry`,
  `AppHidden` (shell), `SessionApps` (session), `TerminalSession`
  (Terminal) and `PaintSession` (Paint) are read only by their writer.
- **Seven kinds are dead:** `TerminalConfig`, `MonitorConfig`,
  `BrowserConfig`, `BrowserHistory`, `BrowserTab`, `Zones`, `PinnedTabs`.
- **A menu storm.** Every `KeyActions` a kit app hears makes it publish all
  its menus again: one app start sends dozens of `SetAppMenu`.

Live traffic is small (about 8 messages a second, nearly all `Frame`). The
problem is the state, not the traffic.

## Decisions

1. **A sticky record is current state with one owner and a reader other
   than its writer.** Per-window state may stay on the bus, one record per
   window per owner.
2. **Private state leaves the bus.** What only its writer reads goes to the
   writer's own file.
3. **A record goes when its writer ends for good.** A writer that restarts
   (a service restarted by Oath or `sola`, an app re-executing itself after
   an update) keeps its records until it comes back and sends them again;
   whatever it does not send again goes then. No timers.
4. **Replay is lossless.** A client gets every record it asked for and then
   `Replayed`, or its connection is closed and it subscribes again.
5. Config shared by two processes (`MailConfig`, `CalendarConfig`: Settings
   writes, the app reads) stays on the bus; narrow subscriptions keep it
   from everyone else.

## Delivery (B)

- **Replay is one queue entry.** A client's queue carries either one
  message or a replay: the records, the roster for `ClientConnected`, and
  `Replayed`, in that order. The writer thread writes them one after
  another. It is queued under the bus lock, so live messages queue behind it
  and a client never sees an older replayed value after a newer live one.
- **A full queue closes the connection.** When a live message or a replay
  does not fit, the bus logs it and shuts that client's socket. The client
  reconnects (every client already does after a bus restart), subscribes
  again and gets a fresh replay. Nothing is dropped without the client
  knowing.
- The bus keeps each record's kind beside it, so a replay does not decode
  every stored record.

## Ownership (D1)

- **The owner of a sticky record is the process that wrote it last**: its
  app id and pid. The bus reads the pid from the socket (`SO_PEERCRED`).
  Persistent records are configuration and have no owner; they stay until
  retracted.
- **The bus watches each client's process** with a pidfd. When a process
  ends:
  - if it did not declare itself supervised, the bus retracts its records
    at once (subscribers get the retractions, as from the writer);
  - if it did (`$supervised`, sent after identify), its records stay. When a
    process with the same app id sends `$settled`, meaning it has sent its
    current state, the bus retracts the records the old process still owns.
- A process that only loses its connection (a bus restart, a closed slow
  connection, re-executing itself in place) keeps its pid, so nothing is
  removed. Its client re-sends its stickies on reconnect, as today.
- Supervised: `sola-river` and `sola-shell`, whose records describe windows
  that outlive them. River settles after its first render sequence (every
  window told, with its size); the shell after `Replayed` and placing the
  windows. The other services need no flag: `sola-vaultd`'s `VaultState`
  should go while the vault is down (it retracts it itself), and
  `sola-session` writes only persistent `SessionApps`.

## Per-window records (D2)

Seven per-window kinds and two per-tab kinds become three, each with one
owner:

| New | Owner | Replaces | Keyed by |
|---|---|---|---|
| `WindowFacts` | `sola-river` | `WindowGeometry`, `WindowTab`, `WindowParent` | `window_id` |
| `WindowPlace` | `sola-shell` | `WindowChrome`, `WindowFloating`, `WindowWorkspace`, `TabWindow` | `window_id` |
| `Tab` (gains `activity`) | the tab's app | `Tab`, `TabActivity` | `key` |

`WindowPlace` carries the window's tab key and whether it is shown, so an
app finds its tabs' windows there. River stops writing floating state of its
own; after a bus restart the shell's client re-sends its records.
`WindowVideos` (the browser's, a few records) stays.

## Private state off the bus (D3)

`TabLayout`, `FloatGeometry`, `AppHidden`, `SessionApps`,
`TerminalSession` and `PaintSession` move to their owner's own file under
`$XDG_STATE_HOME/sola/<app>/` (a `sola-core` helper: read, write
atomically). On first run the owner reads its old copy from where the bus
kept it (a `state.yaml` section or a namespaced file under the config dir),
so no layout, session or float place is lost.

## Hygiene (A)

- The dead kinds and the replaced ones are removed. `subscribe::NUMBERED`
  stays frozen; a removed name decodes to nothing. The bus skips their old
  `state.yaml` sections quietly.
- **Subscriptions name what the app reads.** The kit gives a baseline (what
  its frame, theme, quit and menus read); an app adds its own. Only Monitor
  (an audit view) and the shell (which reads nearly every kind) subscribe
  to everything.
- **No menu storm:** the kit marks its menus stale when keys change and
  publishes them once, when its poller has caught up with the bus.
- **Monitor lists every record**, by topic and keys.
- Removing a web app retracts its `KeyActions` and `SetAppMenu` too.

## Compatibility and rollout

Payload types change and kinds are added and removed, so the bus and every
Sola app must come from one build: installing this applies every Sola
package at once (the desk restarts, and every Sola app). An older app asks
for kinds by name; ones it asks for that are gone give it nothing. An older
bus treats `$supervised` and `$settled` as unknown topics and drops them.

## Expected result

On the same desk: about 315 records instead of 578 (172 per-window, 72
tabs, about 55 per-app, about 16 per-desk), every replay complete, records
gone with the apps that wrote them, and no app subscribed to kinds it does
not read.
