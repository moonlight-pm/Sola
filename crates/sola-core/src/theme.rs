//! Design-token schema shared by sola-bus (the wire type for `Topic::Theme`)
//! and sola-kit (consumer + editor). Lives in sola-core because sola-bus
//! depends on sola-core, not the other way around — same arrangement as
//! `crate::applications::Application`.
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
//! The renderer only ever sees CSS — `Theme::to_css` lowers the structured
//! theme into a single `:root { … }` block. Component CSS references the
//! scoped `--sola-<component>-<slot>` vars exclusively; atoms are an
//! implementation detail of the `:root` block.
//!
//! Editing is a kit-only concern: a future theme editor mutates `Theme`,
//! validates it, then publishes via `Topic::Theme`.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// A token's name (key into `Palette.tokens`). Stringly-typed so adding
/// a new token is one map insertion and zero type churn.
pub type TokenName = String;

/// A component slot's name (key into `ComponentBindings.slots`).
pub type SlotName = String;

// Note on Default: we don't `#[derive(Default)]` for `Theme` because the
// derived (empty palette + empty components) value would fail
// `validate()` and break every existing call site. The manual impl
// below delegates to `Theme::seed()` so `Theme::default()` returns a
// usable theme.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Theme {
    pub palette: Palette,
    pub components: BTreeMap<String, ComponentBindings>,
}

impl Default for Theme {
    fn default() -> Self {
        Self::seed()
    }
}

// ── Layer 1 — flat palette of named tokens ──────────────────────────

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct Palette {
    pub tokens: BTreeMap<TokenName, Token>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Token {
    pub kind: TokenKind,
    pub value: String,
    pub groups: Vec<String>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum TokenKind {
    Color,
    FontFamily,
    TextSize,
    Space,
    Radius,
}

// ── Layer 2 — per-component bindings ────────────────────────────────

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct ComponentBindings {
    pub slots: BTreeMap<SlotName, Binding>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Binding {
    pub group: String,
    pub token: TokenName,
}

// ── Validation ──────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ValidationError {
    /// A binding references a token that doesn't exist in the palette.
    DanglingToken {
        component: String,
        slot: SlotName,
        token: TokenName,
    },
    /// A binding's group isn't listed in the referenced token's `groups`.
    GroupNotInToken {
        component: String,
        slot: SlotName,
        token: TokenName,
        group: String,
    },
    /// A binding's group expects a different `TokenKind` than the
    /// token actually is (e.g. a `border` slot pointing at a `Space` token).
    GroupKindMismatch {
        component: String,
        slot: SlotName,
        token: TokenName,
        group: String,
        expected: TokenKind,
        actual: TokenKind,
    },
    /// A binding's group isn't in the static group → kind table
    /// (i.e. it's not one of the v1 vocabulary entries).
    UnknownGroup {
        component: String,
        slot: SlotName,
        group: String,
    },
}

/// Static map of selection-group → expected `TokenKind`. Each group
/// corresponds to exactly one kind: groups are colors-only, sizing-only,
/// font-only, etc., never mixed. A token may declare multiple groups
/// (e.g. an accent color usable as a border), but each group's kind is
/// fixed.
fn group_kind(group: &str) -> Option<TokenKind> {
    match group {
        "surface" | "border" | "text" | "accent" | "accent-tint" | "status" => {
            Some(TokenKind::Color)
        }
        "font-family" => Some(TokenKind::FontFamily),
        "text-size" => Some(TokenKind::TextSize),
        "space" => Some(TokenKind::Space),
        "radius" => Some(TokenKind::Radius),
        _ => None,
    }
}

impl Theme {
    /// Validate every binding against the palette. Collects all errors;
    /// the editor surfaces all of them at once instead of stopping at the
    /// first.
    pub fn validate(&self) -> Result<(), Vec<ValidationError>> {
        let mut errors = Vec::new();
        for (component, bindings) in &self.components {
            for (slot, binding) in &bindings.slots {
                // Group must be in the static vocabulary.
                let Some(expected_kind) = group_kind(&binding.group) else {
                    errors.push(ValidationError::UnknownGroup {
                        component: component.clone(),
                        slot: slot.clone(),
                        group: binding.group.clone(),
                    });
                    // Without a known group, the group/kind/groups checks
                    // below have nothing to compare against.
                    continue;
                };
                // Token must exist in the palette.
                let Some(token) = self.palette.tokens.get(&binding.token) else {
                    errors.push(ValidationError::DanglingToken {
                        component: component.clone(),
                        slot: slot.clone(),
                        token: binding.token.clone(),
                    });
                    continue;
                };
                // Token's kind must match the group's expected kind.
                if token.kind != expected_kind {
                    errors.push(ValidationError::GroupKindMismatch {
                        component: component.clone(),
                        slot: slot.clone(),
                        token: binding.token.clone(),
                        group: binding.group.clone(),
                        expected: expected_kind,
                        actual: token.kind,
                    });
                }
                // Token must self-declare it's eligible for this group.
                if !token.groups.iter().any(|g| g == &binding.group) {
                    errors.push(ValidationError::GroupNotInToken {
                        component: component.clone(),
                        slot: slot.clone(),
                        token: binding.token.clone(),
                        group: binding.group.clone(),
                    });
                }
            }
        }
        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors)
        }
    }

    /// Render this theme into the full `:root { … }` CSS block, atoms
    /// first (one var per palette token, name = key), then per-component
    /// scoped vars (`--sola-<component>-<slot>: var(--<token>);`).
    ///
    /// Output is deterministic — every map iterated here is `BTreeMap`,
    /// so iteration is alphabetical. The golden snapshot test in this
    /// module locks the exact byte sequence.
    pub fn to_css(&self) -> String {
        let mut out = String::new();
        out.push_str(":root {\n");
        // Layer 1 — atoms
        out.push_str("  /* atoms */\n");
        for (name, token) in &self.palette.tokens {
            out.push_str("  --");
            out.push_str(name);
            out.push_str(": ");
            out.push_str(&token.value);
            out.push_str(";\n");
        }
        // Layer 2 — bindings (one block per component)
        for (component, bindings) in &self.components {
            out.push_str("\n  /* ");
            out.push_str(component);
            out.push_str(" */\n");
            for (slot, binding) in &bindings.slots {
                out.push_str("  --sola-");
                out.push_str(component);
                out.push_str("-");
                out.push_str(slot);
                out.push_str(": var(--");
                out.push_str(&binding.token);
                out.push_str(");\n");
            }
        }
        out.push_str("}\n");
        out
    }
}

// ── Default seed ────────────────────────────────────────────────────

/// Build a `Token` with the given kind, value, and groups. Helper used
/// by `Theme::default` to keep the seed compact.
fn tok(kind: TokenKind, value: &str, groups: &[&str]) -> Token {
    Token {
        kind,
        value: value.to_string(),
        groups: groups.iter().map(|s| s.to_string()).collect(),
    }
}

/// Build a `Binding` with the given group and token. Helper used by
/// `Theme::default` to keep the seed compact.
fn bind(group: &str, token: &str) -> Binding {
    Binding {
        group: group.to_string(),
        token: token.to_string(),
    }
}

impl Theme {
    /// Build the seed theme. Mirrors `Default::default` but kept as a
    /// named constructor for clarity at call sites; `Default` delegates.
    pub fn seed() -> Self {
        let mut palette = Palette::default();
        // Colors
        palette.tokens.insert(
            "bg-primary".into(),
            tok(TokenKind::Color, "#0d1117", &["surface"]),
        );
        palette.tokens.insert(
            "bg-secondary".into(),
            tok(TokenKind::Color, "#161b22", &["surface"]),
        );
        palette.tokens.insert(
            "bg-tertiary".into(),
            tok(TokenKind::Color, "#1c2129", &["surface"]),
        );
        palette.tokens.insert(
            "bg-hover".into(),
            tok(TokenKind::Color, "#1a2030", &["surface"]),
        );
        palette.tokens.insert(
            "border".into(),
            tok(TokenKind::Color, "#2d333b", &["border"]),
        );
        palette.tokens.insert(
            "border-subtle".into(),
            tok(TokenKind::Color, "#21262d", &["border"]),
        );
        palette.tokens.insert(
            "text-primary".into(),
            tok(TokenKind::Color, "#e6edf3", &["text"]),
        );
        palette.tokens.insert(
            "text-secondary".into(),
            tok(TokenKind::Color, "#8b949e", &["text"]),
        );
        palette.tokens.insert(
            "text-tertiary".into(),
            tok(TokenKind::Color, "#6e7681", &["text"]),
        );
        palette.tokens.insert(
            "text-muted".into(),
            tok(TokenKind::Color, "#484f58", &["text"]),
        );
        palette.tokens.insert(
            "text-accent".into(),
            tok(TokenKind::Color, "#58a6ff", &["text", "accent"]),
        );
        palette.tokens.insert(
            "accent".into(),
            tok(TokenKind::Color, "#00d4ff", &["accent"]),
        );
        palette.tokens.insert(
            "accent-dim".into(),
            tok(
                TokenKind::Color,
                "rgba(0, 212, 255, 0.12)",
                &["accent-tint"],
            ),
        );
        palette.tokens.insert(
            "danger".into(),
            tok(TokenKind::Color, "#f85149", &["status"]),
        );
        palette.tokens.insert(
            "success".into(),
            tok(TokenKind::Color, "#3fb950", &["status"]),
        );
        // Fonts
        palette.tokens.insert(
            "font-sans".into(),
            tok(
                TokenKind::FontFamily,
                "'DM Sans', system-ui, sans-serif",
                &["font-family"],
            ),
        );
        palette.tokens.insert(
            "font-mono".into(),
            tok(
                TokenKind::FontFamily,
                "'JetBrains Mono', 'Fira Code', 'Source Code Pro', monospace",
                &["font-family"],
            ),
        );
        // Text sizes
        palette.tokens.insert(
            "text-caption".into(),
            tok(TokenKind::TextSize, "11px", &["text-size"]),
        );
        palette.tokens.insert(
            "text-body".into(),
            tok(TokenKind::TextSize, "12px", &["text-size"]),
        );
        palette.tokens.insert(
            "text-body-lg".into(),
            tok(TokenKind::TextSize, "13px", &["text-size"]),
        );
        palette.tokens.insert(
            "text-heading".into(),
            tok(TokenKind::TextSize, "16px", &["text-size"]),
        );
        palette.tokens.insert(
            "text-display".into(),
            tok(TokenKind::TextSize, "20px", &["text-size"]),
        );
        // Spacing
        palette.tokens.insert(
            "space-xs".into(),
            tok(TokenKind::Space, "4px", &["space"]),
        );
        palette.tokens.insert(
            "space-sm".into(),
            tok(TokenKind::Space, "8px", &["space"]),
        );
        palette.tokens.insert(
            "space-md".into(),
            tok(TokenKind::Space, "12px", &["space"]),
        );
        palette.tokens.insert(
            "space-lg".into(),
            tok(TokenKind::Space, "16px", &["space"]),
        );
        palette.tokens.insert(
            "space-xl".into(),
            tok(TokenKind::Space, "20px", &["space"]),
        );
        palette.tokens.insert(
            "space-xxl".into(),
            tok(TokenKind::Space, "24px", &["space"]),
        );
        // Radius
        palette.tokens.insert(
            "radius-sm".into(),
            tok(TokenKind::Radius, "3px", &["radius"]),
        );
        palette.tokens.insert(
            "radius-md".into(),
            tok(TokenKind::Radius, "4px", &["radius"]),
        );
        palette.tokens.insert(
            "radius-lg".into(),
            tok(TokenKind::Radius, "6px", &["radius"]),
        );

        // Components — page (globals applied at body { … })
        let mut page = ComponentBindings::default();
        page.slots.insert("bg".into(), bind("surface", "bg-primary"));
        page.slots
            .insert("text".into(), bind("text", "text-primary"));
        page.slots
            .insert("font".into(), bind("font-family", "font-sans"));
        page.slots
            .insert("text-size".into(), bind("text-size", "text-body"));

        // Components — sidebar
        let mut sidebar = ComponentBindings::default();
        sidebar
            .slots
            .insert("bg".into(), bind("surface", "bg-secondary"));
        sidebar
            .slots
            .insert("border".into(), bind("border", "border-subtle"));
        sidebar
            .slots
            .insert("section-label-color".into(), bind("text", "text-secondary"));
        sidebar
            .slots
            .insert("section-label-size".into(), bind("text-size", "text-caption"));
        sidebar
            .slots
            .insert("item-text-idle".into(), bind("text", "text-secondary"));
        sidebar
            .slots
            .insert("item-text-active".into(), bind("text", "text-primary"));
        sidebar
            .slots
            .insert("item-text-size".into(), bind("text-size", "text-body"));
        sidebar
            .slots
            .insert("item-icon-idle".into(), bind("text", "text-secondary"));
        sidebar
            .slots
            .insert("item-icon-active".into(), bind("accent", "accent"));
        sidebar
            .slots
            .insert("item-bg-hover".into(), bind("surface", "bg-hover"));
        sidebar
            .slots
            .insert("item-bg-active".into(), bind("accent-tint", "accent-dim"));
        sidebar
            .slots
            .insert("item-stripe".into(), bind("accent", "accent"));
        sidebar
            .slots
            .insert("padding-block".into(), bind("space", "space-md"));
        sidebar
            .slots
            .insert("padding-inline".into(), bind("space", "space-sm"));
        sidebar
            .slots
            .insert("item-padding-block".into(), bind("space", "space-sm"));
        sidebar
            .slots
            .insert("item-padding-inline".into(), bind("space", "space-md"));
        sidebar.slots.insert("gap".into(), bind("space", "space-xs"));

        let mut components = BTreeMap::new();
        components.insert("page".into(), page);
        components.insert("sidebar".into(), sidebar);

        Self {
            palette,
            components,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_validates_clean() {
        let theme = Theme::default();
        theme.validate().expect("default theme must validate");
    }

    /// Golden-snapshot test for the rendered `:root { … }` block. Locked
    /// against the seed; iteration is alphabetical via `BTreeMap`, so any
    /// edit to atoms or bindings shows up here as a diff. Update the
    /// expected string deliberately.
    #[test]
    fn default_to_css_is_stable() {
        let css = Theme::default().to_css();
        let expected = "\
:root {
  /* atoms */
  --accent: #00d4ff;
  --accent-dim: rgba(0, 212, 255, 0.12);
  --bg-hover: #1a2030;
  --bg-primary: #0d1117;
  --bg-secondary: #161b22;
  --bg-tertiary: #1c2129;
  --border: #2d333b;
  --border-subtle: #21262d;
  --danger: #f85149;
  --font-mono: 'JetBrains Mono', 'Fira Code', 'Source Code Pro', monospace;
  --font-sans: 'DM Sans', system-ui, sans-serif;
  --radius-lg: 6px;
  --radius-md: 4px;
  --radius-sm: 3px;
  --space-lg: 16px;
  --space-md: 12px;
  --space-sm: 8px;
  --space-xl: 20px;
  --space-xs: 4px;
  --space-xxl: 24px;
  --success: #3fb950;
  --text-accent: #58a6ff;
  --text-body: 12px;
  --text-body-lg: 13px;
  --text-caption: 11px;
  --text-display: 20px;
  --text-heading: 16px;
  --text-muted: #484f58;
  --text-primary: #e6edf3;
  --text-secondary: #8b949e;
  --text-tertiary: #6e7681;

  /* page */
  --sola-page-bg: var(--bg-primary);
  --sola-page-font: var(--font-sans);
  --sola-page-text: var(--text-primary);
  --sola-page-text-size: var(--text-body);

  /* sidebar */
  --sola-sidebar-bg: var(--bg-secondary);
  --sola-sidebar-border: var(--border-subtle);
  --sola-sidebar-gap: var(--space-xs);
  --sola-sidebar-item-bg-active: var(--accent-dim);
  --sola-sidebar-item-bg-hover: var(--bg-hover);
  --sola-sidebar-item-icon-active: var(--accent);
  --sola-sidebar-item-icon-idle: var(--text-secondary);
  --sola-sidebar-item-padding-block: var(--space-sm);
  --sola-sidebar-item-padding-inline: var(--space-md);
  --sola-sidebar-item-stripe: var(--accent);
  --sola-sidebar-item-text-active: var(--text-primary);
  --sola-sidebar-item-text-idle: var(--text-secondary);
  --sola-sidebar-item-text-size: var(--text-body);
  --sola-sidebar-padding-block: var(--space-md);
  --sola-sidebar-padding-inline: var(--space-sm);
  --sola-sidebar-section-label-color: var(--text-secondary);
  --sola-sidebar-section-label-size: var(--text-caption);
}
";
        assert_eq!(css, expected);
    }

    #[test]
    fn theme_round_trips_through_toml() {
        let theme = Theme::default();
        let s = toml::to_string(&theme).expect("serialize");
        let back: Theme = toml::from_str(&s).expect("deserialize");
        assert_eq!(theme, back);
    }

    #[test]
    fn validate_rejects_dangling_token() {
        let mut theme = Theme::default();
        theme
            .components
            .get_mut("sidebar")
            .unwrap()
            .slots
            .insert(
                "bg".into(),
                Binding {
                    group: "surface".into(),
                    token: "no-such-token".into(),
                },
            );
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
        let mut theme = Theme::default();
        // Point sidebar's `border` slot at `bg-primary`, which is a
        // surface-only token (its `groups: ["surface"]` doesn't include
        // "border"). Both the kind matches (Color) so we hit
        // GroupNotInToken, not GroupKindMismatch.
        theme
            .components
            .get_mut("sidebar")
            .unwrap()
            .slots
            .insert(
                "border".into(),
                Binding {
                    group: "border".into(),
                    token: "bg-primary".into(),
                },
            );
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
}
