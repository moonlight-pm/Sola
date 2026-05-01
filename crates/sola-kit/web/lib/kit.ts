//! Sola Kit — design tokens, atoms, components.
//
// applyTheme is the one bit needed before Phase 3. Atoms/components and
// the auto-installed bus listener arrive in later phases.

/** Apply a map of CSS custom properties to :root. */
export function applyTheme(vars: Record<string, string>): void {
  const root = document.documentElement;
  for (const [key, value] of Object.entries(vars)) {
    root.style.setProperty(key.startsWith('--') ? key : `--${key}`, value);
  }
}
