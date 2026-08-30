# Shell menubar Bluetooth widget

**Date:** 2026-08-29  
**Status:** Frozen — implemented in `sola-shell`; installed 2026-08-29 (desk smoke pending)  
**Related:** [shell iced](2026-05-22-sola-shell-iced-port-design.md); [system monitors](2026-06-16-menubar-system-monitors-design.md); [omarchy consideration](../ideas/2026-08-22-omarchy-consideration.md) (calendar / audio / bluetooth as shell popovers, not a Waybar)  
**Implementation:** `crates/sola-shell/src/bluetooth/` + menubar right-cluster icon + `Panel::Bluetooth` on the existing Menu overlay  
**Dogfood:** `shell` installed debug 2026-08-29. Host BlueZ on (novus `nixos-rebuild` 2026-08-29; Intel AX210 `hci0` powered). Desk smoke of the popover pending.  
**Gaps:** no Forget / unpair; no audio-profile picker; no adapter chooser when several exist; pairing agent is KeyboardDisplay on the shell connection (not a call-plane D3 confirm)

## Intent

A Bluetooth control in the **Mac-shaped** sola-shell menubar. The bar stays a menu bar: one quiet icon in the right cluster, click opens a popover. Not a rearrangeable status bar, not a second overlay window, not `bluetoothctl`.

## Product rules

| Rule | Choice |
|------|--------|
| Icon | Right cluster, **left of the stats** (system-control, not a metric). Full-height hit (`bar_button`). `lucide/bluetooth` / `bluetooth-off` / `bluetooth-connected`. Theme `text` / muted — **no accent**, no view-local hex. |
| Off vs on | Readable on the icon. Adapter powered-off → `bluetooth-off` + muted. Powered, nothing connected → `bluetooth`. Powered + ≥1 connected → `bluetooth-connected`, still quiet (same fg as on). |
| No adapter | **Hide** the icon (same honesty as GPU hiding without NVML). Powered-off adapter still shows. |
| Click | Existing **Menu** overlay, `Panel::Bluetooth`. Kit `popover`, card width 320 (stats / notify pile family). |
| Power | Adapter **toggler** in the popover (kit `form_row` + `toggle_style`). A widget that cannot turn Bluetooth on is a dead icon. |
| Connected | Name, plus **battery % only when BlueZ `Battery1.Percentage` (or equivalent) is present**. Never fake 100%. **Disconnect** does not unpair. |
| Paired, idle | Listed under the connected set so reconnect works without inquiry. **Connect**. |
| Add | **Add device** starts discovery; nearby unpaired appear below. **Pair** then **Connect**. **Done** (or panel close) stops inquiry. Do not leave an inquiry scan running. Nearby omits labels that are a 6-pair hex address (`AA-BB-CC-DD-EE-FF` or colons). |
| Forget | **Not v1.** No mystery “remove”. |
| Pairing UI | BlueZ **Agent1** inline in this popover (PIN / passkey / confirm / incoming allow). Desk-local shell, **not** sola-call **D3**. |
| Sampling | In-process `zbus` on the **system** bus (`org.bluez`). Background thread + channel into iced. Slow poll + ObjectManager signals. Opening the panel refreshes immediately. **No 16ms timer.** No new bus topic, no new daemon. |

## Layout

Menubar right cluster, left → right:

```
[mail?] [notify?] [bluetooth?]  CPU  GPU?  MEM  RX  TX  │  clock
```

Popover (Menu window, anchored under the icon, 8px gutter):

```
Bluetooth                              [toggler]
────────────────────────────────────────────────
WH-1000XM5                      72%    Disconnect
MX Master 3                            Disconnect

Not connected
Keychron K2                            Connect

[ Add device ]
```

While adding, a **Nearby** list of named/typed unpaired devices and **Pair**. Anonymous HEX addresses are omitted. Copy while searching: “Put the device in pairing mode.” Agent prompts replace the footer until resolved. Empty powered-on: “No connected devices.” Powered-off: “Bluetooth is off.”

## Architecture

```
sola-shell (iced daemon)
  menubar chip  →  Msg::ToggleBluetooth
  Menu overlay  →  Panel::Bluetooth  →  bluetooth::view
  bluetooth/ worker thread
       zbus system bus → org.bluez
       Adapter1 / Device1 / Battery1 / ObjectManager
       Agent1 at /org/sola/shell/agent (KeyboardDisplay)
       mpsc Event → iced
       Command ← UI (power, discover, pair, connect, disconnect, agent reply)
```

Discovery is a command (`SetDiscovering`), not a standing scan. Panel close / `Done` sends `SetDiscovering(false)`.

First adapter wins in v1 (`/org/bluez/hciN` with `Adapter1`).

## Out of scope

- Forget / unpair, trusted-flag editor, adapter picker
- A2DP / codec / input-device settings
- `org.freedesktop.Notifications` / blueman
- `solactl bluetooth` (no second consumer yet)
- Waybar-style module rearrange
