//! Theme validation — every binding's group/token/kind must be
//! mutually consistent with the palette. Errors are collected (not
//! short-circuited) so a future editor can surface all problems at
//! once.

use super::types::{SlotName, Theme, TokenKind, TokenName};

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
pub(super) fn group_kind(group: &str) -> Option<TokenKind> {
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
}
