//! Design-token schema shared by sola-bus (the wire type for `Topic::Theme`)
//! and sola-kit (consumer + editor). Lives in sola-core because sola-bus
//! depends on sola-core, not the other way around — same arrangement as
//! `crate::applications::Application`.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Theme {
    pub colors: Colors,
    pub typography: Typography,
    pub spacing: Spacing,
    pub radius: Radius,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Colors {
    pub bg_primary: String,
    pub bg_secondary: String,
    pub bg_tertiary: String,
    pub bg_hover: String,
    pub border: String,
    pub border_subtle: String,
    pub text_primary: String,
    pub text_secondary: String,
    pub text_tertiary: String,
    pub text_muted: String,
    pub text_accent: String,
    pub accent: String,
    pub accent_dim: String,
    pub danger: String,
    pub success: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Typography {
    pub font_sans: String,
    pub font_mono: String,
    pub text_caption: String,
    pub text_body: String,
    pub text_body_lg: String,
    pub text_heading: String,
    pub text_display: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Spacing {
    pub xs: String,
    pub sm: String,
    pub md: String,
    pub lg: String,
    pub xl: String,
    pub xxl: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Radius {
    pub sm: String,
    pub md: String,
    pub lg: String,
}

impl Default for Theme {
    fn default() -> Self {
        Self {
            colors: Colors {
                bg_primary: "#0d1117".into(),
                bg_secondary: "#161b22".into(),
                bg_tertiary: "#1c2129".into(),
                bg_hover: "#1a2030".into(),
                border: "#2d333b".into(),
                border_subtle: "#21262d".into(),
                text_primary: "#e6edf3".into(),
                text_secondary: "#8b949e".into(),
                text_tertiary: "#6e7681".into(),
                text_muted: "#484f58".into(),
                text_accent: "#58a6ff".into(),
                accent: "#00d4ff".into(),
                accent_dim: "rgba(0, 212, 255, 0.12)".into(),
                danger: "#f85149".into(),
                success: "#3fb950".into(),
            },
            typography: Typography {
                font_sans: "'DM Sans', system-ui, sans-serif".into(),
                font_mono: "'JetBrains Mono', 'Fira Code', 'Source Code Pro', monospace".into(),
                text_caption: "11px".into(),
                text_body: "12px".into(),
                text_body_lg: "13px".into(),
                text_heading: "16px".into(),
                text_display: "20px".into(),
            },
            spacing: Spacing {
                xs: "4px".into(),
                sm: "8px".into(),
                md: "12px".into(),
                lg: "16px".into(),
                xl: "20px".into(),
                xxl: "24px".into(),
            },
            radius: Radius {
                sm: "3px".into(),
                md: "4px".into(),
                lg: "6px".into(),
            },
        }
    }
}

impl Theme {
    /// Flatten a `Theme` into the CSS-custom-property map that `applyTheme`
    /// applies to `:root` in the WebView. Var names are deterministic.
    /// Returns a `BTreeMap` so iteration order is stable for tests.
    pub fn to_css_vars(&self) -> BTreeMap<String, String> {
        let mut m = BTreeMap::new();
        let c = &self.colors;
        m.insert("--bg-primary".into(), c.bg_primary.clone());
        m.insert("--bg-secondary".into(), c.bg_secondary.clone());
        m.insert("--bg-tertiary".into(), c.bg_tertiary.clone());
        m.insert("--bg-hover".into(), c.bg_hover.clone());
        m.insert("--border".into(), c.border.clone());
        m.insert("--border-subtle".into(), c.border_subtle.clone());
        m.insert("--text-primary".into(), c.text_primary.clone());
        m.insert("--text-secondary".into(), c.text_secondary.clone());
        m.insert("--text-tertiary".into(), c.text_tertiary.clone());
        m.insert("--text-muted".into(), c.text_muted.clone());
        m.insert("--text-accent".into(), c.text_accent.clone());
        m.insert("--accent".into(), c.accent.clone());
        m.insert("--accent-dim".into(), c.accent_dim.clone());
        m.insert("--danger".into(), c.danger.clone());
        m.insert("--success".into(), c.success.clone());

        let t = &self.typography;
        m.insert("--font-sans".into(), t.font_sans.clone());
        m.insert("--font-mono".into(), t.font_mono.clone());
        m.insert("--text-caption".into(), t.text_caption.clone());
        m.insert("--text-body".into(), t.text_body.clone());
        m.insert("--text-body-lg".into(), t.text_body_lg.clone());
        m.insert("--text-heading".into(), t.text_heading.clone());
        m.insert("--text-display".into(), t.text_display.clone());

        let s = &self.spacing;
        m.insert("--space-xs".into(), s.xs.clone());
        m.insert("--space-sm".into(), s.sm.clone());
        m.insert("--space-md".into(), s.md.clone());
        m.insert("--space-lg".into(), s.lg.clone());
        m.insert("--space-xl".into(), s.xl.clone());
        m.insert("--space-xxl".into(), s.xxl.clone());

        let r = &self.radius;
        m.insert("--radius-sm".into(), r.sm.clone());
        m.insert("--radius-md".into(), r.md.clone());
        m.insert("--radius-lg".into(), r.lg.clone());

        m
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_to_css_vars_has_expected_count() {
        let vars = Theme::default().to_css_vars();
        // 15 colors + 7 typography + 6 spacing + 3 radius
        assert_eq!(vars.len(), 31);
    }

    #[test]
    fn default_to_css_vars_sample_values() {
        let vars = Theme::default().to_css_vars();
        assert_eq!(vars.get("--bg-primary").unwrap(), "#0d1117");
        assert_eq!(vars.get("--accent").unwrap(), "#00d4ff");
        assert_eq!(vars.get("--space-md").unwrap(), "12px");
        assert_eq!(vars.get("--radius-sm").unwrap(), "3px");
        assert_eq!(vars.get("--font-sans").unwrap(), "'DM Sans', system-ui, sans-serif");
    }

    #[test]
    fn theme_round_trips_through_toml() {
        let theme = Theme::default();
        let s = toml::to_string(&theme).expect("serialize");
        let back: Theme = toml::from_str(&s).expect("deserialize");
        assert_eq!(theme, back);
    }
}
