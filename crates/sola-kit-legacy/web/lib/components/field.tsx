// Field — a labeled wrapper around a single form control. Renders
// a real `<label>` so clicking the label text focuses the contained
// input via native HTML semantics — no `htmlFor` plumbing needed.
//
// Layout — two directions, picked via the `direction` prop:
//
//   column (default):              row:
//     [label?]                       [label?] [control]
//     [control]                      [error? | help?]
//     [error? | help?]
//
// In row mode the label sits to the left of the control, fixed-width
// so a grid of rows lines up. Help/error remains a single line below
// the row band; pass `help=""` to suppress when help would otherwise
// add unwanted vertical space. Tooltip-style help can ride on the
// container instead via the `title` prop.
//
// `error` takes precedence over `help` when both are passed: at any
// given moment the user sees one or the other, not both. Both render
// at label-size; only their color differs.
//
// Field is the foundation for the upcoming TextInput, ColorInput,
// Select, etc. — those components don't ship their own labels.
// Apps wrap them with `<Field>`.

import { type Handle, type RemixNode } from "@remix-run/ui";

export type FieldDirection = "column" | "row";

export interface FieldProps {
  /**
   * Label text rendered above (or beside, in row mode) the control.
   * Optional — Field also works as a "labelless container" if you
   * want consistent gap and help/error styling without a heading.
   */
  label?: string;

  /**
   * Help text rendered below the control. Hidden when `error` is
   * also set (errors take precedence).
   */
  help?: string;

  /**
   * Error message rendered below the control in `--sola-field-
   * error-color`. Shadows `help` when both are set.
   */
  error?: string;

  /**
   * Layout direction. `"column"` (default) stacks label / control /
   * help vertically. `"row"` puts the label left and the control
   * right, with help/error remaining below the row band.
   */
  direction?: FieldDirection;

  /**
   * Forwarded to the `<label>` element's native `title` attribute,
   * which becomes a browser tooltip on hover. Useful in dense
   * row-direction layouts where help text would crowd the row.
   */
  title?: string;

  /**
   * The form control itself — typically a TextInput, Select,
   * ColorInput, or any custom control. Field doesn't constrain the
   * type; it's purely structural.
   */
  children?: RemixNode;
}

export function Field(handle: Handle<FieldProps>) {
  return () => {
    const { label, help, error, direction = "column", title, children } =
      handle.props;

    const classes = `sola-field${direction === "row" ? " sola-field-row" : ""}`;

    return (
      <label class={classes} title={title}>
        {direction === "row"
          ? (
            <span class="sola-field-row-band">
              {label
                ? <span class="sola-field-label">{label}</span>
                : null}
              <span class="sola-field-row-control">{children}</span>
            </span>
          )
          : (
            <>
              {label
                ? <span class="sola-field-label">{label}</span>
                : null}
              {children}
            </>
          )}
        {error
          ? <span class="sola-field-error">{error}</span>
          : help
          ? <span class="sola-field-help">{help}</span>
          : null}
      </label>
    );
  };
}
