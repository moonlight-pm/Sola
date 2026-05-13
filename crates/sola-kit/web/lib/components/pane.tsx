// Pane — the scrollable padded content area used inside every kit
// app page. Acts as a flex item (`flex: 1 1 auto`) so it absorbs
// the remaining space in its parent flex container, applies themed
// padding from the `pane` bindings, and scrolls internally when its
// content overflows.
//
// Defaults to a `<section>` for semantics; `as` overrides when
// nesting (e.g. when a parent already provides the section).

import { type Handle, type RemixNode } from "@remix-run/ui";

export interface PaneProps {
  /**
   * HTML tag to render. Defaults to `"section"` — a content pane is
   * typically a semantic landmark. Override with `"div"` when
   * nesting (an outer `<section>` already covers semantics).
   */
  as?: string;
  children?: RemixNode;
}

export function Pane(handle: Handle<PaneProps>) {
  return () => {
    const Tag = handle.props.as ?? "section";
    return <Tag class="sola-pane">{handle.props.children}</Tag>;
  };
}
