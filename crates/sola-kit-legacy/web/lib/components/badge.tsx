// Badge — small pill displaying status text alongside other
// content. `kind` chooses semantic color (background + foreground
// scoped vars); shape (radius, padding, text size) is shared.
//
// Used for inline status indicators (e.g. "not found" next to a
// configured application, "unread" count next to a mailbox). Not
// for free-form labels — use `<Text kind="label">` for those.

import { type Handle, type RemixNode } from "@remix-run/ui";

export type BadgeKind =
  | "neutral"
  | "info"
  | "success"
  | "warning"
  | "danger";

export interface BadgeProps {
  /** Semantic color tone. Defaults to "neutral". */
  kind?: BadgeKind;
  children?: RemixNode;
}

export function Badge(handle: Handle<BadgeProps>) {
  return () => {
    const k: BadgeKind = handle.props.kind ?? "neutral";
    return (
      <span class={`sola-badge sola-badge-${k}`}>
        {handle.props.children}
      </span>
    );
  };
}
