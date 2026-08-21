# Monitor

**Status:** partial (installed on the local desk 2026-08-21; smoke pending)

sola-monitor is the desk inspector for the two IPC planes.

## Planes

- **Bus** — fan-out facts. Live log of topics, sources, and payloads. The right rail is last-known sticky state (`Theme`, `Windows`, …).
- **Call** — request/reply. Live log of invokes (owner.method, caller, status, duration). The right rail is the live catalog of advertised owners and methods.

The app connects to `sola-call` as an **observer** (not a provider). It does not put RPC on the bus.

## Using it

- Filter box matches topic/source/payload (Bus) or owner/method/caller (Call).
- Topic / owner dropdown narrows the log.
- **Pause** holds new traffic in a buffer (count on Resume). **Clear** drops the current plane’s log, not stickies or the catalog.
- Click a log row, sticky, or catalog method to fill the **inspector** well under the log. The table stays one line.
- The log follows the tail until you scroll up; scroll back to the bottom to resume.

Quit is **Monitor → Quit Monitor** (⌘Q).

## Not in this pass

Copy-from-inspector; mixed Bus+Call log; catalog as a bus sticky (monitor speaks the call observer protocol instead).
