// ColorInput — a Swatch trigger that opens a ColorPicker popover.
//
// The Swatch previews the current value as a CSS color; clicking it
// pops the picker open. The picker fires `onChange` whenever the
// user adjusts a slider or commits hex input; ColorInput forwards
// that straight to its own consumer.
//
// "Color expression" means any CSS color string the picker can
// parse: hex (`#0d1117`), hex+alpha (`#0d1117cc`), rgb / rgba
// (`rgba(0, 212, 255, 0.5)`). Strings that don't parse (named
// colors, var references, color-mix()) display correctly in the
// swatch — the browser parses them — but pop the picker open at
// its last-good HSLA state rather than re-syncing.

import { type Handle } from "@remix-run/ui";
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
  return () => {
    const { value, onChange } = handle.props;
    const swatchColor = value && value.trim() !== "" ? value : "transparent";

    return (
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
    );
  };
}
