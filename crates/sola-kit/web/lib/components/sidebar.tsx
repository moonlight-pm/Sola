// Sidebar — kit-shipped Remix v3 components.
//
// Three component factories:
//   - Sidebar          — container, optional `width` (default 220px)
//   - SidebarSection   — optional `label`; renders a header + the items
//   - SidebarItem      — `active`, `disabled`, optional `leading`/`trailing`
//                        named slots, and an `onSelect` callback fired on
//                        click / Enter / Space
//
// Selection is parent-controlled: the consumer passes `active` and an
// `onSelect` to whichever item should be current. We do NOT track the
// current selection inside the sidebar tree.
//
// Styles live in `sidebar.css`, registered via a `<link>` in the host
// `index.html`. The CSS references only `--sola-sidebar-*` (scoped vars
// emitted by the new theme protocol); typography (color, font-family,
// font-size) cascades from the surrounding `<Root>` via natural CSS
// inheritance. No atom is referenced directly.

import { type Handle, type RemixNode } from "@remix-run/ui";
import { on } from "@sola/kit";

// ── Sidebar ─────────────────────────────────────────────────────────

export interface SidebarProps {
  children?: RemixNode;
}

export function Sidebar(handle: Handle<SidebarProps>) {
  // Sidebar fills its layout slot — width comes from the parent
  // (typically a Split's first pane), scroll comes from the parent's
  // overflow:auto wrapper, and the visual separator from Sidebar to
  // the next pane comes from Split's themed divider. Sidebar owns
  // only its own background, padding, gap, and typography.
  return () => (
    <nav class="sola-sidebar" role="navigation">
      {handle.props.children}
    </nav>
  );
}

// ── SidebarSection ──────────────────────────────────────────────────

export interface SidebarSectionProps {
  /**
   * Optional uppercase label rendered above the section's items.
   * If absent the section renders without a header (purely a
   * grouping wrapper).
   */
  label?: string;
  children?: RemixNode;
}

export function SidebarSection(handle: Handle<SidebarSectionProps>) {
  return () => {
    const { label, children } = handle.props;
    return (
      <div class="sola-sidebar-section" role="group">
        {label
          ? <header class="sola-sidebar-section-label">{label}</header>
          : null}
        {children}
      </div>
    );
  };
}

// ── SidebarItem ─────────────────────────────────────────────────────

export interface SidebarItemProps {
  /**
   * Whether this item is the currently selected one. Visual: bg tint
   * + 2-px accent stripe at the leading edge. Parent-controlled.
   */
  active?: boolean;

  /**
   * Disabled items render at 0.4 opacity, ignore clicks/keys, and are
   * removed from the tab order.
   */
  disabled?: boolean;

  /**
   * Fired on click, Enter, or Space. Arguments are intentionally
   * empty — identifying the item is the consumer's job (closure
   * capture, or a `key`/`id` from the iteration).
   */
  onSelect?: () => void;

  /**
   * Optional leading slot — a `<icon>`, image, or any element
   * rendered before the label. Hidden by default if not passed.
   */
  leading?: RemixNode;

  /**
   * Optional trailing slot — a badge, count, status dot, or other
   * adornment rendered after the label. Hidden by default if not
   * passed.
   */
  trailing?: RemixNode;

  /**
   * The item label. Default-slot content. Usually a string but any
   * RemixNode is accepted.
   */
  children?: RemixNode;
}

export function SidebarItem(handle: Handle<SidebarItemProps>) {
  // Event listeners attach via the `mix` prop's `on()` helpers. Remix v3
  // doesn't type lowercase event attributes (`onclick=`, `onkeydown=`)
  // on host elements, and the camelCase React-style attrs aren't part
  // of its DOM typings either — `mix={[on("click", …)]}` is the
  // canonical attachment mechanism.
  const fire = () => {
    if (handle.props.disabled) return;
    handle.props.onSelect?.();
  };

  const handleClick = () => fire();

  const handleKeyDown = (e: KeyboardEvent) => {
    if (e.key === "Enter" || e.key === " ") {
      e.preventDefault();
      fire();
    }
  };

  return () => {
    const { active, disabled, leading, trailing, children } = handle.props;
    // Class string assembled at render time so the active/disabled
    // selectors in sidebar.css can target plain class names.
    const classes = [
      "sola-sidebar-item",
      active ? "is-active" : "",
      disabled ? "is-disabled" : "",
    ]
      .filter(Boolean)
      .join(" ");

    return (
      <div
        class={classes}
        role="button"
        tabindex={disabled ? -1 : 0}
        aria-current={active ? "true" : "false"}
        aria-disabled={disabled ? "true" : "false"}
        mix={[on("click", handleClick), on("keydown", handleKeyDown)]}
      >
        {leading
          ? <span class="sola-sidebar-item-leading">{leading}</span>
          : null}
        <span class="sola-sidebar-item-label">{children}</span>
        {trailing
          ? <span class="sola-sidebar-item-trailing">{trailing}</span>
          : null}
      </div>
    );
  };
}
