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
