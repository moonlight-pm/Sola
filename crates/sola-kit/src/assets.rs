/// Content type for embedded assets.
#[derive(Clone, Copy, Debug)]
pub enum ContentType {
    Html,
    Css,
    JavaScript,
    TypeScript,
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
            Self::JavaScript | Self::TypeScript => "application/javascript",
            Self::Svg => "image/svg+xml",
        }
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
        // For .js requests, try .ts source (browser requests .js from import extensions)
        if path.ends_with(".js") {
            let ts_path = format!("{}ts", &path[..path.len() - 2]);
            return self.assets.iter().find(|a| a.path == ts_path);
        }
        None
    }
}

/// Platform assets embedded from sola-kit's own web/ directory.
pub fn platform_assets() -> AssetBundle {
    AssetBundle {
        assets: &[
            Asset {
                path: "/lib/ipc.ts",
                content: include_str!("../web/lib/ipc.ts"),
                content_type: ContentType::TypeScript,
            },
            Asset {
                path: "/lib/store.ts",
                content: include_str!("../web/lib/store.ts"),
                content_type: ContentType::TypeScript,
            },
            Asset {
                path: "/lib/kit.ts",
                content: include_str!("../web/lib/kit.ts"),
                content_type: ContentType::TypeScript,
            },
            Asset {
                path: "/lib/kit.css",
                content: include_str!("../web/lib/kit.css"),
                content_type: ContentType::Css,
            },
            Asset {
                path: "/lib/components/button.ts",
                content: include_str!("../web/lib/components/button.ts"),
                content_type: ContentType::TypeScript,
            },
            Asset {
                path: "/vendor/arrow/index.mjs",
                content: include_str!("../web/vendor/arrow/index.mjs"),
                content_type: ContentType::JavaScript,
            },
            Asset {
                path: "/vendor/arrow/chunks/internal-DchK7S7v.mjs",
                content: include_str!("../web/vendor/arrow/chunks/internal-DchK7S7v.mjs"),
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
