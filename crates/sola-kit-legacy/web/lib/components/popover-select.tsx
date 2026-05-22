// PopoverSelect — a Popover-backed dropdown that behaves like a
// native HTML `<select>` for sizing, but renders inside the page
// (no OS-level popup window — sola-river would see CEF's native
// `<select>` as a real top-level surface).
//
// Sizing model — three modes via the `width` prop:
//
//   "auto"  (default)  trigger min-width = widest option's natural
//                      label width, measured synchronously via
//                      @chenglou/pretext on the trigger's computed
//                      font. Matches the closed-width behaviour of
//                      native <select>.
//   "fill"             trigger fills its parent (grid cell, flex
//                      item, etc.). Useful when the caller controls
//                      column width and just wants the trigger to
//                      stretch.
//   "<css length>"     explicit width override. Useful for
//                      coordinating a row of selects to a single
//                      caller-computed max — see BindingsEditor.
//
// Pretext measures text via canvas `measureText` without touching
// the DOM, so there's no layout reflow per remeasure. We invalidate
// the cached width when (a) the option list's label set changes or
// (b) a Topic::Theme delivery lands (which may have flipped the
// font family / size). The hash check inside `measure()` makes per-
// render calls a no-op when nothing relevant changed.

import { type Handle } from "@remix-run/ui";
import { ref } from "@remix-run/ui";
import {
  measureNaturalWidth,
  prepareWithSegments,
} from "@chenglou/pretext";
import { onThemeChange } from "@sola/kit";
import { on } from "@sola/kit";
import { Popover, type PopoverPlacement } from "@sola/popover";

export interface PopoverSelectOption {
  /** Value passed to `onChange` when selected. */
  value: string;
  /** Visible label. Defaults to `value`. */
  label?: string;
}

export type PopoverSelectWidth = "auto" | "fill" | string;

export interface PopoverSelectProps {
  options: PopoverSelectOption[];
  value: string;
  onChange: (value: string) => void;
  /** Trigger sizing. See module header for behaviour of each mode. */
  width?: PopoverSelectWidth;
  /** Text shown in the trigger when `value` isn't in `options`. */
  placeholder?: string;
  /** Forwarded to the underlying Popover. */
  placement?: PopoverPlacement;
}

export function PopoverSelect(handle: Handle<PopoverSelectProps>) {
  // Captured by the `ref` mixin on the trigger span; cleared on
  // remove. Used by `measure()` to read computed font and padding.
  let triggerEl: HTMLElement | null = null;

  // Measured `min-width` in px, or `null` if we haven't measured
  // yet (either no trigger captured or in "fill" / explicit-width
  // mode where measurement is skipped).
  let measuredMinWidth: number | null = null;

  // Cache key for the last measurement — joined option labels +
  // canvas-style font. measure() short-circuits if this hasn't
  // changed.
  let lastHash: string = "";

  function measure() {
    if (!triggerEl) return;
    const widthMode = handle.props.width;
    if (widthMode !== undefined && widthMode !== "auto") return;

    const opts = handle.props.options;
    const cs = getComputedStyle(triggerEl);
    // Canvas-style font string: `<style> <weight> <size> <family>`.
    // measureText needs the whole shorthand; computed style hands
    // us each piece resolved (font-size in px, font-family with
    // quotes preserved).
    const font = `${cs.fontStyle} ${cs.fontWeight} ${cs.fontSize} ${cs.fontFamily}`;

    const labels = opts.map((o) => o.label ?? o.value);
    const hash = labels.join("\x01") + "\x02" + font;
    if (hash === lastHash) return;
    lastHash = hash;

    if (opts.length === 0) {
      measuredMinWidth = null;
      handle.update();
      return;
    }

    let widest = 0;
    for (const label of labels) {
      const prepared = prepareWithSegments(label, font);
      const w = measureNaturalWidth(prepared);
      if (w > widest) widest = w;
    }

    const padL = parseFloat(cs.paddingLeft) || 0;
    const padR = parseFloat(cs.paddingRight) || 0;
    // The trigger is `display: inline-flex` with a `gap`; gap shows
    // up as columnGap when there's a row layout. Fall back to
    // generic `gap` for completeness.
    const gap = parseFloat(cs.columnGap || cs.gap || "0") || 0;
    // Chevron is a 12×12 SVG — see the corresponding CSS rule.
    const chevronWidth = 12;
    // +2px of slack so the measured label doesn't graze the chevron
    // glyph stem on sub-pixel rounding. Cheap insurance.
    measuredMinWidth = Math.ceil(
      widest + padL + padR + gap + chevronWidth + 2,
    );
    handle.update();
  }

  // Re-measure on theme deliveries — the font tokens may have
  // flipped (e.g. `--font-mono` swapped out). Pretext is fast
  // enough that we don't bother diffing first; the hash inside
  // measure() will short-circuit if the font ended up the same.
  let setupComplete = false;
  // deno-lint-ignore no-unused-vars
  const _dispose = onThemeChange(() => {
    if (!setupComplete) return;
    // Clear the hash so a same-options remeasure runs with the new
    // font.
    lastHash = "";
    measure();
  });
  setupComplete = true;

  const onTriggerRef = (node: Element) => {
    triggerEl = node as HTMLElement;
    // Computed-style values aren't available until the node is in
    // the document and styles have resolved; queueMicrotask defers
    // past the synchronous insert callback.
    queueMicrotask(measure);
  };

  return () => {
    const { options, value, onChange, width, placeholder, placement } =
      handle.props;

    // Cheap to call every render — measure() hashes labels+font and
    // short-circuits when nothing changed. Handles parent-driven
    // option-list updates (BindingsEditor swapping candidate sets
    // across slots).
    queueMicrotask(measure);

    const selected = options.find((o) => o.value === value);
    const triggerLabel = selected
      ? (selected.label ?? selected.value)
      : (placeholder ?? value);

    // Sizing — see module header. Inline `style` only; the CSS
    // file deliberately doesn't set width so we don't fight over
    // it.
    const triggerStyle = (() => {
      if (width === "fill") return "width: 100%";
      if (width && width !== "auto") return `width: ${width}`;
      if (measuredMinWidth != null) return `min-width: ${measuredMinWidth}px`;
      return "";
    })();

    // In "fill" mode we also need the wrapping `.sola-popover-root`
    // to be 100% — by default it's `width: max-content` and would
    // collapse to the trigger's content size. A wrapper class
    // scoped to PopoverSelect lets us override popover-root width
    // without touching the Popover component itself.
    const wrapperClass = width === "fill"
      ? "sola-popover-select sola-popover-select-fill"
      : "sola-popover-select";

    const renderContent = ({ close }: { close: () => void }) => (
      <div class="sola-popover-select-list">
        {options.map((opt) => {
          const isSelected = opt.value === value;
          const cls = "sola-popover-select-option" +
            (isSelected ? " is-selected" : "");
          return (
            <button
              type="button"
              class={cls}
              mix={[
                on("click", () => {
                  onChange(opt.value);
                  close();
                }),
              ]}
            >
              {opt.label ?? opt.value}
            </button>
          );
        })}
      </div>
    );

    return (
      <span class={wrapperClass}>
        <Popover content={renderContent} placement={placement}>
          <span
            class="sola-popover-select-trigger"
            style={triggerStyle}
            mix={[ref(onTriggerRef)]}
          >
            <span class="sola-popover-select-trigger-label">
              {triggerLabel}
            </span>
            {chevronIcon()}
          </span>
        </Popover>
      </span>
    );
  };
}

function chevronIcon() {
  return (
    <svg
      class="sola-popover-select-chevron"
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      stroke-width="2"
      stroke-linecap="round"
      stroke-linejoin="round"
      aria-hidden="true"
    >
      <path d="m6 9 6 6 6-6" />
    </svg>
  );
}
