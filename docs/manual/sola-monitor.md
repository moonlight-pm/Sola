# Monitor

Monitor (`sola-monitor`) is the desk inspector for Sola's two IPC planes:
the bus and the call plane.

Monitor does not ship on Oath: `pkg:sola` does not carry `sola-monitor`.
The launcher still lists **Monitor**, and picking it there fails (the top
row says it could not launch). The rest of this note is how the app works
where it is installed.

## Planes

Pick a plane in the left rail.

- **Bus** — fan-out facts. A live log of time, topic, source, and payload.
  The right rail is the last-known sticky state (`Theme`, `Windows`, …).
- **Call** — request/reply. A live log of invokes: time, `owner.method`,
  caller, status with its duration, and the parameters. The right rail is
  the live catalog of advertised owners and methods. The plane's subtitle
  says **Host down** while `sola-call` is not running.

Monitor connects to `sola-call` as an **observer**, not a provider. It does
not put RPC on the bus.

## Using it

- The filter box matches topic, source, or payload (Bus), or owner,
  method, caller, parameters, or result (Call). Matching ignores case.
- The topic or owner dropdown narrows the log.
- **Pause** holds new traffic in a buffer, and **Resume** shows its
  count. **Clear** drops the current plane's log, not the stickies or
  the catalog.
- Click a log row, a sticky, or a catalog method to fill the **inspector**
  well under the log; click it again to clear the inspector. The table
  stays one line per row.
- The log follows the tail until you scroll up; scroll back to the bottom
  to resume.
- Each plane keeps its last 5,000 entries.

Quit is **Monitor → Quit Monitor** (⌘Q), in the titlebar.

## Limits

- You cannot copy from the inspector.
- Bus and Call traffic are not shown in one mixed log.
- The call catalog is not a bus sticky; Monitor reads it through the call
  observer protocol instead.
