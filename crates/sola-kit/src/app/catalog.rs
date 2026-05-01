//! Static catalog of every atom + component the kit ships, with the
//! CSS-token vars each one uses. Used by the storybook for the sidebar
//! AND for the reverse index ("which components use --accent?").
//!
//! These lists must match the `*Tokens` exports in
//! `web/lib/components/<name>.ts`. The parity test below enforces it.

#[derive(Debug, Clone, Copy)]
pub struct CatalogEntry {
    pub name: &'static str,
    pub group: Group,
    pub tokens: &'static [&'static str],
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Group {
    Atom,
    Component,
}

pub static CATALOG: &[CatalogEntry] = &[
    // Atoms
    CatalogEntry {
        name: "button",
        group: Group::Atom,
        tokens: &[
            "--accent", "--accent-dim",
            "--bg-tertiary", "--text-secondary", "--text-primary",
            "--danger", "--border-subtle",
            "--radius-sm", "--text-body", "--space-sm", "--space-md",
        ],
    },
    CatalogEntry {
        name: "field",
        group: Group::Atom,
        tokens: &[
            "--bg-primary", "--border-subtle", "--accent",
            "--text-primary", "--danger",
            "--radius-sm", "--text-body", "--space-xs", "--space-sm",
        ],
    },
    CatalogEntry {
        name: "badge",
        group: Group::Atom,
        tokens: &[
            "--bg-tertiary", "--text-secondary",
            "--accent", "--accent-dim",
            "--danger", "--success",
            "--radius-sm", "--text-caption", "--space-xs",
        ],
    },
    CatalogEntry {
        name: "icon",
        group: Group::Atom,
        tokens: &["--text-secondary"],
    },
    // Components
    CatalogEntry {
        name: "sidebar",
        group: Group::Component,
        tokens: &[
            "--bg-secondary", "--border-subtle", "--text-muted",
            "--space-xs", "--space-sm", "--space-md",
            "--text-caption",
        ],
    },
    CatalogEntry {
        name: "nav-item",
        group: Group::Component,
        tokens: &[
            "--text-secondary", "--text-primary",
            "--bg-tertiary", "--accent", "--accent-dim",
            "--radius-sm", "--text-body", "--space-xs", "--space-sm",
        ],
    },
    CatalogEntry {
        name: "section",
        group: Group::Component,
        tokens: &[
            "--text-primary", "--text-tertiary",
            "--text-heading", "--text-body",
            "--space-xs", "--space-md", "--space-lg",
        ],
    },
    CatalogEntry {
        name: "row",
        group: Group::Component,
        tokens: &[
            "--bg-secondary", "--text-primary", "--text-tertiary",
            "--radius-md", "--text-body", "--text-caption",
            "--space-sm", "--space-md",
        ],
    },
    CatalogEntry {
        name: "list",
        group: Group::Component,
        tokens: &["--space-xs"],
    },
    CatalogEntry {
        name: "form",
        group: Group::Component,
        tokens: &[
            "--bg-secondary", "--text-secondary",
            "--radius-md", "--text-body",
            "--space-sm", "--space-md",
        ],
    },
    CatalogEntry {
        name: "tabs",
        group: Group::Component,
        tokens: &[
            "--bg-secondary", "--bg-tertiary", "--accent-dim", "--accent",
            "--text-secondary", "--text-primary", "--border-subtle",
            "--radius-sm", "--text-body", "--text-caption",
            "--space-xs", "--space-sm",
        ],
    },
    CatalogEntry {
        name: "toast",
        group: Group::Component,
        tokens: &[
            "--bg-secondary", "--border-subtle",
            "--accent", "--success", "--danger",
            "--radius-md", "--text-body",
            "--space-sm", "--space-md",
        ],
    },
    CatalogEntry {
        name: "empty",
        group: Group::Component,
        tokens: &[
            "--text-muted",
            "--text-body", "--text-caption",
            "--space-md",
        ],
    },
];

/// Reverse index: which components consume the given token?
pub fn consumers_of(token: &str) -> Vec<&'static CatalogEntry> {
    CATALOG.iter().filter(|e| e.tokens.contains(&token)).collect()
}

#[cfg(test)]
mod parity {
    //! Asserts each Rust CATALOG entry's tokens match the corresponding
    //! `*Tokens` export in `web/lib/components/<name>.ts`. We parse the
    //! TS file naively (regex on the array literal); good enough for our
    //! single-line declarations.

    use super::*;

    fn js_tokens_for(name: &str) -> Vec<String> {
        let path = format!("web/lib/components/{name}.ts");
        let src = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {path}: {e}"));
        // Find `export const <camel>Tokens = [...]` and pull out the var
        // strings (anything between single quotes inside the array).
        // Convert kebab-case to lowerCamelCase: "nav-item" → "navItem".
        let camel = {
            let mut parts = name.split('-');
            let first = parts.next().unwrap_or("").to_string();
            let rest: String = parts
                .map(|p| {
                    let mut c = p.chars();
                    match c.next() {
                        Some(h) => h.to_uppercase().collect::<String>() + c.as_str(),
                        None => String::new(),
                    }
                })
                .collect();
            first + rest.as_str()
        };
        let needle = format!("{camel}Tokens");
        let start = src.find(&needle).unwrap_or_else(|| panic!("no {needle} in {path}"));
        let after = &src[start..];
        let bracket = after.find('[').expect("no [ after Tokens");
        let close = after[bracket..].find(']').expect("no ] after Tokens");
        let inner = &after[bracket + 1..bracket + close];
        let mut out = Vec::new();
        for chunk in inner.split(',') {
            let t = chunk.trim();
            let t = t.trim_matches(|c| c == '\'' || c == '"' || c == '\n' || c == ' ');
            if !t.is_empty() {
                out.push(t.to_string());
            }
        }
        out
    }

    #[test]
    fn rust_catalog_matches_typescript_exports() {
        for entry in CATALOG {
            // nav-item.ts → navItemTokens, button.ts → buttonTokens, etc.
            let js: Vec<String> = js_tokens_for(entry.name);
            let rs: Vec<String> = entry.tokens.iter().map(|s| s.to_string()).collect();
            let mut js_sorted = js.clone();
            js_sorted.sort();
            let mut rs_sorted = rs.clone();
            rs_sorted.sort();
            assert_eq!(
                js_sorted, rs_sorted,
                "catalog mismatch for {}: rust={:?}, ts={:?}",
                entry.name, rs, js
            );
        }
    }
}
