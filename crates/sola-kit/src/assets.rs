/// Content type for embedded assets.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ContentType {
    Html,
    Css,
    JavaScript,
    TypeScript,
    /// TypeScript with JSX. Goes through the swc TS+JSX transform.
    Tsx,
    /// JavaScript with JSX. Goes through the swc JSX transform (no type strip).
    Jsx,
    Svg,
}

impl ContentType {
    /// Detect content type from file extension.
    pub fn from_path(path: &str) -> Option<Self> {
        if path.ends_with(".d.ts") {
            return None; // Skip declaration files
        }
        if path.ends_with(".html") {
            Some(Self::Html)
        } else if path.ends_with(".css") {
            Some(Self::Css)
        } else if path.ends_with(".tsx") {
            Some(Self::Tsx)
        } else if path.ends_with(".jsx") {
            Some(Self::Jsx)
        } else if path.ends_with(".js") || path.ends_with(".mjs") {
            Some(Self::JavaScript)
        } else if path.ends_with(".ts") {
            Some(Self::TypeScript)
        } else if path.ends_with(".svg") {
            Some(Self::Svg)
        } else {
            None
        }
    }

    pub fn mime(&self) -> &'static str {
        match self {
            Self::Html => "text/html; charset=utf-8",
            Self::Css => "text/css",
            Self::JavaScript | Self::TypeScript | Self::Tsx | Self::Jsx => {
                "application/javascript"
            }
            Self::Svg => "image/svg+xml",
        }
    }

    /// True if this content type needs a JSX→h() transform pass.
    pub fn has_jsx(&self) -> bool {
        matches!(self, Self::Tsx | Self::Jsx)
    }

    /// True if this content type needs TypeScript type stripping.
    pub fn has_types(&self) -> bool {
        matches!(self, Self::TypeScript | Self::Tsx)
    }
}

/// A single embedded asset.
pub struct Asset {
    pub path: &'static str,
    pub content: &'static str,
    pub content_type: ContentType,
}

/// Collection of embedded assets from an app's web directory.
pub struct AssetBundle {
    pub assets: &'static [Asset],
}

impl AssetBundle {
    pub fn find(&self, path: &str) -> Option<&Asset> {
        // Try exact match first
        if let Some(asset) = self.assets.iter().find(|a| a.path == path) {
            return Some(asset);
        }
        // For .js requests, try .ts/.tsx/.jsx source (browser requests .js
        // from import extensions; we keep the import-source as the file extension).
        if path.ends_with(".js") {
            let stem = &path[..path.len() - 2];
            for ext in ["ts", "tsx", "jsx"] {
                let candidate = format!("{stem}{ext}");
                if let Some(a) = self.assets.iter().find(|a| a.path == candidate) {
                    return Some(a);
                }
            }
            return None;
        }
        // Extensionless: try .ts, .tsx, .jsx, .js, .mjs in order. Matches what
        // tsconfig "moduleResolution: bundler" lets the editor accept, so
        // `import './foo'` resolves at runtime the same way it does in the LSP.
        let last_seg = path.rsplit('/').next().unwrap_or("");
        if !last_seg.is_empty() && !last_seg.contains('.') {
            for ext in [".ts", ".tsx", ".jsx", ".js", ".mjs"] {
                let candidate = format!("{path}{ext}");
                if let Some(a) = self.assets.iter().find(|a| a.path == candidate) {
                    return Some(a);
                }
            }
        }
        None
    }
}

/// Platform assets embedded from sola-kit's own web/ directory.
///
/// Currently only the vendored Preact runtime — `lib/` modules will be
/// re-introduced module-by-module against this slot as we bootstrap the
/// Preact-based kit.
pub fn platform_assets() -> AssetBundle {
    AssetBundle {
        assets: &[
            Asset {
                path: "/lib/ipc.ts",
                content: include_str!("../web/lib/ipc.ts"),
                content_type: ContentType::TypeScript,
            },
            Asset {
                path: "/vendor/preact/preact.module.js",
                content: include_str!("../web/vendor/preact/preact.module.js"),
                content_type: ContentType::JavaScript,
            },
            Asset {
                path: "/vendor/preact/hooks.module.js",
                content: include_str!("../web/vendor/preact/hooks.module.js"),
                content_type: ContentType::JavaScript,
            },
            Asset {
                path: "/vendor/preact/signals-core.module.js",
                content: include_str!("../web/vendor/preact/signals-core.module.js"),
                content_type: ContentType::JavaScript,
            },
            Asset {
                path: "/vendor/preact/signals.module.js",
                content: include_str!("../web/vendor/preact/signals.module.js"),
                content_type: ContentType::JavaScript,
            },
        ],
    }
}

/// Macro to embed all servable files from a web directory at compile time.
///
/// Usage: `embed_web!("web/")`
///
/// Generates a static `AssetBundle` containing all .html, .css, .js, .mjs, .ts
/// files found in the directory (excluding .d.ts files).
#[macro_export]
macro_rules! embed_web {
    ($dir:literal) => {{
        // NOTE: This macro requires manual listing of files since proc macros
        // that walk the filesystem at compile time need a separate proc-macro crate.
        // Apps list their assets explicitly using the embed_assets! helper.
        compile_error!(
            "embed_web! requires explicit file listing. Use sola_kit::asset_bundle! instead."
        )
    }};
}

/// Build an AssetBundle from explicit file entries.
///
/// Usage:
/// ```ignore
/// let assets = sola_kit::asset_bundle! {
///     "/index.html" => (include_str!("../web/index.html"), Html),
///     "/src/app.ts" => (include_str!("../web/src/app.ts"), TypeScript),
/// };
/// ```
#[macro_export]
macro_rules! asset_bundle {
    ( $( $path:literal => ($content:expr, $ctype:ident) ),* $(,)? ) => {
        $crate::AssetBundle {
            assets: &[
                $(
                    $crate::Asset {
                        path: $path,
                        content: $content,
                        content_type: $crate::ContentType::$ctype,
                    },
                )*
            ],
        }
    };
}
