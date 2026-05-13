//! Theme → CSS lowering. The renderer only ever sees CSS — this
//! module is the sole producer of the `:root { … }` block that is
//! pushed to every kit window via `__solaRecv` whenever
//! `Topic::Theme` is delivered.

use super::types::Theme;

impl Theme {
    /// Render this theme into the full `:root { … }` CSS block, atoms
    /// first (one var per palette token, name = key), then per-component
    /// scoped vars (`--sola-<component>-<slot>: var(--<token>);`).
    ///
    /// Output is deterministic — every map iterated here is `BTreeMap`,
    /// so iteration is alphabetical. Golden snapshot tests in downstream
    /// crates lock the exact byte sequence.
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
