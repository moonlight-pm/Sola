// Stack — a flex-box layout primitive. Sugar over the most common
// container layouts (vertical or horizontal stack with consistent
// gap and alignment).
//
// Stack is layout-only: no theme bindings, no colors, no typography.
// `gap` accepts a semantic SpaceTag (`"md"`) which expands to
// `var(--space-md)`, or any raw CSS length (`"12px"`, `"0.5rem"`)
// which passes through. See `resolveSpace` in `@sola/kit`.
//
// Renders `<div class="sola-stack" style="…">{children}</div>` with
// every flex property derived from props serialized into the inline
// style. The `sola-stack` class is purely for DevTools inspection
// and app-side override hooks; the component itself ships no CSS.
//
// Defaults:
//   direction "column"  — vertical stack is the 80% case
//   gap       "0"       — no implicit spacing; apps must opt in
//   align     "stretch" — children fill the cross axis (matches
//                         flex's own default)
//   justify   "start"   — children pack toward the main-axis start
//   wrap      false     — single-line by default (relevant for row)
//   inline    false     — block-level by default
//   fill      false     — Stack does not absorb parent flex space
//                         unless asked to

import { type Handle, type RemixNode } from "@remix-run/ui";
import { resolveSpace, type SpaceValue } from "@sola/kit";

export type StackDirection = "column" | "row";
export type StackAlign = "start" | "center" | "end" | "stretch";
export type StackJustify =
  | "start"
  | "center"
  | "end"
  | "between"
  | "around";

export interface StackProps {
  /** Main-axis direction. Defaults to "column" (vertical stack). */
  direction?: StackDirection;
  /**
   * Gap between children. Pass a SpaceTag (`"md"`) for theme-driven
   * spacing, or any raw CSS length (`"12px"`, `"0.5rem"`) for
   * one-offs. Defaults to `"0"`.
   */
  gap?: SpaceValue;
  /** Cross-axis alignment. Defaults to "stretch" (matches flex). */
  align?: StackAlign;
  /** Main-axis distribution. Defaults to "start". */
  justify?: StackJustify;
  /**
   * Wrap children onto multiple lines (mostly relevant for
   * `direction="row"`). Defaults to false.
   */
  wrap?: boolean;
  /** Use `inline-flex` instead of `flex`. Defaults to false. */
  inline?: boolean;
  /**
   * Make the Stack itself a flex item that absorbs available space
   * in its parent (`flex: 1 1 auto`). Useful when nesting Stacks
   * inside other flex containers.
   */
  fill?: boolean;
  children?: RemixNode;
}

const ALIGN: Record<StackAlign, string> = {
  start: "flex-start",
  center: "center",
  end: "flex-end",
  stretch: "stretch",
};

const JUSTIFY: Record<StackJustify, string> = {
  start: "flex-start",
  center: "center",
  end: "flex-end",
  between: "space-between",
  around: "space-around",
};

export function Stack(handle: Handle<StackProps>) {
  return () => {
    const {
      direction = "column",
      gap = "0",
      align = "stretch",
      justify = "start",
      wrap = false,
      inline = false,
      fill = false,
      children,
    } = handle.props;

    // `min-width/height: 0` lets nested flex children shrink below
    // their content size — flexbox's default `auto` minimum is the
    // cause of most "child overflows parent" bugs.
    const style = [
      `display: ${inline ? "inline-flex" : "flex"}`,
      `flex-direction: ${direction}`,
      `gap: ${resolveSpace(gap)}`,
      `align-items: ${ALIGN[align]}`,
      `justify-content: ${JUSTIFY[justify]}`,
      `flex-wrap: ${wrap ? "wrap" : "nowrap"}`,
      "min-width: 0",
      "min-height: 0",
      fill ? "flex: 1 1 auto" : "",
    ]
      .filter(Boolean)
      .join("; ");

    return (
      <div class="sola-stack" style={style}>
        {children}
      </div>
    );
  };
}
