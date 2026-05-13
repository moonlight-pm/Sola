// ColorInput — a Swatch trigger that opens a ColorPicker popover,
// with a hover-revealed copy button beside the swatch.
//
// The Swatch previews the current value as a CSS color; clicking it
// pops the picker open. The picker fires `onChange` whenever the
// user adjusts a slider or commits hex input; ColorInput forwards
// that straight to its own consumer.
//
// The hover copy button writes the current value verbatim to the
// system clipboard (via `navigator.clipboard.writeText`). Paste
// happens through any normal text input (the picker's hex field
// included) — no special integration on the receiving side. The
// button lives outside the Popover's trigger element so it does
// not toggle the popover; that means clicking it while the picker
// is open will close the picker (via the document-level
// click-outside listener), which is the right behaviour: the
// commit has already gone through `onChange`.
//
// "Color expression" means any CSS color string the picker can
// parse: hex (`#0d1117`), hex+alpha (`#0d1117cc`), rgb / rgba
// (`rgba(0, 212, 255, 0.5)`). Strings that don't parse (named
// colors, var references, color-mix()) display correctly in the
// swatch — the browser parses them — but pop the picker open at
// its last-good HSLA state rather than re-syncing.

import { type Handle } from "@remix-run/ui";
import { on } from "@sola/kit";
import { ColorPicker } from "@sola/color-picker";
import { Popover } from "@sola/popover";
import { Swatch } from "@sola/swatch";

export interface ColorInputProps {
  /** Current CSS color expression. */
  value?: string;

  /** Fires when the user adjusts the picker. */
  onChange?: (value: string) => void;
}

export function ColorInput(handle: Handle<ColorInputProps>) {
  // `copied` flips true for ~900ms after a successful copy so the
  // icon swaps to a check mark — non-blocking visual confirmation
  // that doesn't require a toast/snackbar component.
  let copied = false;
  let copiedTimer: number | null = null;

  const onCopy = async (e: Event) => {
    // Don't bubble to the popover-root listener — copying is a
    // separate action and shouldn't toggle the picker open.
    e.stopPropagation();
    const value = handle.props.value;
    if (!value) return;
    try {
      await navigator.clipboard.writeText(value);
      copied = true;
      handle.update();
      if (copiedTimer !== null) clearTimeout(copiedTimer);
      copiedTimer = setTimeout(() => {
        copied = false;
        copiedTimer = null;
        handle.update();
      }, 900) as unknown as number;
    } catch (err) {
      console.error("ColorInput copy failed", err);
    }
  };

  return () => {
    const { value, onChange } = handle.props;
    const swatchColor = value && value.trim() !== "" ? value : "transparent";
    const disableCopy = !value;

    return (
      <span class="sola-color-input">
        <Popover
          content={<ColorPicker value={value} onChange={onChange} />}
        >
          <span class="sola-color-input-trigger">
            <Swatch
              color={swatchColor}
              size="var(--sola-color-input-swatch-size)"
            />
          </span>
        </Popover>
        <button
          type="button"
          class="sola-color-input-copy"
          aria-label={copied ? "Copied" : "Copy color value"}
          title={copied ? "Copied" : "Copy color value"}
          disabled={disableCopy ? true : false}
          mix={[on("click", onCopy)]}
        >
          {copied ? checkIcon() : copyIcon()}
        </button>
      </span>
    );
  };
}

// Inline lucide icons (24×24 viewBox, strokes use currentColor).
// Inlined rather than fetched from /opt/sola/share/icons because the
// kit doesn't yet mount the icon directory under its app:// scheme,
// and the alternative — a single PNG/SVG <img> — can't be re-coloured
// per theme without filter hacks. Inline SVG inherits `color` directly.
function copyIcon() {
  return (
    <svg
      class="sola-color-input-copy-icon"
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
      class="sola-color-input-copy-icon"
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
