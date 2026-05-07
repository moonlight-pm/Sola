// ColorInput — a leading Swatch + a TextInput, composed. The swatch
// previews the current value as a CSS color; the text input edits
// the raw color expression. Both are controlled by the consumer.
//
// "Color expression" means any CSS color string: hex (`#0d1117`),
// rgba (`rgba(0, 212, 255, 0.5)`), named (`tomato`), var
// references (`var(--accent)`), modern functions
// (`color-mix(...)`, `oklch(...)`). The swatch renders whatever
// the browser parses; an unparseable value falls back to
// `transparent`, which reveals the checker pattern.
//
// Defers to TextInput for input semantics — onInput fires on every
// keystroke, onChange on commit (blur/Enter). The disabled and
// invalid states pass through to TextInput; the swatch always
// renders the value as-given regardless of those states.

import { type Handle } from "@remix-run/ui";
import { Swatch } from "@sola/swatch";
import { TextInput } from "@sola/text-input";

export interface ColorInputProps {
  /** Current CSS color expression. The component is controlled —
      re-rendering with a different value updates both the swatch
      and the text input. */
  value?: string;

  /** Placeholder shown when value is empty. */
  placeholder?: string;

  /** Disable typing and dim the input. The swatch still renders. */
  disabled?: boolean;

  /** Visual error state on the text input (does not affect the
      swatch). */
  invalid?: boolean;

  /** Fires on every keystroke. */
  onInput?: (value: string) => void;

  /** Fires on commit (blur or Enter). */
  onChange?: (value: string) => void;
}

export function ColorInput(handle: Handle<ColorInputProps>) {
  return () => {
    const { value, placeholder, disabled, invalid, onInput, onChange } =
      handle.props;
    // The swatch reflects whatever value is currently in the input.
    // An empty or unparseable color → "transparent", which reveals
    // the checker pattern through the swatch overlay.
    const swatchColor = value && value.trim() !== "" ? value : "transparent";

    return (
      <div class="sola-color-input">
        <Swatch
          color={swatchColor}
          size="var(--sola-color-input-swatch-size)"
        />
        <TextInput
          value={value}
          placeholder={placeholder ?? "#rrggbb or rgba(…) or var(--…)"}
          disabled={disabled}
          invalid={invalid}
          onInput={onInput}
          onChange={onChange}
        />
      </div>
    );
  };
}
