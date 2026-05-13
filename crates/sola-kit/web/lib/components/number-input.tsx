// NumberInput — a numeric editor with a trailing unit hint and
// step buttons.
//
// Layout:
//
//   ┌─────────────────────────┐
//   │  12        px   −   +   │
//   └─────────────────────────┘
//
// The value passed in/out is the *full token value* (e.g. `"12px"`):
// the component splits it into numeric + unit halves, edits the
// number internally, and re-serialises on every change. The unit
// shows as a small static label today; it's the seam for a future
// per-row unit selector (px / em / rem / %) without consumers
// having to change.
//
// The input is intentionally *uncontrolled* (no `value` prop on
// the underlying `<input>`) — same pattern as the ColorPicker
// sliders. Remix v3's controlled-input reflection schedules a
// microtask that rewrites the DOM value to the prior render's
// number on every native `input` event, which is why edits to the
// previous `<TextInput>`-based px fields were silently reverted.
// Here we own the input via a `ref` mixin and only push from JS
// when an external sync arrives.

import { type Handle, ref } from "@remix-run/ui";
import { on } from "@sola/kit";

export interface NumberInputProps {
  /** Current value as a full token string (`"12px"`). The component
      extracts the numeric portion for editing and re-attaches the
      unit on output. */
  value?: string;
  /** Unit suffix shown as a static hint and appended to emissions.
      Defaults to `"px"`. */
  unit?: string;
  /** Step size for the −/+ buttons. Defaults to `1`. */
  step?: number;
  /** Optional clamps on the step buttons (and on typed values). */
  min?: number;
  max?: number;
  /** Fires whenever the user commits a change — step click, typed
      digit, or external sync round-trip. Value is the full
      `<num><unit>` string. */
  onChange?: (value: string) => void;
}

export function NumberInput(handle: Handle<NumberInputProps>) {
  // Internal draft of the numeric portion as a plain string. We
  // keep it as a string (not a number) so partial input like `""`
  // or `"-"` while typing stays editable instead of collapsing to
  // `NaN`.
  let draft = "";
  let lastSeenValue: string | undefined = undefined;
  let lastEmitted: string | null = null;
  let inputEl: HTMLInputElement | null = null;

  function unit(): string {
    return handle.props.unit ?? "px";
  }

  function parseNumeric(s: string): number | null {
    // Accept the numeric prefix of any string — `"12px"` → 12,
    // `"4.5rem"` → 4.5, `"none"` → null. Tolerant on purpose so a
    // FontFamily value accidentally routed here doesn't crash.
    const m = String(s).match(/^\s*(-?\d+(?:\.\d+)?)/);
    return m ? parseFloat(m[1]) : null;
  }

  function syncFromExternal(value: string | undefined): void {
    if (value === lastSeenValue) return;
    lastSeenValue = value;
    if (value === lastEmitted) return;
    const parsed = parseNumeric(value ?? "");
    const next = parsed === null ? "" : String(parsed);
    if (next === draft) return;
    draft = next;
    if (inputEl && inputEl.value !== draft) inputEl.value = draft;
  }

  function clamp(n: number): number {
    let v = n;
    const { min, max } = handle.props;
    if (min !== undefined && v < min) v = min;
    if (max !== undefined && v > max) v = max;
    return v;
  }

  function emit(numStr: string): void {
    const out = `${numStr}${unit()}`;
    lastEmitted = out;
    handle.props.onChange?.(out);
  }

  function applyTypedValue(raw: string): void {
    // Accept anything that *could* lead to a number: empty,
    // lone minus, partial decimal. Only emit when it parses.
    draft = raw;
    if (raw === "" || raw === "-" || raw === "." || raw === "-.") return;
    const n = parseFloat(raw);
    if (Number.isNaN(n)) return;
    const clamped = clamp(n);
    if (clamped !== n) {
      // Out of range — snap the DOM and the draft back to the
      // clamped value so the user can see what happened.
      draft = String(clamped);
      if (inputEl) inputEl.value = draft;
    }
    emit(draft);
  }

  function stepBy(direction: 1 | -1): void {
    const step = handle.props.step ?? 1;
    const base = draft === "" ? 0 : (parseFloat(draft) || 0);
    const next = clamp(base + direction * step);
    // Strip trailing `.0` etc. by going through Number first.
    draft = String(Number(next.toFixed(6)).valueOf());
    if (inputEl) inputEl.value = draft;
    emit(draft);
    handle.update();
  }

  const onDecClick = (e: Event) => {
    e.stopPropagation();
    stepBy(-1);
  };
  const onIncClick = (e: Event) => {
    e.stopPropagation();
    stepBy(1);
  };

  return () => {
    syncFromExternal(handle.props.value);

    return (
      <span class="sola-number-input">
        <input
          type="text"
          inputmode="decimal"
          class="sola-number-input-field"
          mix={[
            ref<HTMLInputElement>((el, signal) => {
              inputEl = el;
              el.value = draft;
              el.addEventListener("input", (ev) => {
                applyTypedValue((ev.target as HTMLInputElement).value);
              }, { signal });
            }),
          ]}
        />
        <span class="sola-number-input-unit">{unit()}</span>
        <button
          type="button"
          class="sola-number-input-step"
          aria-label="Decrease"
          mix={[on("click", onDecClick)]}
        >
          {minusIcon()}
        </button>
        <button
          type="button"
          class="sola-number-input-step"
          aria-label="Increase"
          mix={[on("click", onIncClick)]}
        >
          {plusIcon()}
        </button>
      </span>
    );
  };
}

// Inline lucide minus / plus icons. Strokes use `currentColor` so
// the buttons pick up the theme's text-secondary tint by default.
function minusIcon() {
  return (
    <svg
      class="sola-number-input-icon"
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      stroke-width="2"
      stroke-linecap="round"
      stroke-linejoin="round"
      aria-hidden="true"
    >
      <path d="M5 12h14" />
    </svg>
  );
}

function plusIcon() {
  return (
    <svg
      class="sola-number-input-icon"
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      stroke-width="2"
      stroke-linecap="round"
      stroke-linejoin="round"
      aria-hidden="true"
    >
      <path d="M5 12h14" />
      <path d="M12 5v14" />
    </svg>
  );
}
