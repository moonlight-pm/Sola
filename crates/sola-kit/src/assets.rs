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
    /// JSON (incl. source maps).
    Json,
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
        } else if path.ends_with(".map") || path.ends_with(".json") {
            Some(Self::Json)
        } else {
            None
        }
    }

    /// Bare MIME (no charset). CEF's `Response::set_mime_type` wants the
    /// type only — charset is set separately via `set_charset`, and the
    /// composite `Content-Type` header is built by Chromium downstream.
    /// Smuggling a charset in here causes Chromium to mis-detect the type
    /// and fall back to plaintext rendering.
    pub fn mime(&self) -> &'static str {
        match self {
            Self::Html => "text/html",
            Self::Css => "text/css",
            Self::JavaScript | Self::TypeScript | Self::Tsx | Self::Jsx => {
                "application/javascript"
            }
            Self::Svg => "image/svg+xml",
            Self::Json => "application/json",
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

/// A single embedded asset. `content` is bytes, not str — text assets stay
/// UTF-8 by construction (we control the inputs) but the broader pipeline
/// is byte-clean for free, and it lets us share the same struct for files
/// resolved via either `include_bytes!` or `include_dir!`'s `File` (which
/// is also bytes-native).
#[derive(Clone, Copy)]
pub struct Asset {
    pub path: &'static str,
    pub content: &'static [u8],
    pub content_type: ContentType,
}

/// Mount an `include_dir!` tree at a URL prefix. Files inside the tree are
/// served as `<url_prefix><relative path inside dir>`. ContentType is
/// derived from the file extension via `ContentType::from_path`.
///
/// Used when an asset bundle wants to embed a whole vendored subtree
/// (`web/vendor/remix-ui/`, etc.) without enumerating every file.
pub struct DirMount {
    pub url_prefix: &'static str,
    pub dir: &'static include_dir::Dir<'static>,
}

/// Collection of embedded assets from an app's web directory.
///
/// Lookups walk `assets` first (exact path, then `.js → .ts/.tsx/.jsx`
/// fallback, then extensionless probe), then each `dirs` mount in order
/// (same fallback rules applied within the mount). Conceptually:
///
/// ```text
/// app:///vendor/remix-ui/runtime/component.ts
///                                                ↓
///   assets[]    (per-file include_bytes!)        ↓ no match
///                                                ↓
///   dirs[N] where url_prefix = "/vendor/remix-ui/"
///   → look up "runtime/component.ts" inside the include_dir!() tree
///   → derive ContentType from ".ts" extension → TypeScript
///   → return synthesized Asset
/// ```
pub struct AssetBundle {
    pub assets: &'static [Asset],
    pub dirs: &'static [DirMount],
}

impl AssetBundle {
    /// Resolve `path` to an `Asset`, with the same `.js → .ts/.tsx/.jsx`
    /// and extensionless fallbacks the LSP applies. Returns an owned
    /// `Asset` (it's `Copy`) so dir-mounted lookups can synthesize on the
    /// fly without an arena.
    pub fn find(&self, path: &str) -> Option<Asset> {
        if let Some(a) = self.find_in_assets(path) {
            return Some(a);
        }
        for mount in self.dirs {
            if let Some(rest) = path.strip_prefix(mount.url_prefix) {
                if let Some(a) = find_in_dir(mount, rest) {
                    return Some(a);
                }
            }
        }
        None
    }

    fn find_in_assets(&self, path: &str) -> Option<Asset> {
        if let Some(asset) = self.assets.iter().find(|a| a.path == path) {
            return Some(*asset);
        }
        // For .js requests, try .ts/.tsx/.jsx source (browser requests .js
        // from import extensions; we keep the source on disk as the
        // original .ts/.tsx/.jsx).
        if path.ends_with(".js") {
            let stem = &path[..path.len() - 2];
            for ext in ["ts", "tsx", "jsx"] {
                let candidate = format!("{stem}{ext}");
                if let Some(a) = self.assets.iter().find(|a| a.path == candidate) {
                    return Some(*a);
                }
            }
            return None;
        }
        // Extensionless: try .ts, .tsx, .jsx, .js, .mjs in order. Matches
        // what tsconfig "moduleResolution: bundler" accepts, so
        // `import './foo'` resolves at runtime the same way it does in
        // the LSP.
        let last_seg = path.rsplit('/').next().unwrap_or("");
        if !last_seg.is_empty() && !last_seg.contains('.') {
            for ext in [".ts", ".tsx", ".jsx", ".js", ".mjs"] {
                let candidate = format!("{path}{ext}");
                if let Some(a) = self.assets.iter().find(|a| a.path == candidate) {
                    return Some(*a);
                }
            }
        }
        None
    }
}

/// Look up `rel` (relative path inside the dir) using the same fallback
/// rules as the assets array. The mount's URL prefix is re-prepended when
/// building the synthesized `Asset.path` so downstream consumers see the
/// full URL they requested.
fn find_in_dir(mount: &DirMount, rel: &str) -> Option<Asset> {
    if let Some(a) = lookup_dir_exact(mount, rel) {
        return Some(a);
    }
    if let Some(stem) = rel.strip_suffix(".js") {
        for ext in ["ts", "tsx", "jsx"] {
            let candidate = format!("{stem}.{ext}");
            if let Some(a) = lookup_dir_exact(mount, &candidate) {
                return Some(a);
            }
        }
        return None;
    }
    let last_seg = rel.rsplit('/').next().unwrap_or("");
    if !last_seg.is_empty() && !last_seg.contains('.') {
        for ext in [".ts", ".tsx", ".jsx", ".js", ".mjs"] {
            let candidate = format!("{rel}{ext}");
            if let Some(a) = lookup_dir_exact(mount, &candidate) {
                return Some(a);
            }
        }
    }
    None
}

fn lookup_dir_exact(mount: &DirMount, rel: &str) -> Option<Asset> {
    let file = mount.dir.get_file(rel)?;
    // ContentType is derived from extension; declaration files (.d.ts)
    // and unknowns are filtered out so the dir mount can't accidentally
    // serve types or random binaries.
    let ctype = ContentType::from_path(rel)?;
    // Leak the joined path to get a `&'static str`. This is one
    // allocation per *unique* missing-then-found path per process; in
    // practice the renderer requests each module URL once. Acceptable
    // for now; if it becomes a hotspot, intern via a OnceLock<HashMap>.
    let full_path: &'static str = Box::leak(
        format!("{}{}", mount.url_prefix, rel).into_boxed_str(),
    );
    Some(Asset {
        path: full_path,
        content: file.contents(),
        content_type: ctype,
    })
}

/// Vendored `@remix-run/ui` source tree — kit infrastructure shared by
/// every app. Mounted under `/vendor/remix-ui/*` and resolved via the
/// kit-injected importmap (`@remix-run/ui` → `/vendor/remix-ui/index.ts`).
static REMIX_UI_DIR: include_dir::Dir<'_> =
    include_dir::include_dir!("$CARGO_MANIFEST_DIR/web/vendor/remix-ui");

/// Vendored `@chenglou/pretext` — pre-built dist from the npm tarball.
/// Used by PopoverSelect (and any other consumer) for synchronous,
/// DOM-free text-width measurement via canvas `measureText`. Mounted
/// under `/vendor/pretext/*`; the `@chenglou/pretext` importmap entry
/// points at `dist/layout.js`.
static PRETEXT_DIR: include_dir::Dir<'_> =
    include_dir::include_dir!("$CARGO_MANIFEST_DIR/web/vendor/pretext");

/// Platform assets the kit serves on every app:// scheme request, regardless
/// of which app it's hosting.
///
/// Includes:
/// - The default `/index.html` and `/index.tsx` — apps don't ship
///   their own unless they need to customise; `ctx::add_window` falls
///   back here when an app bundle has no `/index.html`.
/// - The IPC bridge (`/lib/ipc.ts`) and kit helpers (`/lib/kit.ts`).
/// - Every kit-shipped component (TS source + CSS). The kit's
///   `inject_kit_head` walks this bundle for `Css` assets and emits a
///   `<link rel="stylesheet">` for each — apps don't enumerate
///   component stylesheets in their own `index.html`.
/// - The vendored Remix v3 source tree under `/vendor/remix-ui/`.
pub fn platform_assets() -> &'static AssetBundle {
    static PLATFORM: AssetBundle = AssetBundle {
        assets: &[
            Asset {
                path: "/index.html",
                content: include_bytes!("../web/lib/index.html"),
                content_type: ContentType::Html,
            },
            Asset {
                path: "/index.tsx",
                content: include_bytes!("../web/lib/index.tsx"),
                content_type: ContentType::Tsx,
            },
            Asset {
                path: "/lib/ipc.ts",
                content: include_bytes!("../web/lib/ipc.ts"),
                content_type: ContentType::TypeScript,
            },
            Asset {
                path: "/lib/kit.ts",
                content: include_bytes!("../web/lib/kit.ts"),
                content_type: ContentType::TypeScript,
            },
            Asset {
                path: "/lib/base.css",
                content: include_bytes!("../web/lib/base.css"),
                content_type: ContentType::Css,
            },
            Asset {
                path: "/lib/components/bindings-editor.tsx",
                content: include_bytes!("../web/lib/components/bindings-editor.tsx"),
                content_type: ContentType::Tsx,
            },
            Asset {
                path: "/lib/components/bindings-editor.css",
                content: include_bytes!("../web/lib/components/bindings-editor.css"),
                content_type: ContentType::Css,
            },
            Asset {
                path: "/lib/components/button.tsx",
                content: include_bytes!("../web/lib/components/button.tsx"),
                content_type: ContentType::Tsx,
            },
            Asset {
                path: "/lib/components/button.css",
                content: include_bytes!("../web/lib/components/button.css"),
                content_type: ContentType::Css,
            },
            Asset {
                path: "/lib/components/card.tsx",
                content: include_bytes!("../web/lib/components/card.tsx"),
                content_type: ContentType::Tsx,
            },
            Asset {
                path: "/lib/components/card.css",
                content: include_bytes!("../web/lib/components/card.css"),
                content_type: ContentType::Css,
            },
            Asset {
                path: "/lib/components/color-input.tsx",
                content: include_bytes!("../web/lib/components/color-input.tsx"),
                content_type: ContentType::Tsx,
            },
            Asset {
                path: "/lib/components/color-input.css",
                content: include_bytes!("../web/lib/components/color-input.css"),
                content_type: ContentType::Css,
            },
            Asset {
                path: "/lib/components/color-picker.tsx",
                content: include_bytes!("../web/lib/components/color-picker.tsx"),
                content_type: ContentType::Tsx,
            },
            Asset {
                path: "/lib/components/color-picker.css",
                content: include_bytes!("../web/lib/components/color-picker.css"),
                content_type: ContentType::Css,
            },
            Asset {
                path: "/lib/components/field.tsx",
                content: include_bytes!("../web/lib/components/field.tsx"),
                content_type: ContentType::Tsx,
            },
            Asset {
                path: "/lib/components/field.css",
                content: include_bytes!("../web/lib/components/field.css"),
                content_type: ContentType::Css,
            },
            Asset {
                path: "/lib/components/font-input.tsx",
                content: include_bytes!("../web/lib/components/font-input.tsx"),
                content_type: ContentType::Tsx,
            },
            Asset {
                path: "/lib/components/font-input.css",
                content: include_bytes!("../web/lib/components/font-input.css"),
                content_type: ContentType::Css,
            },
            Asset {
                path: "/lib/components/number-input.tsx",
                content: include_bytes!("../web/lib/components/number-input.tsx"),
                content_type: ContentType::Tsx,
            },
            Asset {
                path: "/lib/components/number-input.css",
                content: include_bytes!("../web/lib/components/number-input.css"),
                content_type: ContentType::Css,
            },
            Asset {
                path: "/lib/components/pane.tsx",
                content: include_bytes!("../web/lib/components/pane.tsx"),
                content_type: ContentType::Tsx,
            },
            Asset {
                path: "/lib/components/pane.css",
                content: include_bytes!("../web/lib/components/pane.css"),
                content_type: ContentType::Css,
            },
            Asset {
                path: "/lib/components/popover.tsx",
                content: include_bytes!("../web/lib/components/popover.tsx"),
                content_type: ContentType::Tsx,
            },
            Asset {
                path: "/lib/components/popover.css",
                content: include_bytes!("../web/lib/components/popover.css"),
                content_type: ContentType::Css,
            },
            Asset {
                path: "/lib/components/popover-select.tsx",
                content: include_bytes!("../web/lib/components/popover-select.tsx"),
                content_type: ContentType::Tsx,
            },
            Asset {
                path: "/lib/components/popover-select.css",
                content: include_bytes!("../web/lib/components/popover-select.css"),
                content_type: ContentType::Css,
            },
            Asset {
                path: "/lib/components/root.tsx",
                content: include_bytes!("../web/lib/components/root.tsx"),
                content_type: ContentType::Tsx,
            },
            Asset {
                path: "/lib/components/root.css",
                content: include_bytes!("../web/lib/components/root.css"),
                content_type: ContentType::Css,
            },
            Asset {
                path: "/lib/components/sidebar.tsx",
                content: include_bytes!("../web/lib/components/sidebar.tsx"),
                content_type: ContentType::Tsx,
            },
            Asset {
                path: "/lib/components/sidebar.css",
                content: include_bytes!("../web/lib/components/sidebar.css"),
                content_type: ContentType::Css,
            },
            Asset {
                path: "/lib/components/stack.tsx",
                content: include_bytes!("../web/lib/components/stack.tsx"),
                content_type: ContentType::Tsx,
            },
            Asset {
                path: "/lib/components/swatch.tsx",
                content: include_bytes!("../web/lib/components/swatch.tsx"),
                content_type: ContentType::Tsx,
            },
            Asset {
                path: "/lib/components/swatch.css",
                content: include_bytes!("../web/lib/components/swatch.css"),
                content_type: ContentType::Css,
            },
            Asset {
                path: "/lib/components/text.tsx",
                content: include_bytes!("../web/lib/components/text.tsx"),
                content_type: ContentType::Tsx,
            },
            Asset {
                path: "/lib/components/text.css",
                content: include_bytes!("../web/lib/components/text.css"),
                content_type: ContentType::Css,
            },
            Asset {
                path: "/lib/components/text-input.tsx",
                content: include_bytes!("../web/lib/components/text-input.tsx"),
                content_type: ContentType::Tsx,
            },
            Asset {
                path: "/lib/components/text-input.css",
                content: include_bytes!("../web/lib/components/text-input.css"),
                content_type: ContentType::Css,
            },
            Asset {
                path: "/lib/components/token-value-editor.tsx",
                content: include_bytes!("../web/lib/components/token-value-editor.tsx"),
                content_type: ContentType::Tsx,
            },
        ],
        dirs: &[
            DirMount {
                url_prefix: "/vendor/remix-ui/",
                dir: &REMIX_UI_DIR,
            },
            DirMount {
                url_prefix: "/vendor/pretext/",
                dir: &PRETEXT_DIR,
            },
        ],
    };
    &PLATFORM
}

/// Build an AssetBundle from explicit file entries and optional directory
/// mounts.
///
/// Usage:
/// ```ignore
/// static REMIX_UI_DIR: include_dir::Dir<'_> =
///     include_dir::include_dir!("$CARGO_MANIFEST_DIR/web/vendor/remix-ui");
///
/// let assets = sola_kit::asset_bundle! {
///     "/index.html" => (include_bytes!("../web/index.html"), Html),
///     "/src/app.ts" => (include_bytes!("../web/src/app.ts"), TypeScript),
///     @dir "/vendor/remix-ui/" => &REMIX_UI_DIR,
/// };
/// ```
///
/// Every entry — both file entries and `@dir` clauses — must end in a
/// trailing comma. (Macro grammar: each repetition is terminated by `,`,
/// not separated; mixing the two forms gets ambiguous otherwise.)
#[macro_export]
macro_rules! asset_bundle {
    (
        $( $path:literal => ($content:expr, $ctype:ident), )*
        $( @dir $prefix:literal => $dir:expr, )*
    ) => {
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
            dirs: &[
                $(
                    $crate::DirMount {
                        url_prefix: $prefix,
                        dir: $dir,
                    },
                )*
            ],
        }
    };
}
