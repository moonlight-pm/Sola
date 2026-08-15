//! Theme protocol types — the wire shape used by `Topic::Theme` on the
//! bus and consumed (lowered to CSS, validated, edited) elsewhere. Two
//! layers: a flat palette of named atoms, and per-component bindings
//! that point at those atoms via selection groups.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// A token's name (key into `Palette.tokens`). Stringly-typed so adding
/// a new token is one map insertion and zero type churn.
pub type TokenName = String;

/// A component slot's name (key into `ComponentBindings.slots`).
pub type SlotName = String;

// Note on Default: we don't `#[derive(Default)]` because the derived
// (empty palette) value, while technically valid, is rarely useful. The
// manual impl below seeds the palette with the v1 atom catalog (see
// `Palette::seed`) and leaves `components` empty — sola-kit composes
// its own component bindings on top via `kit_default_theme()`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Theme {
    pub palette: Palette,
    pub components: BTreeMap<String, ComponentBindings>,
}

/// A theme persisted under a user-supplied name. Lives behind the
/// `Topic::CustomTheme` persistent topic (keyed by `name`) so the
/// storybook's user-created presets survive restart. The hardcoded
/// "Default" preset is *not* round-tripped through this type — it
/// reconstitutes itself from Rust constants every boot.
///
/// `name` is also the on-disk filename
/// (`~/.config/sola/theme/presets/<name>.yaml`), so it's constrained
/// to strict kebab-case via [`is_valid_theme_name`]. Emitters should
/// reject invalid names at the edge (UI input) and the bus host
/// drops persists for invalid names as a defensive measure.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct NamedTheme {
    pub name: String,
    pub theme: Theme,
}

/// True if `name` is a valid custom-theme identifier: lowercase
/// letters joined by single hyphens, no leading/trailing hyphen, no
/// double hyphens, no digits. Examples: `alpha`, `solar-flare`. Used
/// both for the in-memory `NamedTheme.name` and the on-disk filename
/// under `~/.config/sola/theme/presets/`.
pub fn is_valid_theme_name(name: &str) -> bool {
    if name.is_empty() {
        return false;
    }
    let bytes = name.as_bytes();
    if !bytes[0].is_ascii_lowercase() || !bytes[bytes.len() - 1].is_ascii_lowercase() {
        return false;
    }
    let mut prev_hyphen = false;
    for &b in bytes {
        match b {
            b'a'..=b'z' => prev_hyphen = false,
            b'-' => {
                if prev_hyphen {
                    return false;
                }
                prev_hyphen = true;
            }
            _ => return false,
        }
    }
    true
}

impl Default for Theme {
    fn default() -> Self {
        Self {
            palette: Palette::seed(),
            components: BTreeMap::new(),
        }
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

impl Token {
    /// Convenience constructor for seed code. Keeps the verbose
    /// `Token { kind: …, value: "…".into(), groups: vec![…] }` boilerplate
    /// out of every entry.
    pub fn new(kind: TokenKind, value: impl Into<String>, groups: &[&str]) -> Self {
        Self {
            kind,
            value: value.into(),
            groups: groups.iter().map(|s| (*s).to_string()).collect(),
        }
    }
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

impl Binding {
    /// Convenience constructor for seed code.
    pub fn new(group: impl Into<String>, token: impl Into<TokenName>) -> Self {
        Self {
            group: group.into(),
            token: token.into(),
        }
    }
}
