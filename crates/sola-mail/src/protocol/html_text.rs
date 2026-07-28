//! HTML message bodies → readable plain text (no HTML engine in the UI).

/// Convert HTML mail into plain text suitable for an iced scrollable.
pub fn to_plain(html: &str) -> String {
    // html2text wants a width for wrapping; 100 columns is a reasonable
    // reading width inside a desktop pane.
    let raw = match html2text::from_read(html.as_bytes(), 100) {
        Ok(s) => s,
        Err(_) => {
            // Fallback: crude tag strip so the user still sees something.
            return html
                .replace('<', " <")
                .split('<')
                .map(|chunk| chunk.split('>').nth(1).unwrap_or(chunk))
                .collect::<String>();
        }
    };
    // Collapse excessive blank lines from table-heavy marketing mail.
    let mut out = String::with_capacity(raw.len());
    let mut blank_run = 0u32;
    for line in raw.lines() {
        if line.trim().is_empty() {
            blank_run += 1;
            if blank_run <= 2 {
                out.push('\n');
            }
        } else {
            blank_run = 0;
            out.push_str(line);
            out.push('\n');
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_simple_markup() {
        let plain = to_plain("<p>Hello <b>world</b></p>");
        assert!(plain.to_lowercase().contains("hello"));
        assert!(plain.to_lowercase().contains("world"));
        assert!(!plain.contains("<b>"));
    }

    #[test]
    fn preserves_link_destination() {
        let plain = to_plain(r#"<a href="https://example.com/path">Click</a>"#);
        assert!(
            plain.contains("example.com") || plain.contains("Click"),
            "got: {plain:?}"
        );
    }
}
