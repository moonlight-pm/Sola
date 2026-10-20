# Monitor

Monitor (`sola-monitor`) is the desk inspector for Sola's two IPC planes:
the bus and the call plane.

## Planes

- **Bus** — fan-out facts. A live log of topics, sources, and payloads. The
  right rail is the last-known sticky state (`Theme`, `Windows`, …).
- **Call** — request/reply. A live log of invokes (owner.method, caller,
  status, duration). The right rail is the live catalog of advertised
  owners and methods.

Monitor connects to `sola-call` as an **observer**, not a provider. It does
not put RPC on the bus.

## Using it

- The filter box matches topic, source, or payload (Bus), or owner,
  method, or caller (Call).
- The topic or owner dropdown narrows the log.
- **Pause** holds new traffic in a buffer, and **Resume** shows its
  count. **Clear** drops the current plane’s log, not the stickies or
  the catalog.
- Click a log row, a sticky, or a catalog method to fill the **inspector**
  well under the log. The table stays one line per row.
- The log follows the tail until you scroll up; scroll back to the bottom
  to resume.

Quit is **Monitor → Quit Monitor** (⌘Q).

## Limits

- You cannot copy from the inspector.
- Bus and Call traffic are not shown in one mixed log.
- The call catalog is not a bus sticky; Monitor reads it through the call
  observer protocol instead.
