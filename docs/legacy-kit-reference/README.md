# Legacy sola-kit reference

These files are verbatim copies from the **`feat/sola-kit`** branch
(the lit-html / signals incarnation of sola-kit that preceded the
current CEF + Remix v3 rebuild). The branch remains in git; this
directory exists so the most-valuable design artifacts are
discoverable without checking out a parallel tree.

None of this compiles or runs in the current repo — it references
APIs (`lit-html`, `@sola/kit` from the lit kit, `pango`) that the
new kit deliberately doesn't have. Treat the files as design-spec
material to port from when the corresponding feature lands in the
new kit.

## What's here

### `role-defs.ts` (1,111 lines)

Per-component **semantic role taxonomy** for 12 components (button,
field, badge, empty, toast, tabs, form, list, row, section,
sidebar, icon). Each component declares labeled groups (Shape,
Type, Surface, per-variant Background/Text/Border, States) with
human-readable descriptions and `defaultToken` / `allowNone`
flags.

This is the spec for the **per-component bindings editor** we'll
need on the Tokens page once it grows past palette atoms. The new
kit's per-component `bindings()` functions
(`crates/sola-kit/src/components/<name>.rs`) carry the same
slot→token mapping, but lack the labels / descriptions / semantic
grouping that this file works out. When building the bindings
editor, structure each component's UI around the groups defined
here.

### `font-picker.ts` (98 lines)

Searchable popover of installed font families. Each option renders
in its own font for instant recognition. Pattern + UX to port when
the Tokens editor gains FontFamily editing — the new kit's
`Popover` component already covers the floating-panel mechanics;
this file shows the search/filter + per-option font preview on top.

### `fonts.rs` (72 lines)

Rust-side enumeration of installed font families via Pango/
fontconfig, split into mono vs proportional, with a curated
`GENERIC_FAMILIES` filter list (CSS generics + pango shorthand
aliases) that took trial-and-error to land on.

The Pango dependency does not fit the new (CEF-only, GTK-free)
kit. The strategy and the filter list port directly — just swap
the call to `pangocairo::FontMap::default().list_families()` for
the equivalent `fontconfig` or `font-kit` enumeration.

## Other ideas not extracted

The legacy branch also has worthwhile patterns we didn't extract
to avoid noise; revisit on the branch if needed:

- **300 ms debounced `theme_set`** in `token-edit.ts` — avoids RPC
  storm during slider drag. Worth porting if/when the picker
  feels laggy in practice.
- **Preview-frame pattern** in `preview/{component-previews,preview
  -frame,role-view,chips}.ts` — every component gets a
  `<sola-X-preview>` custom element combining variants + role
  editor in a standard frame. More systematic than the new kit's
  hand-rolled showcases.
- **`tokens-{colors,typography,spacing}.ts`** — per-kind editor
  pages with kind-specific affordances (e.g. "Used in N
  components" meta on each color). Mostly subsumed by the current
  Tokens page; consult only if specializing per-kind.

## Provenance

- Branch: `feat/sola-kit`
- Commit at the time of extraction: `9421bca` (sola-kit:
  component-first rewrite — every reusable view is a custom
  element)
