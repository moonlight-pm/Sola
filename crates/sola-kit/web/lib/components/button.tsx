// Button — kit-shipped Remix v3 component.
//
// Single factory `Button` rendering a real `<button>` element so the
// browser handles keyboard activation (Enter/Space → click), focus,
// and the disabled-removed-from-tab-order semantics for free.
//
// Variant + state styling lives entirely in `button.css`; this file
// only assembles class names. CSS references only `--sola-button-*`
// scoped vars, never atoms — the theme protocol owns the look.
//
// Slots are named props (Remix v3 idiom): `leading` / `trailing` for
// adornments around the label; the label itself is default-slot
// `children`.
//
// `confirm` mode — two-stage destructive action. First click swaps
// the visible variant to `danger` and the label to `confirmLabel`;
// a second click within 2 s commits and fires `onPress`. 2 s of
// inactivity rolls back to idle silently. The disarm timer is
// component-owned (`setTimeout`) and cleared on every interaction.

import { type Handle, type RemixNode } from "@remix-run/ui";
import { on } from "@sola/kit";

export type ButtonVariant = "default" | "primary" | "ghost" | "danger";

export interface ButtonProps {
  variant?: ButtonVariant;
  disabled?: boolean;
  type?: "button" | "submit" | "reset";
  onPress?: () => void;
  leading?: RemixNode;
  trailing?: RemixNode;
  children?: RemixNode;
  /**
   * Two-stage confirmation pattern. When `true`, the first click
   * arms the button (variant flips to danger, label swaps to
   * `confirmLabel`); the next click within 2 s fires `onPress`.
   * 2 s of inactivity disarms silently.
   */
  confirm?: boolean;
  /** Label shown while armed. Defaults to "Click again to confirm". */
  confirmLabel?: string;
}

const CONFIRM_TIMEOUT_MS = 2000;

export function Button(handle: Handle<ButtonProps>) {
  let armed = false;
  let timer: ReturnType<typeof setTimeout> | null = null;

  const disarm = () => {
    if (timer) {
      clearTimeout(timer);
      timer = null;
    }
    if (armed) {
      armed = false;
      handle.update();
    }
  };

  const handleClick = () => {
    if (handle.props.disabled) return;
    if (handle.props.confirm) {
      if (!armed) {
        armed = true;
        timer = setTimeout(() => {
          armed = false;
          timer = null;
          handle.update();
        }, CONFIRM_TIMEOUT_MS);
        handle.update();
        return;
      }
      // armed → commit
      disarm();
      handle.props.onPress?.();
      return;
    }
    handle.props.onPress?.();
  };

  return () => {
    const {
      variant,
      disabled,
      type,
      leading,
      trailing,
      children,
      confirm,
      confirmLabel,
    } = handle.props;
    const v: ButtonVariant = armed && confirm ? "danger" : variant ?? "default";

    const classes = [
      "sola-button",
      `sola-button-${v}`,
      disabled ? "is-disabled" : "",
      armed && confirm ? "is-armed" : "",
    ]
      .filter(Boolean)
      .join(" ");

    const labelContent = armed && confirm
      ? (confirmLabel ?? "Click again to confirm")
      : children;

    return (
      <button
        class={classes}
        type={type ?? "button"}
        disabled={disabled ? true : false}
        mix={[on("click", handleClick)]}
      >
        {leading
          ? <span class="sola-button-leading">{leading}</span>
          : null}
        <span class="sola-button-label">{labelContent}</span>
        {trailing
          ? <span class="sola-button-trailing">{trailing}</span>
          : null}
      </button>
    );
  };
}
