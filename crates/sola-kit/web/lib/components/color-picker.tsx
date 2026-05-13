// ColorPicker — the HSL + alpha editor panel ColorInput shows
// inside its Popover. Adapted from the lit-based picker in the
// legacy sola-kit worktree (web/app/src/color-picker.ts), ported to
// Remix v3 with closure-captured state.
//
// Pure presentation + math: receives `value` (any CSS color
// expression we can parse — hex / hex+alpha / rgb / rgba), fires
// `onChange(newValue)` on every adjustment. Owns its own HSLA
// edit state so slider drags don't have to round-trip through the
// consumer per pixel.
//
// We don't reach for `<input type="color">` — WebKit on Linux
// spawns the GTK color chooser dialog, which looks foreign and
// needs GSettings schemas. This is a fully in-window picker.

import { type Handle } from "@remix-run/ui";
import { on } from "@sola/kit";
import { TextInput } from "@sola/text-input";

interface Rgba { r: number; g: number; b: number; a: number }
interface Hsla { h: number; s: number; l: number; a: number }

export interface ColorPickerProps {
  /** Current CSS color expression. Parsed into HSLA for editing;
      unparseable values keep the picker at its last good state. */
  value?: string;
  /** Fires whenever the user adjusts a slider or commits hex input. */
  onChange?: (value: string) => void;
}

export function ColorPicker(handle: Handle<ColorPickerProps>) {
  // Edit state is HSLA — sliders modify these; we lower to a hex
  // string on every change before firing `onChange`. The picker re-
  // initialises this from the incoming `value` whenever it changes
  // *externally* (i.e. between renders where the value isn't one
  // we just emitted).
  let edit: Hsla = { h: 0, s: 0, l: 0, a: 1 };
  // The hex field is its own draft so partial strings (`#a`, `#ab`)
  // can be typed without being reverted by Remix's controlled-input
  // reflection. The picker only updates HSLA when the draft parses.
  let hexDraft = "";
  let lastEmitted: string | null = null;
  let lastSeenValue: string | undefined = undefined;

  function syncFromExternal(value: string | undefined): void {
    if (value === lastSeenValue) return;
    lastSeenValue = value;
    // Don't re-sync if the new external value is one we just
    // emitted — the round-trip would jitter the slider positions
    // when a value parses-to-itself with rounding losses.
    if (value === lastEmitted) return;
    hexDraft = value ?? "";
    const parsed = parseColor(value ?? "");
    if (parsed === null) return;
    const hsl = rgbToHsl(parsed.r, parsed.g, parsed.b);
    edit = { h: hsl.h, s: hsl.s, l: hsl.l, a: parsed.a };
  }

  function emit(): void {
    const rgb = hslToRgb(edit.h, edit.s, edit.l);
    const hex2 = (n: number) => Math.round(n).toString(16).padStart(2, "0");
    const body = hex2(rgb.r) + hex2(rgb.g) + hex2(rgb.b);
    const out = edit.a >= 1
      ? `#${body}`
      : `#${body}${hex2(edit.a * 255)}`;
    hexDraft = out;
    lastEmitted = out;
    handle.props.onChange?.(out);
  }

  function setChannel(k: keyof Hsla, v: number): void {
    edit = { ...edit, [k]: v };
    emit();
    handle.update();
  }

  const onHexInput = (v: string) => {
    // Always accept the typed text — even partial / invalid — so
    // the hex field stays editable. HSLA + outward emit only happen
    // when the draft parses to a real color.
    hexDraft = v;
    const parsed = parseColor(v.trim());
    if (parsed !== null) {
      const hsl = rgbToHsl(parsed.r, parsed.g, parsed.b);
      edit = { h: hsl.h, s: hsl.s, l: hsl.l, a: parsed.a };
      lastEmitted = v;
      handle.props.onChange?.(v);
    }
    handle.update();
  };

  return () => {
    syncFromExternal(handle.props.value);
    const s = edit;
    const hue = Math.round(s.h);
    const sat = Math.round(s.s);
    const lum = Math.round(s.l);
    const alpha = s.a;

    const satStyle =
      `background: linear-gradient(to right, hsl(${hue}, 0%, ${lum}%), hsl(${hue}, 100%, ${lum}%))`;
    const lumStyle =
      `background: linear-gradient(to right, hsl(${hue}, ${sat}%, 0%), hsl(${hue}, ${sat}%, 50%), hsl(${hue}, ${sat}%, 100%))`;
    const alphaStyle =
      `background-image: linear-gradient(to right, hsla(${hue}, ${sat}%, ${lum}%, 0), hsla(${hue}, ${sat}%, ${lum}%, 1)), repeating-conic-gradient(#666 0% 25%, transparent 0% 50%); background-size: auto, 8px 8px`;
    const previewStyle =
      `background: ${handle.props.value ?? "transparent"}`;

    const sliderRow = (
      label: string,
      min: number,
      max: number,
      step: number,
      value: number,
      display: string,
      onInput: (v: number) => void,
      sliderClass: string,
      sliderStyle: string,
    ) => (
      <div class="sola-color-picker-row">
        <span class="sola-color-picker-label">{label}</span>
        <input
          type="range"
          class={`sola-color-picker-slider ${sliderClass}`}
          min={String(min)}
          max={String(max)}
          step={String(step)}
          value={String(value)}
          style={sliderStyle}
          mix={[on("input", (e: Event) => {
            const t = e.target as HTMLInputElement;
            onInput(Number(t.value));
          })]}
        />
        <span class="sola-color-picker-value">{display}</span>
      </div>
    );

    return (
      <div class="sola-color-picker">
        <div class="sola-color-picker-preview" style={previewStyle} />
        {sliderRow(
          "H",
          0,
          360,
          1,
          hue,
          String(hue),
          (v) => setChannel("h", v),
          "sola-color-picker-slider-h",
          "",
        )}
        {sliderRow(
          "S",
          0,
          100,
          1,
          sat,
          String(sat),
          (v) => setChannel("s", v),
          "",
          satStyle,
        )}
        {sliderRow(
          "L",
          0,
          100,
          1,
          lum,
          String(lum),
          (v) => setChannel("l", v),
          "",
          lumStyle,
        )}
        {sliderRow(
          "A",
          0,
          1,
          0.01,
          alpha,
          alpha.toFixed(2),
          (v) => setChannel("a", v),
          "sola-color-picker-slider-alpha",
          alphaStyle,
        )}
        <TextInput value={hexDraft} onInput={onHexInput} />
      </div>
    );
  };
}

// ---------- color math (ported from the legacy picker) ----------

function parseColor(input: string): Rgba | null {
  const s = input.trim();
  const hex = s.match(/^#([0-9a-f]{3,8})$/i);
  if (hex) {
    let h = hex[1];
    if (h.length === 3 || h.length === 4) {
      h = h.split("").map((c) => c + c).join("");
    }
    if (h.length !== 6 && h.length !== 8) return null;
    return {
      r: parseInt(h.slice(0, 2), 16),
      g: parseInt(h.slice(2, 4), 16),
      b: parseInt(h.slice(4, 6), 16),
      a: h.length === 8 ? parseInt(h.slice(6, 8), 16) / 255 : 1,
    };
  }
  const rgba = s.match(
    /^rgba?\s*\(\s*(\d+)\s*,\s*(\d+)\s*,\s*(\d+)\s*(?:,\s*([\d.]+)\s*)?\)$/,
  );
  if (rgba) {
    return {
      r: +rgba[1],
      g: +rgba[2],
      b: +rgba[3],
      a: rgba[4] === undefined ? 1 : +rgba[4],
    };
  }
  return null;
}

function rgbToHsl(r: number, g: number, b: number): { h: number; s: number; l: number } {
  r /= 255;
  g /= 255;
  b /= 255;
  const max = Math.max(r, g, b);
  const min = Math.min(r, g, b);
  const l = (max + min) / 2;
  let h = 0;
  let sat = 0;
  if (max !== min) {
    const d = max - min;
    sat = l > 0.5 ? d / (2 - max - min) : d / (max + min);
    if (max === r) h = (g - b) / d + (g < b ? 6 : 0);
    else if (max === g) h = (b - r) / d + 2;
    else h = (r - g) / d + 4;
    h *= 60;
  }
  return { h, s: sat * 100, l: l * 100 };
}

function hslToRgb(h: number, s: number, l: number): { r: number; g: number; b: number } {
  s /= 100;
  l /= 100;
  const c = (1 - Math.abs(2 * l - 1)) * s;
  const x = c * (1 - Math.abs(((h / 60) % 2) - 1));
  const m = l - c / 2;
  let r = 0;
  let g = 0;
  let b = 0;
  if (h < 60) { r = c; g = x; b = 0; }
  else if (h < 120) { r = x; g = c; b = 0; }
  else if (h < 180) { r = 0; g = c; b = x; }
  else if (h < 240) { r = 0; g = x; b = c; }
  else if (h < 300) { r = x; g = 0; b = c; }
  else { r = c; g = 0; b = x; }
  return { r: (r + m) * 255, g: (g + m) * 255, b: (b + m) * 255 };
}
