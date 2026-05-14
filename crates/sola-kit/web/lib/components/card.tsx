// Card — a panel with subtle surface chrome (background, border,
// rounded corners, padding) and an optional header with a label +
// description divided from the body by a thin rule.
//
// Used to group related rows in editor surfaces (Tokens page,
// BindingsEditor) without coupling those pages to a specific layout
// inside the card. The body is just `props.children` — consumers
// decide whether it's a grid, a stack, a list, etc.
//
// Cards opt-in to subgrid alignment when placed inside a parent
// grid: pages set the parent's `grid-template-columns`, then add a
// page-level rule overriding `.sola-card` to be `display: grid;
// grid-template-columns: subgrid` (see BindingsEditor's CSS for
// the pattern). Card itself stays display: block by default so
// standalone usage is uncoupled.

import { type Handle, type RemixNode } from "@remix-run/ui";
import { Text } from "@sola/text";

export interface CardProps {
  /** Optional heading rendered inside the card's header. Renders
      with `<Text kind="label">` — short, uppercase, muted weight. */
  label?: string;
  /** Optional description shown under the label. Caption-sized,
      muted tone. Skipped entirely (no extra space) when absent. */
  description?: string;
  /** Card body — typically a grid or stack. */
  children?: RemixNode;
}

export function Card(handle: Handle<CardProps>) {
  return () => {
    const { label, description, children } = handle.props;
    const hasHeader = !!label || !!description;
    return (
      <section class="sola-card">
        {hasHeader
          ? (
            <header class="sola-card-header">
              {label ? <Text kind="label">{label}</Text> : ""}
              {description
                ? (
                  <Text tone="muted" kind="caption">
                    {description}
                  </Text>
                )
                : ""}
            </header>
          )
          : ""}
        {children}
      </section>
    );
  };
}
