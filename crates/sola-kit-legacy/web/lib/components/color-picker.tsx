// ColorPicker — the HSL + alpha editor panel an editable Swatch
// shows inside its Popover. Adapted from the lit-based picker in
// the legacy sola-kit worktree (web/app/src/color-picker.ts),
// ported to Remix v3 with closure-captured state.
//
// Pure presentation + math: receives `value` (any CSS color
// expression we can parse — hex / hex+alpha / rgb / rgba), fires
// `onChange(newValue)` on every adjustment. Owns its own HSLA
// edit state so slider drags don't have to round-trip through the
// consumer per pixel.
//
// The hex field carries a copy-to-clipboard button in its trailing
// slot. Clicking copies the current draft value verbatim; the icon
// swaps to a checkmark for ~900ms as non-blocking confirmation.
// Living here (rather than next to the swatch trigger) keeps the
// copy affordance discoverable when the user is actually inspecting
// a value, and stops the swatch — which now carries the editable
// semantics directly — from also juggling clipboard chrome.
//
// We don't reach for `<input type="color">` — WebKit on Linux
// spawns the GTK color chooser dialog, which looks foreign and
// needs GSettings schemas. This is a fully in-window picker.

import { type Handle, ref } from "@remix-run/ui";
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
  // `copied` flips true for ~900ms after a successful copy so the
  // trailing icon swaps to a check mark. Non-blocking confirmation
  // without a separate toast/snackbar component.
  let copied = false;
  let copiedTimer: number | null = null;

  const onCopy = async (e: Event) => {
    // Stop the event reaching the popover-root listener — clicking
    // the trailing button is a separate action, not a trigger
    // toggle.
    e.stopPropagation();
    if (!hexDraft) return;
    try {
      await navigator.clipboard.writeText(hexDraft);
      copied = true;
      handle.update();
      if (copiedTimer !== null) clearTimeout(copiedTimer);
      copiedTimer = setTimeout(() => {
        copied = false;
        copiedTimer = null;
        handle.update();
      }, 900) as unknown as number;
    } catch (err) {
      console.error("ColorPicker copy failed", err);
    }
  };
  // Slider DOM refs, captured by the `ref` mixin. The sliders are
  // intentionally uncontrolled: passing `value` would trip Remix's
  // controlled-input reflection, which schedules a microtask that
  // reverts the DOM value to the previous render's number — racing
  // the browser's native drag handling and snapping the thumb back
  // by one tick per move. We set initial values from the ref
  // callback and push externally-driven changes (hex edits, theme
  // refreshes) via `pushDomValues`; drags are read-only from JS
  // and the DOM stays authoritative.
  const sliderEls: Partial<Record<keyof Hsla, HTMLInputElement>> = {};

  function pushDomValues(): void {
    for (const k of ["h", "s", "l", "a"] as const) {
      const el = sliderEls[k];
      if (!el) continue;
      const v = String(edit[k]);
      if (el.value !== v) el.value = v;
    }
  }

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
    pushDomValues();
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
      // Hex was the source of truth here; reflect to sliders.
      pushDomValues();
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
      channel: keyof Hsla,
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
          style={sliderStyle}
          mix={[
            // One ref does both jobs: captures the slider node,
            // seeds its initial value, and attaches the `input`
            // listener directly via addEventListener with the
            // ref's AbortSignal handling cleanup. Avoiding the
            // separate `on` mixin keeps things obvious — there's
            // exactly one place per slider where the listener is
            // wired up, and the listener captures the same
            // closure (`onInput`) that the render passed in.
            ref<HTMLInputElement>((el, signal) => {
              sliderEls[channel] = el;
              el.value = String(value);
              el.addEventListener(
                "input",
                (e) => {
                  const t = e.target as HTMLInputElement;
                  onInput(Number(t.value));
                },
                { signal },
              );
            }),
          ]}
        />
        <span class="sola-color-picker-value">{display}</span>
      </div>
    );

    return (
      <div class="sola-color-picker">
        <div class="sola-color-picker-preview" style={previewStyle} />
        {sliderRow(
          "H",
          "h",
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
          "s",
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
          "l",
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
          "a",
          0,
          1,
          0.01,
          alpha,
          alpha.toFixed(2),
          (v) => setChannel("a", v),
          "sola-color-picker-slider-alpha",
          alphaStyle,
        )}
        <TextInput
          value={hexDraft}
          onInput={onHexInput}
          trailing={
            <button
              type="button"
              class="sola-color-picker-copy"
              aria-label={copied ? "Copied" : "Copy color value"}
              title={copied ? "Copied" : "Copy color value"}
              disabled={hexDraft ? false : true}
              mix={[on("click", onCopy)]}
            >
              {copied ? checkIcon() : copyIcon()}
            </button>
          }
        />
      </div>
    );
  };
}

// Inline lucide icons (24×24 viewBox, strokes use currentColor).
// Inlined rather than fetched from /opt/sola/share/icons because the
// kit doesn't yet mount the icon directory under its app:// scheme.
function copyIcon() {
  return (
    <svg
      class="sola-color-picker-copy-icon"
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      stroke-width="2"
      stroke-linecap="round"
      stroke-linejoin="round"
      aria-hidden="true"
    >
      <rect width="14" height="14" x="8" y="8" rx="2" ry="2" />
      <path d="M4 16c-1.1 0-2-.9-2-2V4c0-1.1.9-2 2-2h10c1.1 0 2 .9 2 2" />
    </svg>
  );
}

function checkIcon() {
  return (
    <svg
      class="sola-color-picker-copy-icon"
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      stroke-width="2"
      stroke-linecap="round"
      stroke-linejoin="round"
      aria-hidden="true"
    >
      <path d="M20 6 9 17l-5-5" />
    </svg>
  );
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
