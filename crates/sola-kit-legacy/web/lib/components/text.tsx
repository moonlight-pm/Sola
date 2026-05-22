// Text — the typography primitive. One element handles every
// readable string in a kit app: page headings, body copy, captions,
// and the small uppercase labels used as section dividers.
//
// `kind` chooses size + decoration; `tone` overlays a color. Each
// kind picks a sensible default HTML tag for semantics (display→h1,
// heading→h2, body/body-lg→p, caption/label→span); `as` overrides
// when needed.
//
// Color inheritance: the default tone uses `color: inherit`, so
// body text inherits the Root color and Text never re-references
// `--sola-root-text` — same rule the kit's other components follow
// for typography (font-family, font-size). The muted/subtle tones
// bind to their own scoped color slots.

import { type Handle, type RemixNode } from "@remix-run/ui";

export type TextKind =
  | "display"
  | "heading"
  | "body-lg"
  | "body"
  | "caption"
  | "label";

export type TextTone = "default" | "muted" | "subtle";

export interface TextProps {
  /** Size + decoration variant. Defaults to "body". */
  kind?: TextKind;
  /** Color overlay. Defaults to "default" (inherits Root text color). */
  tone?: TextTone;
  /**
   * Override the rendered HTML tag. Defaults are picked per `kind`:
   * display → h1, heading → h2, body → p, body-lg → p, caption →
   * span, label → span. Use to demote a visual heading to an `<h3>`
   * or render label text inside a `<legend>`, etc.
   */
  as?: string;
  children?: RemixNode;
}

const DEFAULT_TAG: Record<TextKind, string> = {
  display: "h1",
  heading: "h2",
  "body-lg": "p",
  body: "p",
  caption: "span",
  label: "span",
};

export function Text(handle: Handle<TextProps>) {
  return () => {
    const kind = handle.props.kind ?? "body";
    const tone = handle.props.tone ?? "default";
    const Tag = handle.props.as ?? DEFAULT_TAG[kind];

    const classes = `sola-text sola-text--${kind}` +
      (tone !== "default" ? ` sola-text--${tone}` : "");

    return <Tag class={classes}>{handle.props.children}</Tag>;
  };
}
