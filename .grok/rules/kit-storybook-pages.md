# Kit storybook pages when changing components

When you change a **sola-kit component** (styles, layout, density, API, or
behavior under `crates/sola-kit/src/components/`), **ask the user** whether to
also update the matching **storybook page** under
`crates/sola-kit/src/storybook/pages/`.

## Why

Graphite design-system work landed Overview + shared chrome first. Other tabs
still use older lab layouts; they inherit component styles but may not match
Open Design (`sola-kit-ds.html`) page composition. Page rewrites are optional
and should not block component work.

## Do

1. Ship / propose the component change first.
2. **Ask** something like: “Want the storybook **Button** (or Field / Card / …)
   page updated to match Open Design / the new look?”
3. Only rewrite the page if they say yes (or already asked for parity).

## Do not

- Silently rewrite every storybook page on every component tweak.
- Block a component fix waiting for full OD page parity.
- Assume Overview-style layout is required for every tab.

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
| shared `style.rs` materials | ask which demos matter; often Overview + affected control pages |

Open Design reference: project **Sola** → `sola-kit-ds.html`.
