# Shell menubar volume and devices

**Date:** 2026-08-29  
**Status:** Frozen — implemented in `sola-shell`; 12-band spectrum on the chip 2026-09-02  
**Related:** [shell iced](2026-05-22-sola-shell-iced-port-design.md); [system monitors](2026-06-16-menubar-system-monitors-design.md); [Bluetooth menubar](2026-08-29-shell-bluetooth-menubar-design.md); [omarchy consideration](../ideas/2026-08-22-omarchy-consideration.md) (audio mixer as a shell popover, not a Waybar)  
**Implementation:** `crates/sola-shell/src/audio/` + menubar right-cluster icon + LED spectrum analyzer + `Panel::Audio` on the existing Menu overlay  
**Dogfood:** `shell` installed debug 2026-08-29; 12-band spectrum **installed** `shell` release 2026-09-02 and desk-smoked on Spotify (presence-band retune). Still pending: volume keys vs chip, sink/source switch.  
**Gaps:** no per-app streams; no Bluetooth profile/codec; host PipeWire + WirePlumber (`pw-dump`, `wpctl`, `pw-cat`)

## Intent

A volume control in the **Mac-shaped** sola-shell menubar. Media keys already change the default sink (`solactl media` → `wpctl`). The bar must **show** that level, and the popover must pick **output** and **input** devices.

## Product rules

| Rule | Choice |
|------|--------|
| Icon | Right cluster, **right of Bluetooth** / immediately **left of the stats** (system-control, not a metric). Full-height `bar_button`. Lucide `volume` / `volume-1` / `volume-2` / `volume-x` (muted) plus a **12-band LED spectrum analyzer** (same 2×2 dither as CPU/MEM; each band is 3 LED columns; ~3× the stats-graph width). Phosphor stack is green → teal → azure → violet → amber by height. Glyph still encodes quiet / mid / loud / muted. |
| No PipeWire | **Hide** the chip (same honesty as GPU / Bluetooth adapter). |
| Click | Existing **Menu** overlay, `Panel::Audio`. Kit `popover`, card width 320. |
| Output | Slider for the **default sink** (same object media keys move). Mute on that sink. List sinks; click sets default (`wpctl set-default`). |
| Input | Slider + mute for the **default source**. List sources; click sets default. Omit sink **monitors** (`.monitor`) and WirePlumber internal capture nodes. |
| Keys | `XF86AudioRaiseVolume` / `LowerVolume` / `Mute` stay `solactl media`. The chip **follows** those changes (refresh after the chord + poll). |
| Sampling | In-process: `pw-dump` (device graph) + `wpctl` (get-volume / set-volume / set-mute / set-default / inspect). Background thread. ~1s poll, plus refresh on panel open and after media keys. **No 16ms iced timer.** Spectrum is a separate `pw-cat` tap on the default sink (`stream.capture.sink`, `node.passive`) → 2048-point FFT → 12 power-warped log bands (~55 Hz–6.5 kHz, more bars on the treble, constant-Q overlap) + pink (~+4 dB/oct) weight → peak-hold autoscale (loudest recent band = full height); the canvas uses `RedrawRequest::At(50ms)` only while bands are live (same At pattern as the kit working ring). No new bus topic, no pavucontrol. |

## Layout

```
[mail?] [notify?] [bluetooth?] [volume ████████████]  CPU …  clock
```

Popover:

```
Output                         90%
[==============|=======]           mute
  HDMI (Dell U4025QW)               ✓
  WH-CH520

Input                         100%
[======================]           mute
  WH-CH520                          ✓
```

Empty input: “No input devices.” Slider is 0–100, capped at 100% (same as media-key `-l 1.0`).

## Architecture

```
media keys  →  solactl media  →  wpctl (default sink)
                 ↘ audio::Refresh
sola-shell audio worker
  pw-dump + wpctl inspect/get-volume  →  Snapshot  →  iced
  Command (volume / mute / default)   →  wpctl
sola-shell audio meter
  pw-cat (default-sink monitor, passive)  →  FFT → 12 warped-log bands + pink weight
  canvas RedrawRequest::At(50ms) while live
```

First matching `Audio/Sink` / `Audio/Source` nodes from `pw-dump`. Default ids from `wpctl inspect @DEFAULT_AUDIO_SINK@` / `@DEFAULT_AUDIO_SOURCE@`.
