//! Design-token schema shared by sola-bus (the wire type for `Topic::Theme`)
//! and sola-kit (consumer + editor). Lives in sola-core because sola-bus
//! depends on sola-core, not the other way around.
//!
//! Two-layer protocol:
//!
//! 1. **Palette** (atoms) — a flat map of named tokens (`bg-secondary`,
//!    `accent`, `space-md` …). Each token carries a `kind` (color, font,
//!    size, …) and a list of `groups` it's eligible for ("surface",
//!    "border", "accent", …).
//! 2. **Components** (bindings) — for each component, a map of slot →
//!    `Binding { group, token }`. The slot's `group` constrains which
//!    palette tokens can be picked; `token` is the current selection.
//!
//! sola-core owns the *types*, the *palette seed* (brand-level atoms),
//! validation, and the CSS lowering. **Per-component bindings live with
//! the components that draw them** (in sola-kit, sola-shell, …) — see
//! `sola-kit::theme::kit_default_theme` for the kit's composed default.
//!
//! The renderer only ever sees CSS — `Theme::to_css` lowers the structured
//! theme into a single `:root { … }` block. Component CSS references the
//! scoped `--sola-<component>-<slot>` vars exclusively; atoms are an
//! implementation detail of the `:root` block.

mod css;
mod palette;
mod types;
mod validate;

pub use types::{
    Binding, ComponentBindings, NamedTheme, Palette, SlotName, Theme, Token, TokenKind, TokenName,
    is_valid_theme_name,
};
pub use validate::ValidationError;

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    #[test]
    fn default_validates_clean() {
        let theme = Theme::default();
        theme.validate().expect("default theme must validate");
    }

    #[test]
    fn default_has_seeded_palette_and_empty_components() {
        let theme = Theme::default();
        assert!(theme.components.is_empty());
        // A spot check that the palette seed populated.
        assert!(theme.palette.tokens.contains_key("bg-primary"));
        assert!(theme.palette.tokens.contains_key("accent"));
        assert!(theme.palette.tokens.contains_key("space-md"));
    }

    #[test]
    fn theme_round_trips_through_yaml() {
        let theme = Theme::default();
        let s = serde_yaml_ng::to_string(&theme).expect("serialize");
        let back: Theme = serde_yaml_ng::from_str(&s).expect("deserialize");
        assert_eq!(theme, back);
    }

    /// Helper: build a Theme with a single component bound to a single slot.
    /// Lets validation tests construct exactly the failure shape they want
    /// without depending on which component bindings any consumer ships.
    fn theme_with_one_binding(component: &str, slot: &str, binding: Binding) -> Theme {
        let mut comp = ComponentBindings::default();
        comp.slots.insert(slot.into(), binding);
        let mut components = BTreeMap::new();
        components.insert(component.into(), comp);
        Theme {
            palette: Palette::seed(),
            components,
        }
    }

    #[test]
    fn validate_rejects_dangling_token() {
        let theme = theme_with_one_binding("demo", "bg", Binding::new("surface", "no-such-token"));
        let err = theme.validate().expect_err("expected validation error");
        assert!(
            err.iter().any(|e| matches!(
                e,
                ValidationError::DanglingToken { token, .. } if token == "no-such-token"
            )),
            "expected DanglingToken error, got: {err:?}"
        );
    }

    #[test]
    fn validate_rejects_group_mismatch() {
        // bg-primary is a surface-only token (`groups: ["surface"]`).
        // Pointing a `border`-group slot at it triggers GroupNotInToken
        // (not GroupKindMismatch — bg-primary is still a Color).
        let theme = theme_with_one_binding("demo", "border", Binding::new("border", "bg-primary"));
        let err = theme.validate().expect_err("expected validation error");
        assert!(
            err.iter().any(|e| matches!(
                e,
                ValidationError::GroupNotInToken { token, group, .. }
                    if token == "bg-primary" && group == "border"
            )),
            "expected GroupNotInToken error, got: {err:?}"
        );
    }

    #[test]
    fn is_valid_theme_name_accepts_kebab() {
        assert!(is_valid_theme_name("alpha"));
        assert!(is_valid_theme_name("solar-flare"));
        assert!(is_valid_theme_name("a"));
        assert!(is_valid_theme_name("a-b-c-d-e"));
    }

    #[test]
    fn is_valid_theme_name_rejects_non_kebab() {
        assert!(!is_valid_theme_name(""), "empty");
        assert!(!is_valid_theme_name("Alpha"), "uppercase");
        assert!(!is_valid_theme_name("alpha2"), "digit");
        assert!(!is_valid_theme_name("-alpha"), "leading hyphen");
        assert!(!is_valid_theme_name("alpha-"), "trailing hyphen");
        assert!(!is_valid_theme_name("alpha--beta"), "double hyphen");
        assert!(!is_valid_theme_name("alpha bravo"), "space");
        assert!(!is_valid_theme_name("alpha/bravo"), "slash");
        assert!(!is_valid_theme_name("alpha_bravo"), "underscore");
    }

    #[test]
    fn validate_rejects_unknown_group() {
        let theme =
            theme_with_one_binding("demo", "bg", Binding::new("not-a-real-group", "bg-primary"));
        let err = theme.validate().expect_err("expected validation error");
        assert!(
            err.iter().any(|e| matches!(
                e,
                ValidationError::UnknownGroup { group, .. } if group == "not-a-real-group"
            )),
            "expected UnknownGroup error, got: {err:?}"
        );
    }
}
