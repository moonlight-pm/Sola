// Root — the kit's top-of-tree wrapper. Every kit app's `Main`
// component should return a `<Root>` containing its content. Root is
// what supplies the page-level look (background, foreground, font
// family, base text size) via the theme protocol's `--sola-root-*`
// scoped vars.
//
// Mechanically `<Root>` renders a single `<div class="sola-root">`
// that fills its parent (the document body, since the kit's
// `index.tsx` mounts `<Main />` into body). The CSS pins the
// dimensions to 100%×100% so the wrapper occupies the full viewport
// without each app re-asserting `100vh`/`100vw`.
//
// Root is intentionally layout-agnostic: it does not impose flex,
// grid, or any direction. Apps decide their own internal layout
// (the storybook uses a flex row containing a sidebar plus a content
// section). If a future app wants Root itself to be flex/grid, that
// becomes a prop or — preferably — a sibling layout component.

import { type Handle, type RemixNode } from "@remix-run/ui";

export interface RootProps {
  children?: RemixNode;
}

export function Root(handle: Handle<RootProps>) {
  return () => (
    <div class="sola-root">
      {handle.props.children}
    </div>
  );
}
