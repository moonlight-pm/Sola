// TextInput — a styled single-line `<input>`. Controlled by the
// consumer: pass `value` and an `onInput` (live keystrokes) or
// `onChange` (commit on blur / Enter) callback. The component does
// not maintain its own state.
//
// Underlying element is a real `<input>` so all native behavior
// (selection, IME composition, copy/paste, undo) works
// transparently. Styles ride on the `.sola-text-input` class; the
// `is-invalid` modifier swaps the border color for the error tone.
//
// Used inside `<Field>` for labeled forms; `<Field>`'s `<label>`
// wrapping makes clicking the label text focus this input via
// native HTML semantics. When a `trailing` slot is set the input
// is rendered inside a `.sola-text-input-wrapper` span, with the
// trailing content absolutely positioned at the input's right
// edge and the input's right padding extended so typed text
// doesn't collide with it.

import { type Handle, type RemixNode } from "@remix-run/ui";
import { on } from "@sola/kit";

export type TextInputType = "text" | "search" | "email" | "url" | "password";

export interface TextInputProps {
  /** Current value. The input is controlled — re-rendering with a
      different `value` updates the DOM. */
  value?: string;

  /** Placeholder shown when `value` is empty. */
  placeholder?: string;

  /** Native input type. Defaults to "text". Pick "password" to mask
      input, "email"/"url" for soft validation hints, "search" for
      OS-rendered clear button on some platforms. */
  type?: TextInputType;

  /** Disabled inputs render dimmed, ignore typing, and skip the tab
      order. */
  disabled?: boolean;

  /** Visual error state. Swaps the border to the error color; pair
      with a Field-level `error` prop for the message. */
  invalid?: boolean;

  /**
   * Optional content rendered inside the input box at the right
   * edge — copy buttons, clear icons, password reveal toggles. The
   * input's right padding is extended so typed text doesn't sit
   * underneath whatever you pass here.
   */
  trailing?: RemixNode;

  /**
   * Fires on every keystroke / IME composition event. The string
   * argument is the input's current value.
   */
  onInput?: (value: string) => void;

  /**
   * Fires on commit — blur or Enter for text inputs. The string
   * argument is the value at commit time.
   */
  onChange?: (value: string) => void;
}

export function TextInput(handle: Handle<TextInputProps>) {
  const handleInput = (e: Event) => {
    if (handle.props.disabled) return;
    const target = e.target as HTMLInputElement;
    handle.props.onInput?.(target.value);
  };

  const handleChange = (e: Event) => {
    if (handle.props.disabled) return;
    const target = e.target as HTMLInputElement;
    handle.props.onChange?.(target.value);
  };

  return () => {
    const { value, placeholder, type, disabled, invalid, trailing } =
      handle.props;
    const classes = [
      "sola-text-input",
      invalid ? "is-invalid" : "",
      trailing ? "has-trailing" : "",
    ]
      .filter(Boolean)
      .join(" ");

    const input = (
      <input
        type={type ?? "text"}
        class={classes}
        value={value ?? ""}
        placeholder={placeholder}
        disabled={disabled ? true : false}
        mix={[on("input", handleInput), on("change", handleChange)]}
      />
    );

    if (!trailing) return input;

    return (
      <span class="sola-text-input-wrapper">
        {input}
        <span class="sola-text-input-trailing">{trailing}</span>
      </span>
    );
  };
}
