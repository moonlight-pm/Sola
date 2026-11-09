//! Every file the site is: the pages from `pages/*.html`, the manual, and the
//! files in `static/`.

use crate::manual;

pub struct Site {
    resources: Vec<(String, Vec<u8>)>,
    missing: Vec<u8>,
}

const SHELL: &str = include_str!("../pages/page.html");
const HOME: &str = include_str!("../pages/home.html");
const MANUAL: &str = include_str!("../pages/manual.html");
const PRIVACY: &str = include_str!("../pages/privacy.html");
const MISSING: &str = include_str!("../pages/missing.html");

/// Files copied as they are. `_headers` is Cloudflare's, and sets the security
/// and caching headers.
const STATIC: &[(&str, &[u8])] = &[
    ("/_headers", include_bytes!("../static/_headers")),
    ("/style.css", include_bytes!("../static/style.css")),
    ("/flower.svg", include_bytes!("../static/flower.svg")),
    ("/desk.webp", include_bytes!("../static/desk.webp")),
];

impl Site {
    pub fn build() -> Site {
        let m = manual::render();
        let manual = fill(MANUAL, &[("contents", &m.contents), ("body", &m.body)]);

        let mut resources = vec![
            ("/".to_string(), page("Sola", "", HOME)),
            (
                "/manual".to_string(),
                page("Manual · Sola", "manual", &manual),
            ),
            ("/privacy".to_string(), page("Privacy · Sola", "", PRIVACY)),
        ];
        for (path, body) in STATIC {
            resources.push((path.to_string(), body.to_vec()));
        }
        Site {
            resources,
            missing: page("Not found · Sola", "", MISSING),
        }
    }

    /// Each file by its place under the output directory. Cloudflare serves
    /// `manual.html` at `/manual`, and `404.html` for a path it does not have.
    pub fn files(&self) -> Vec<(String, &[u8])> {
        let mut files: Vec<(String, &[u8])> = self
            .resources
            .iter()
            .map(|(path, body)| {
                let file = match path.as_str() {
                    "/" => "index.html".to_string(),
                    "/manual" | "/privacy" => format!("{}.html", &path[1..]),
                    other => other.trim_start_matches('/').to_string(),
                };
                (file, body.as_slice())
            })
            .collect();
        files.push(("404.html".to_string(), self.missing.as_slice()));
        files
    }

    #[cfg(test)]
    fn get(&self, path: &str) -> Option<&[u8]> {
        self.resources
            .iter()
            .find(|(p, _)| p == path)
            .map(|(_, body)| body.as_slice())
    }
}

fn page(title: &str, current: &str, body: &str) -> Vec<u8> {
    let manual_current = if current == "manual" {
        " class=\"current\""
    } else {
        ""
    };
    fill(
        SHELL,
        &[
            ("title", title),
            ("manual_current", manual_current),
            ("body", body),
        ],
    )
    .into_bytes()
}

/// Put each `{{name}}` in a template. A name left over is a bug in the
/// template.
fn fill(template: &str, values: &[(&str, &str)]) -> String {
    let mut out = template.to_string();
    for (name, value) in values {
        out = out.replace(&format!("{{{{{name}}}}}"), value);
    }
    out
}

pub fn escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            c => out.push(c),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text(site: &Site, path: &str) -> String {
        String::from_utf8_lossy(site.get(path).unwrap()).to_string()
    }

    #[test]
    fn every_placeholder_is_filled() {
        let site = Site::build();
        for (path, body) in site.files() {
            if path.ends_with(".html") {
                let text = String::from_utf8_lossy(body);
                assert!(!text.contains("{{"), "{path} has an unfilled placeholder");
            }
        }
    }

    #[test]
    fn files_are_what_cloudflare_serves() {
        let site = Site::build();
        let names: Vec<String> = site.files().into_iter().map(|(name, _)| name).collect();
        for wanted in [
            "index.html",
            "manual.html",
            "privacy.html",
            "404.html",
            "_headers",
            "style.css",
        ] {
            assert!(names.iter().any(|name| name == wanted), "no {wanted}");
        }
        assert!(names.iter().all(|name| !name.starts_with('/')));
    }

    #[test]
    fn links_land_on_the_site() {
        let site = Site::build();
        let manual = text(&site, "/manual");
        for path in ["/", "/privacy"] {
            let page = text(&site, path);
            for href in page.split("href=\"").skip(1) {
                let dest = href.split('"').next().unwrap();
                if let Some(id) = dest.strip_prefix("/manual#") {
                    assert!(
                        manual.contains(&format!("id=\"{id}\"")),
                        "/manual#{id} has no target"
                    );
                } else if let Some(id) = dest.strip_prefix('#') {
                    assert!(
                        page.contains(&format!("id=\"{id}\"")),
                        "#{id} has no target"
                    );
                } else if dest.starts_with('/') {
                    assert!(site.get(dest).is_some(), "{dest} is not served");
                } else {
                    assert!(dest.starts_with("https://"), "{dest} leads nowhere");
                }
            }
        }
    }
}
