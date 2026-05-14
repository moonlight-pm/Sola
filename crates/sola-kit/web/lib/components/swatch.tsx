// Swatch — a colored square that previews a CSS color value, with
// optional click-to-edit semantics. The single primitive for "show
// a color" anywhere in the kit.
//
//   <Swatch color="#00d4ff" size="md" />                     ← display
//   <Swatch color={value} size="xxl" onChange={onChange} />  ← picker
//
// When `onChange` is set the swatch becomes the trigger of a
// Popover containing a ColorPicker — the same way a Field-shaped
// component would compose its parts, but folded into Swatch
// because the only meaningful interaction on a colored rectangle
// is "click to edit this color." No second component needed.
//
// `size` is a tagged enum mapped to `var(--space-${size})` rather
// than a free-form CSS length. The design system enforces that
// every Swatch in a Sola app picks from the shared space scale —
// callers can't smuggle a 32px rectangle in alongside semantic
// ones. If you genuinely need a different size, mint a new space
// token; the scale is the contract.
//
// The element renders a checkered transparency pattern beneath the
// color, so semi-transparent values (alpha < 1) reveal the pattern
// in proportion to their alpha — the standard convention for color
// pickers. Fully opaque colors hide the checker.

import { type Handle } from "@remix-run/ui";
import { ColorPicker } from "@sola/color-picker";
import { Popover } from "@sola/popover";

export type SwatchSize = "xs" | "sm" | "md" | "lg" | "xl" | "xxl";

export interface SwatchProps {
  /** CSS color expression to display. Required. Any value the
      browser accepts works (`#0d1117`, `rgba(...)`, `var(--accent)`,
      `currentColor`, etc.). */
  color: string;

  /** Square edge length, picked from the kit's space scale. Maps to
      `var(--space-${size})`. Defaults to `"xl"` (≈20px) — large
      enough to read as a clickable affordance, small enough not to
      dominate a form row. */
  size?: SwatchSize;

  /** When set, the swatch becomes a Popover trigger that opens a
      ColorPicker. Fires with the new value on every picker
      adjustment. Omitting `onChange` keeps the swatch purely
      display. */
  onChange?: (value: string) => void;
}

export function Swatch(handle: Handle<SwatchProps>) {
  return () => {
    const { color, size = "xl", onChange } = handle.props;
    const cls = `sola-swatch sola-swatch-${size}` +
      (onChange ? " is-editable" : "");
    const style = `--swatch-color: ${color}`;
    // The visible rectangle. When non-editable this is what the
    // caller gets; when editable the same element becomes the
    // Popover's trigger node.
    const rect = (
      <span class={cls} style={style} aria-hidden={onChange ? undefined : "true"} />
    );
    if (!onChange) return rect;
    return (
      <Popover content={<ColorPicker value={color} onChange={onChange} />}>
        {rect}
      </Popover>
    );
  };
}
