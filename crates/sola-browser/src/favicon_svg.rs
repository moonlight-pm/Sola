//! SVG favicons. CEF's image download decodes rasters only, so a site
//! whose favicon is an SVG (Wicket's `mark.svg`) came back empty and kept
//! the globe. Such a favicon is fetched here and drawn to a PNG.

/// The longest side of a drawn favicon (px).
const SIDE: u32 = 64;
/// The largest SVG fetched.
const MAX_BYTES: usize = 256 * 1024;

/// Is `url` an SVG by its path (query and fragment aside)?
pub fn is_svg(url: &str) -> bool {
    let path = url.split(['?', '#']).next().unwrap_or(url);
    path.to_ascii_lowercase().ends_with(".svg")
}

/// Draw `svg` to a PNG, its longest side [`SIDE`].
pub fn png(svg: &[u8]) -> Option<Vec<u8>> {
    let tree = resvg::usvg::Tree::from_data(svg, &resvg::usvg::Options::default()).ok()?;
    let size = tree.size();
    let scale = SIDE as f32 / size.width().max(size.height());
    let w = ((size.width() * scale).round() as u32).max(1);
    let h = ((size.height() * scale).round() as u32).max(1);
    let mut pixmap = resvg::tiny_skia::Pixmap::new(w, h)?;
    resvg::render(&tree, resvg::tiny_skia::Transform::from_scale(scale, scale), &mut pixmap.as_mut());
    pixmap.encode_png().ok()
}

/// Fetch `url` and draw it, off the calling thread; `done` gets the PNG.
pub fn fetch(url: String, done: impl FnOnce(Vec<u8>) + Send + 'static) {
    std::thread::spawn(move || {
        let Ok(rt) = tokio::runtime::Builder::new_current_thread().enable_all().build() else {
            return;
        };
        let bytes = rt.block_on(async {
            let resp = reqwest::get(&url).await.ok()?.error_for_status().ok()?;
            let body = resp.bytes().await.ok()?;
            (body.len() <= MAX_BYTES).then(|| body.to_vec())
        });
        match bytes.as_deref().and_then(png) {
            Some(png) => done(png),
            None => tracing::warn!(%url, "favicon: could not draw the svg"),
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_svg_favicon_draws_to_a_png() {
        let svg = br##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 16 8"><rect width="16" height="8" fill="#f80"/></svg>"##;
        let out = png(svg).expect("drawn");
        assert_eq!(&out[1..4], b"PNG");
        assert!(is_svg("https://moonlight.pm/admin/static/ui/mark.svg?v=e3"));
        assert!(!is_svg("https://a/favicon.ico"));
    }
}
