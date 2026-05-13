// Swatch — a colored square that previews a CSS color value. Used
// standalone in palettes and embedded as the leading element in
// ColorInput.
//
// The element renders a checkered transparency pattern beneath the
// color, so semi-transparent values (alpha < 1) reveal the pattern
// in proportion to their alpha — the standard convention for color
// pickers. Fully opaque colors hide the checker.
//
// `color` is forwarded to a CSS custom property that the stylesheet
// reads in an `::after` overlay. Any CSS color expression works:
// `#0d1117`, `rgba(0, 212, 255, 0.12)`, `currentColor`, etc.

import { type Handle } from "@remix-run/ui";

export interface SwatchProps {
  /** CSS color expression to display. Required. */
  color: string;
  /** Square edge length as a CSS length. Defaults to "20px". */
  size?: string;
}

export function Swatch(handle: Handle<SwatchProps>) {
  return () => {
    const { color, size = "20px" } = handle.props;
    const style =
      `--swatch-color: ${color}; width: ${size}; height: ${size};`;
    return <span class="sola-swatch" style={style} aria-hidden="true" />;
  };
}
