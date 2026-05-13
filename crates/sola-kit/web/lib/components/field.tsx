// Field — a labeled wrapper around a single form control. Renders
// a real `<label>` so clicking the label text focuses the contained
// input via native HTML semantics — no `htmlFor` plumbing needed.
//
// Layout (top-to-bottom, gap from theme):
//
//   [label?]
//   [control]   (default-slot children)
//   [error? | help?]
//
// `error` takes precedence over `help` when both are passed: at any
// given moment the user sees one or the other, not both. Both render
// at label-size; only their color differs.
//
// Field is the foundation for the upcoming TextInput, ColorInput,
// Select, etc. — those components don't ship their own labels.
// Apps wrap them with `<Field>`.

import { type Handle, type RemixNode } from "@remix-run/ui";

export interface FieldProps {
  /**
   * Label text rendered above the control. Optional — Field also
   * works as a "labelless container" if you want consistent gap and
   * help/error styling without a heading.
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
   * The form control itself — typically a TextInput, Select,
   * ColorInput, or any custom control. Field doesn't constrain the
   * type; it's purely structural.
   */
  children?: RemixNode;
}

export function Field(handle: Handle<FieldProps>) {
  return () => {
    const { label, help, error, children } = handle.props;

    return (
      <label class="sola-field">
        {label
          ? <span class="sola-field-label">{label}</span>
          : null}
        {children}
        {error
          ? <span class="sola-field-error">{error}</span>
          : help
          ? <span class="sola-field-help">{help}</span>
          : null}
      </label>
    );
  };
}
