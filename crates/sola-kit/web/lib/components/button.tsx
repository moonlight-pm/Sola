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

import { type Handle, type RemixNode } from "@remix-run/ui";
import { on } from "@sola/kit";

export type ButtonVariant = "default" | "primary" | "ghost" | "danger";

export interface ButtonProps {
  /**
   * Visual variant. Defaults to `"default"`. Each variant has its own
   * theme slot block (`--sola-button-<variant>-*`).
   */
  variant?: ButtonVariant;

  /**
   * Disabled buttons render at reduced opacity, stop firing `onPress`,
   * and are removed from the tab order via the native `disabled`
   * attribute.
   */
  disabled?: boolean;

  /**
   * Native button `type` — defaults to `"button"` so the button
   * doesn't accidentally submit a wrapping `<form>`. Override to
   * `"submit"` or `"reset"` for form integration.
   */
  type?: "button" | "submit" | "reset";

  /**
   * Fired on click (and, via native `<button>` semantics, on Enter or
   * Space when focused). No arguments — the consumer's closure has
   * everything it needs.
   */
  onPress?: () => void;

  /**
   * Optional leading slot — an icon or status dot rendered before the
   * label. Hidden if not provided.
   */
  leading?: RemixNode;

  /**
   * Optional trailing slot — chevron, kbd hint, or count rendered
   * after the label. Hidden if not provided.
   */
  trailing?: RemixNode;

  /**
   * The button label. Default-slot content; usually a string but any
   * RemixNode is accepted.
   */
  children?: RemixNode;
}

export function Button(handle: Handle<ButtonProps>) {
  // Event listeners attach via `mix={[on(...)]}` — Remix v3 doesn't
  // type lowercase event attrs on host elements, and the native
  // `disabled` attribute already short-circuits click before this
  // handler runs, so the disabled guard here is a belt-and-braces
  // check rather than a load-bearing one.
  const handleClick = () => {
    if (handle.props.disabled) return;
    handle.props.onPress?.();
  };

  return () => {
    const { variant, disabled, type, leading, trailing, children } =
      handle.props;
    const v: ButtonVariant = variant ?? "default";

    const classes = [
      "sola-button",
      `sola-button-${v}`,
      disabled ? "is-disabled" : "",
    ]
      .filter(Boolean)
      .join(" ");

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
        <span class="sola-button-label">{children}</span>
        {trailing
          ? <span class="sola-button-trailing">{trailing}</span>
          : null}
      </button>
    );
  };
}
