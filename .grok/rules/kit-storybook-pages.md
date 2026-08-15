# Kit storybook pages when changing components

When you change a **sola-kit component** (styles, layout, density, API, or
behavior under `crates/sola-kit/src/components/`), **always update** the
matching **storybook page** under `crates/sola-kit/src/storybook/pages/`
in the **same change**. Do not ask first. Do not leave storybook stale.

## Why

The storybook is the dogfood surface for the kit. A component change that
does not show up there is an incomplete kit change. Graphite Overview is
the composition reference; other tabs still inherit styles, but the page
for the component you touched must demonstrate the new look or behavior.

## Do

1. Ship the component change.
2. Update the matching storybook page in the same commit / slice:
   copy, demo rows, hover/close/empty states — whatever the change
   actually affects.
3. If the page is a full rewrite, follow Overview composition. If it is
   a local tweak (e.g. close-chip fill), update the existing demo and
   the one-line description so the new behavior is visible and named.

## Do not

- Skip the page because the widget “already inherits” the style.
- Ask the user whether to update storybook (the answer is yes).
- Block a tiny style fix on a full page rewrite — update the demo that
  exists; rewrite only when the page can no longer show the change.

## Mapping (component → page)

| Component area | Storybook page |
|---|---|
| `button` | `pages/button.rs` |
| `badge` | `pages/badge.rs` |
| `card` | `pages/card.rs` |
| `field` / form helpers | `pages/field.rs`, `pages/form.rs` |
| `sidebar` | `pages/sidebar.rs` |
| `text_input` | `pages/field.rs` (and form demos) |
| theme / shell tokens | `pages/theme.rs`, `pages/shell.rs`, Overview |
| shared `style.rs` materials | Overview + every control page that uses the token |

Storybook Overview is the in-repo composition reference when rewriting a page.
