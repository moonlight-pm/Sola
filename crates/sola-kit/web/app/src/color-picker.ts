// Bespoke in-app color picker for the storybook. Avoids native
// <input type="color"> (which spawns the GTK color chooser dialog —
// requires GSettings schemas, looks foreign, dies on missing schemas).
//
// Usage:
//   ${() => pickerSwatch({
//     id: 'editor:accent',                     // unique per swatch
//     value: () => themeState.current.colors.accent,
//     onChange: (newValue) => setColor('accent', newValue),
//     className: 'kit-editor-swatch',          // sizing/look of the trigger
//   })}
//
// Module-local `local.openId` enforces single-popover-at-a-time across
// all swatch instances. Each instance maintains its own editState
// (reactive HSL+A) inside its component factory.

import { component, html, reactive } from '@arrow-js/core';

interface Rgba { r: number; g: number; b: number; a: number }
interface Hsl { h: number; s: number; l: number }

interface PickerSwatchOpts {
  id: string;
  value: () => string;
  onChange: (newValue: string) => void;
  className?: string;
}

const local = reactive<{ openId: string | null }>({ openId: null });

document.addEventListener('click', (e) => {
  if (local.openId === null) return;
  const path = (e.composedPath ? e.composedPath() : []) as EventTarget[];
  const insideSwatchOrPopover = path.some((n) => {
    const el = n as HTMLElement;
    return el.classList && (
      el.classList.contains('kit-swatch-trigger') ||
      el.classList.contains('kit-color-popover')
    );
  });
  if (!insideSwatchOrPopover) local.openId = null;
});

export const pickerSwatch = component((props: PickerSwatchOpts) => {
  const editState = reactive({ h: 0, s: 0, l: 0, a: 1 });
  const isOpen = () => local.openId === props.id;

  const onTrigger = (e: Event) => {
    e.stopPropagation();
    if (isOpen()) {
      local.openId = null;
      return;
    }
    const parsed = parseColor(props.value());
    if (parsed) {
      const hsl = rgbToHsl(parsed.r, parsed.g, parsed.b);
      editState.h = hsl.h; editState.s = hsl.s; editState.l = hsl.l; editState.a = parsed.a;
    }
    local.openId = props.id;
  };

  return html`<span
    class="${() => `kit-swatch-trigger ${props.className ?? ''}`}"
    style="${() => `background: ${props.value()}`}"
    @click="${onTrigger}"
  >${() => isOpen() ? colorPopover({ value: props.value, onChange: props.onChange, editState }) : null}</span>`;
});

interface PopoverOpts {
  value: () => string;
  onChange: (v: string) => void;
  editState: { h: number; s: number; l: number; a: number };
}

const colorPopover = component((props: PopoverOpts) => {
  const emit = () => {
    const s = props.editState;
    const rgb = hslToRgb(s.h, s.s, s.l);
    const r = Math.round(rgb.r);
    const g = Math.round(rgb.g);
    const b = Math.round(rgb.b);
    const hex2 = (n: number) => n.toString(16).padStart(2, '0');
    const out = s.a >= 1
      ? '#' + hex2(r) + hex2(g) + hex2(b)
      : '#' + hex2(r) + hex2(g) + hex2(b) + hex2(Math.round(s.a * 255));
    props.onChange(out);
  };

  const setH = (v: number) => { props.editState.h = v; emit(); };
  const setS = (v: number) => { props.editState.s = v; emit(); };
  const setL = (v: number) => { props.editState.l = v; emit(); };
  const setA = (v: number) => { props.editState.a = v; emit(); };

  const onHexInput = (e: Event) => {
    const v = (e.target as HTMLInputElement).value.trim();
    const parsed = parseColor(v);
    if (parsed === null) return;
    const hsl = rgbToHsl(parsed.r, parsed.g, parsed.b);
    props.editState.h = hsl.h; props.editState.s = hsl.s; props.editState.l = hsl.l; props.editState.a = parsed.a;
    props.onChange(v);
  };

  return html`<div class="kit-color-popover" @click="${(e: Event) => e.stopPropagation()}">
    <div class="kit-color-preview" style="${() => `background: ${props.value()}`}"></div>

    <div class="kit-slider-row">
      <span class="kit-slider-label">H</span>
      <input type="range" class="kit-slider kit-slider-h" min="0" max="360" step="1"
        value="${() => Math.round(props.editState.h)}"
        @input="${(e: Event) => setH(+(e.target as HTMLInputElement).value)}">
      <span class="kit-slider-value">${() => Math.round(props.editState.h)}</span>
    </div>

    <div class="kit-slider-row">
      <span class="kit-slider-label">S</span>
      <input type="range" class="kit-slider" min="0" max="100" step="1"
        value="${() => Math.round(props.editState.s)}"
        style="${() => `background: linear-gradient(to right, hsl(${props.editState.h}, 0%, ${props.editState.l}%), hsl(${props.editState.h}, 100%, ${props.editState.l}%))`}"
        @input="${(e: Event) => setS(+(e.target as HTMLInputElement).value)}">
      <span class="kit-slider-value">${() => Math.round(props.editState.s)}</span>
    </div>

    <div class="kit-slider-row">
      <span class="kit-slider-label">L</span>
      <input type="range" class="kit-slider" min="0" max="100" step="1"
        value="${() => Math.round(props.editState.l)}"
        style="${() => `background: linear-gradient(to right, hsl(${props.editState.h}, ${props.editState.s}%, 0%), hsl(${props.editState.h}, ${props.editState.s}%, 50%), hsl(${props.editState.h}, ${props.editState.s}%, 100%))`}"
        @input="${(e: Event) => setL(+(e.target as HTMLInputElement).value)}">
      <span class="kit-slider-value">${() => Math.round(props.editState.l)}</span>
    </div>

    <div class="kit-slider-row">
      <span class="kit-slider-label">A</span>
      <input type="range" class="kit-slider kit-slider-alpha" min="0" max="1" step="0.01"
        value="${() => props.editState.a}"
        style="${() => `background-image: linear-gradient(to right, hsla(${props.editState.h}, ${props.editState.s}%, ${props.editState.l}%, 0), hsla(${props.editState.h}, ${props.editState.s}%, ${props.editState.l}%, 1)), repeating-conic-gradient(#666 0% 25%, transparent 0% 50%); background-size: auto, 8px 8px`}"
        @input="${(e: Event) => setA(+(e.target as HTMLInputElement).value)}">
      <span class="kit-slider-value">${() => props.editState.a.toFixed(2)}</span>
    </div>

    <input type="text" class="kit-field kit-color-hex"
      value="${() => props.value()}"
      @input="${onHexInput}">
  </div>`;
});

// ---------- color math ----------

function parseColor(input: string): Rgba | null {
  const s = input.trim();
  // #RGB, #RGBA, #RRGGBB, #RRGGBBAA
  const hex = s.match(/^#([0-9a-f]{3,8})$/i);
  if (hex) {
    let h = hex[1];
    if (h.length === 3 || h.length === 4) {
      h = h.split('').map(c => c + c).join('');
    }
    if (h.length !== 6 && h.length !== 8) return null;
    return {
      r: parseInt(h.slice(0, 2), 16),
      g: parseInt(h.slice(2, 4), 16),
      b: parseInt(h.slice(4, 6), 16),
      a: h.length === 8 ? parseInt(h.slice(6, 8), 16) / 255 : 1,
    };
  }
  // rgb(...) and rgba(...) — accepted on input for backwards compat with
  // any hand-typed values; all picker output is hex.
  const rgba = s.match(/^rgba?\s*\(\s*(\d+)\s*,\s*(\d+)\s*,\s*(\d+)\s*(?:,\s*([\d.]+)\s*)?\)$/);
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

function rgbToHsl(r: number, g: number, b: number): Hsl {
  r /= 255; g /= 255; b /= 255;
  const max = Math.max(r, g, b);
  const min = Math.min(r, g, b);
  const l = (max + min) / 2;
  let h = 0, s = 0;
  if (max !== min) {
    const d = max - min;
    s = l > 0.5 ? d / (2 - max - min) : d / (max + min);
    if (max === r)      h = (g - b) / d + (g < b ? 6 : 0);
    else if (max === g) h = (b - r) / d + 2;
    else                h = (r - g) / d + 4;
    h *= 60;
  }
  return { h, s: s * 100, l: l * 100 };
}

function hslToRgb(h: number, s: number, l: number): { r: number; g: number; b: number } {
  s /= 100; l /= 100;
  const c = (1 - Math.abs(2 * l - 1)) * s;
  const x = c * (1 - Math.abs(((h / 60) % 2) - 1));
  const m = l - c / 2;
  let r = 0, g = 0, b = 0;
  if (h < 60)       { r = c; g = x; b = 0; }
  else if (h < 120) { r = x; g = c; b = 0; }
  else if (h < 180) { r = 0; g = c; b = x; }
  else if (h < 240) { r = 0; g = x; b = c; }
  else if (h < 300) { r = x; g = 0; b = c; }
  else              { r = c; g = 0; b = x; }
  return { r: (r + m) * 255, g: (g + m) * 255, b: (b + m) * 255 };
}
