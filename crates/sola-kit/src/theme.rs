//! Kit-level theme composition. sola-core owns the *types* and the
//! *palette seed* (brand atoms); this module composes the kit's
//! per-component bindings on top to produce the canonical default
//! theme that `KitApp::new` publishes on `Topic::Theme` at startup.

use sola_core::theme::{Palette, Theme};

use crate::components;

/// The kit's default theme — palette atoms from sola-core plus
/// bindings for every kit-shipped component (button, root, sidebar).
///
/// Apps should treat this as a "factory reset" baseline; the canonical
/// owner of the live theme is sola-shell (or sola-bus, whoever owns
/// the persistent `Topic::Theme` payload). Until that ownership move
/// happens, the storybook seeds the bus with this on startup.
pub fn kit_default_theme() -> Theme {
    Theme {
        palette: Palette::seed(),
        components: components::all_bindings(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kit_default_theme_validates() {
        kit_default_theme()
            .validate()
            .expect("kit default theme must validate");
    }

    /// Golden snapshot of the kit's full rendered `:root { … }` block.
    /// This is what every kit window actually sees; any palette or
    /// binding edit shows up as a diff. Update the expected string
    /// deliberately.
    #[test]
    fn kit_default_theme_to_css_is_stable() {
        let css = kit_default_theme().to_css();
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

  /* button */
  --sola-button-danger-bg: var(--danger);
  --sola-button-danger-text: var(--text-primary);
  --sola-button-default-bg: var(--bg-tertiary);
  --sola-button-default-bg-hover: var(--bg-hover);
  --sola-button-default-border: var(--border);
  --sola-button-default-text: var(--text-primary);
  --sola-button-focus-ring: var(--accent);
  --sola-button-gap: var(--space-xs);
  --sola-button-ghost-bg-hover: var(--bg-hover);
  --sola-button-ghost-text: var(--text-secondary);
  --sola-button-padding-block: var(--space-sm);
  --sola-button-padding-inline: var(--space-md);
  --sola-button-primary-bg: var(--accent);
  --sola-button-primary-text: var(--text-primary);
  --sola-button-radius: var(--radius-md);
  --sola-button-text-size: var(--text-body);

  /* field */
  --sola-field-error-color: var(--danger);
  --sola-field-gap: var(--space-xs);
  --sola-field-help-color: var(--text-tertiary);
  --sola-field-label-color: var(--text-secondary);
  --sola-field-label-size: var(--text-caption);

  /* root */
  --sola-root-bg: var(--bg-primary);
  --sola-root-font: var(--font-sans);
  --sola-root-text: var(--text-primary);
  --sola-root-text-size: var(--text-body);

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

  /* text-input */
  --sola-text-input-bg: var(--bg-tertiary);
  --sola-text-input-border: var(--border);
  --sola-text-input-border-focus: var(--accent);
  --sola-text-input-border-invalid: var(--danger);
  --sola-text-input-padding-block: var(--space-xs);
  --sola-text-input-padding-inline: var(--space-sm);
  --sola-text-input-placeholder-color: var(--text-muted);
  --sola-text-input-radius: var(--radius-md);
  --sola-text-input-text: var(--text-primary);
  --sola-text-input-text-size: var(--text-body);
}
";
        assert_eq!(css, expected);
    }
}
