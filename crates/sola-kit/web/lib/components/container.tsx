// Container — centered max-width column with themed inner padding.
// The kit's "readable column" primitive — the standard wrapper a
// page's content sits inside.
//
//   <Container>{...}</Container>                  ← reading width
//   <Container maxWidth="wide">{...}</Container>  ← dashboard width
//   <Container maxWidth="640px">{...}</Container> ← per-call escape
//
// `maxWidth` accepts a semantic MaxWidthTag — narrow, reading
// (default), wide, full — or any raw CSS length like "640px" /
// "70ch" for one-offs. Tag widths are hard-coded in CSS (typography
// decision, not a brand value); themed padding comes from the
// `container` bindings entry on the active theme.

import { type Handle, type RemixNode } from "@remix-run/ui";

export type MaxWidthTag = "narrow" | "reading" | "wide" | "full";
export type MaxWidthValue = MaxWidthTag | (string & {});

const MAX_WIDTH_TAGS: ReadonlySet<string> = new Set([
  "narrow",
  "reading",
  "wide",
  "full",
]);

export interface ContainerProps {
  /** Semantic width tag or any raw CSS length. Defaults to
      `"reading"` — the typical comfortable reading column width. */
  maxWidth?: MaxWidthValue;
  children?: RemixNode;
}

export function Container(handle: Handle<ContainerProps>) {
  return () => {
    const { maxWidth = "reading", children } = handle.props;
    const isTag = MAX_WIDTH_TAGS.has(maxWidth);
    const cls = isTag
      ? `sola-container sola-container-${maxWidth}`
      : "sola-container";
    const style = isTag ? "" : `max-width: ${maxWidth}`;
    return (
      <div class={cls} style={style}>
        {children}
      </div>
    );
  };
}
