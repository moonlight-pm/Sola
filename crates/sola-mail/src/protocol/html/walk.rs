//! Letter HTML → prose blocks: a safe subset, drawn by the kit.
//!
//! Only what is understood is drawn: text with CSS whitespace collapsing,
//! bold / italic / code, links (http, https, mailto), headings, quotes,
//! lists, preformatted text, rules, and pictures. Nothing runs: scripts,
//! styles, forms, frames and embeds are dropped with their contents, and
//! no stylesheet is read. Inline `style` is read only to drop hidden
//! elements (preheaders), for bold / italic, and for a zero font size,
//! which hides text the way CSS does: inherited, until an element sets a
//! size of its own (layout cells zero it to close gaps between pictures). Tables are read as rows:
//! a row of short cells stays one line, anything else stacks its cells.

use sola_kit::components::prose::{ProseBlock, ProseImage, ProseRun};

use super::dom::{Data, Handle};

/// Where a picture's pixels come from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ImageSource {
    /// A part of this message (`cid:` reference, without `<>`).
    Cid(String),
    /// The web: shown only after the reader asks.
    Remote(String),
    /// A `data:` URI, already decoded.
    Data(Vec<u8>),
}

/// The walked letter: blocks, and the pictures they refer to by key.
#[derive(Debug, Default)]
pub struct Walked {
    pub blocks: Vec<ProseBlock>,
    pub images: Vec<(String, ImageSource)>,
}

/// Deeper than this is not a letter; stop descending.
const MAX_DEPTH: usize = 200;
/// A row of cells this short (in chars, all inline) reads as one line.
const ROW_LINE_MAX: usize = 160;
/// Pictures this small (both sides, CSS px) are icons: their alt text
/// stands in for them in the line.
const ICON_MAX: u32 = 32;
/// Between the cells of a row read as one line.
const CELL_GAP: &str = "\u{2003}";

pub fn walk(document: &Handle) -> Walked {
    let mut w = Walker::default();
    w.children(document, 0);
    w.finish()
}

#[derive(Default)]
struct Walker {
    blocks: Vec<ProseBlock>,
    runs: Vec<ProseRun>,
    /// Collapsed whitespace waiting for the next visible character, in
    /// the style it had where it was written (a space before a link is
    /// not part of the link).
    pending_space: Option<Style>,
    bold: u32,
    italic: u32,
    code: u32,
    link: Option<String>,
    quote: u32,
    heading: Option<u8>,
    lists: Vec<ListFrame>,
    images: Vec<(String, ImageSource)>,
    data_images: usize,
    /// The last block is a line of pictures and nothing has come between:
    /// the next picture joins it (inline pictures sit side by side).
    joinable: bool,
    /// The text here is zero-sized (`font-size: 0`, inherited): not shown.
    /// Pictures still are.
    mute: bool,
}

/// Bold, italic, code, link: what a run carries.
#[derive(Debug, Clone, PartialEq, Default)]
struct Style {
    bold: bool,
    italic: bool,
    code: bool,
    link: Option<String>,
}

struct ListFrame {
    ordered: bool,
    items: Vec<Vec<ProseRun>>,
    /// Inline content belongs to an open `<li>`.
    in_item: bool,
}

impl Walker {
    /// A walker for a table cell: same quote and link context, own blocks.
    fn sub(&self) -> Self {
        Self {
            quote: self.quote,
            link: self.link.clone(),
            bold: self.bold,
            italic: self.italic,
            data_images: self.data_images,
            mute: self.mute,
            ..Self::default()
        }
    }

    fn finish(mut self) -> Walked {
        self.end_block();
        while let Some(frame) = self.lists.pop() {
            self.push_list(frame);
        }
        Walked {
            blocks: self.blocks,
            images: self.images,
        }
    }

    fn children(&mut self, node: &Handle, depth: usize) {
        if depth > MAX_DEPTH {
            return;
        }
        let kids = node.children.borrow().clone();
        for child in &kids {
            self.node(child, depth + 1);
        }
    }

    fn node(&mut self, node: &Handle, depth: usize) {
        match &node.data {
            Data::Document => self.children(node, depth),
            Data::Text(t) => {
                let text = t.borrow().clone();
                self.text(&text);
            }
            Data::Other => {}
            Data::Element { .. } => {
                let outer = self.mute;
                self.mute = zero_font(node).unwrap_or(outer);
                self.element(node, depth);
                self.mute = outer;
            }
        }
    }

    fn element(&mut self, node: &Handle, depth: usize) {
        let Some(tag) = node.tag() else {
            return;
        };
        let tag = tag.as_ref().to_ascii_lowercase();
        if SKIPPED.contains(&tag.as_str()) || hidden(node) {
            return;
        }
        match tag.as_str() {
            "br" => self.line_break(),
            "hr" => {
                self.end_block();
                self.blocks.push(ProseBlock::Rule);
            }
            "img" => self.image(node),
            "pre" => {
                self.end_block();
                let mut raw = String::new();
                raw_text(node, &mut raw);
                let raw = raw.trim_matches('\n').to_string();
                if !raw.trim().is_empty() {
                    self.blocks.push(ProseBlock::Code {
                        text: raw,
                        verse: false,
                    });
                }
            }
            "h1" | "h2" | "h3" | "h4" | "h5" | "h6" => {
                let level = tag.as_bytes()[1] - b'0';
                if self.heading.is_some() || self.in_item() {
                    self.children(node, depth);
                    return;
                }
                self.end_block();
                self.heading = Some(level);
                self.children(node, depth);
                self.end_block();
                self.heading = None;
            }
            "blockquote" => {
                self.end_block();
                self.quote += 1;
                self.children(node, depth);
                self.end_block();
                self.quote -= 1;
            }
            "ul" | "ol" => self.list(node, tag == "ol", depth),
            "li" => self.item(node, depth),
            "table" => self.table(node, depth),
            "a" => {
                let href = node.attr("href").and_then(|h| safe_href(&h));
                match href {
                    Some(url) => {
                        let outer = self.link.replace(url);
                        self.children(node, depth);
                        self.link = outer;
                    }
                    None => self.children(node, depth),
                }
            }
            "q" => {
                self.text("“");
                self.children(node, depth);
                self.text("”");
            }
            _ => {
                // CSS defaults: a paragraph has margins (a block of its
                // own); a div does not (a new line). Outlook's MsoNormal
                // paragraphs have theirs zeroed by its stylesheet.
                let outlook_line = tag == "p"
                    && node
                        .attr("class")
                        .is_some_and(|c| c.split_whitespace().any(|c| c.eq_ignore_ascii_case("MsoNormal")));
                let block = PARAGRAPHS.contains(&tag.as_str()) && !outlook_line;
                let line = !block && (LINES.contains(&tag.as_str()) || outlook_line);
                if block {
                    self.end_block();
                }
                if line {
                    self.new_line();
                }
                if block || line {
                    self.joinable = false;
                }
                let (b, i, c) = inline_style(&tag, node);
                self.bold += b as u32;
                self.italic += i as u32;
                self.code += c as u32;
                self.children(node, depth);
                self.bold -= b as u32;
                self.italic -= i as u32;
                self.code -= c as u32;
                if block {
                    self.end_block();
                }
                if line {
                    self.new_line();
                }
                if block || line {
                    self.joinable = false;
                }
            }
        }
    }

    // ── Inline content ────────────────────────────────────────────────

    fn text(&mut self, text: &str) {
        if self.mute {
            return;
        }
        for ch in text.chars() {
            if INVISIBLE.contains(&ch) {
                continue;
            }
            // A no-break space is not collapsed: it holds a line open
            // (Outlook's blank line is a paragraph of one).
            if ch == '\u{a0}' {
                if let Some(style) = self.pending_space.take() {
                    self.push_styled(' ', style);
                }
                self.push(' ');
                continue;
            }
            if ch.is_whitespace() {
                // CSS `white-space: normal`: a run of whitespace is one
                // space, and none at the start of a line.
                self.space();
                continue;
            }
            if let Some(style) = self.pending_space.take() {
                self.push_styled(' ', style);
            }
            self.push(ch);
        }
    }

    /// Whitespace here: one space before the next character, unless the
    /// line has just begun (or already ends in one).
    fn space(&mut self) {
        if self.pending_space.is_none() && !self.at_line_start() {
            self.pending_space = Some(self.style());
        }
    }

    fn style(&self) -> Style {
        Style {
            bold: self.bold > 0,
            italic: self.italic > 0,
            code: self.code > 0,
            link: self.link.clone(),
        }
    }

    fn line_break(&mut self) {
        self.pending_space = None;
        if !self.runs.is_empty() {
            self.push('\n');
        }
    }

    /// A block without margins begins or ends: start a new line unless
    /// one has just begun.
    fn new_line(&mut self) {
        self.pending_space = None;
        let fresh = self
            .runs
            .last()
            .and_then(|r| r.text.chars().last())
            .is_none_or(|c| c == '\n');
        if !fresh {
            self.push('\n');
        }
    }

    fn at_line_start(&self) -> bool {
        self.runs
            .last()
            .and_then(|r| r.text.chars().last())
            .is_none_or(|c| c == '\n' || c == ' ')
    }

    fn push(&mut self, ch: char) {
        let style = self.style();
        self.push_styled(ch, style);
    }

    fn push_styled(&mut self, ch: char, style: Style) {
        self.joinable = false;
        if let Some(last) = self.runs.last_mut()
            && last.url == style.link
            && last.bold == style.bold
            && last.italic == style.italic
            && last.code == style.code
        {
            last.text.push(ch);
            return;
        }
        self.runs.push(ProseRun {
            text: ch.to_string(),
            url: style.link,
            bold: style.bold,
            italic: style.italic,
            code: style.code,
        });
    }

    // ── Blocks ────────────────────────────────────────────────────────

    fn in_item(&self) -> bool {
        self.lists.last().is_some_and(|f| f.in_item)
    }

    /// A block boundary: the line so far becomes a paragraph (a quote, a
    /// heading), or inside a list item a new line of it.
    fn end_block(&mut self) {
        self.pending_space = None;
        if self.in_item() {
            self.line_break();
            return;
        }
        let runs = tidy(std::mem::take(&mut self.runs));
        if runs.is_empty() {
            return;
        }
        let block = match self.heading {
            Some(level) => ProseBlock::Heading { level, runs },
            None if self.quote > 0 => ProseBlock::Quote(runs),
            None => ProseBlock::Paragraph(runs),
        };
        self.blocks.push(block);
    }

    fn list(&mut self, node: &Handle, ordered: bool, depth: usize) {
        // A list inside an item: what the outer list has so far is one
        // block, the inner list its own, and the outer goes on after it.
        if let Some(outer) = self.lists.last_mut()
            && outer.in_item
        {
            let item = tidy(std::mem::take(&mut self.runs));
            if !item.is_empty() {
                outer.items.push(item);
            }
            let items = std::mem::take(&mut outer.items);
            let ordered_outer = outer.ordered;
            outer.in_item = false;
            if !items.is_empty() {
                self.blocks.push(ProseBlock::List {
                    ordered: ordered_outer,
                    items,
                });
            }
        } else {
            self.end_block();
        }
        self.lists.push(ListFrame {
            ordered,
            items: Vec::new(),
            in_item: false,
        });
        self.children(node, depth);
        if let Some(frame) = self.lists.pop() {
            self.push_list(frame);
        }
    }

    fn push_list(&mut self, mut frame: ListFrame) {
        if frame.in_item {
            let item = tidy(std::mem::take(&mut self.runs));
            if !item.is_empty() {
                frame.items.push(item);
            }
        }
        if !frame.items.is_empty() {
            self.blocks.push(ProseBlock::List {
                ordered: frame.ordered,
                items: frame.items,
            });
        }
    }

    fn item(&mut self, node: &Handle, depth: usize) {
        if self.lists.is_empty() {
            // A stray <li>: a paragraph.
            self.end_block();
            self.children(node, depth);
            self.end_block();
            return;
        }
        self.pending_space = None;
        if let Some(frame) = self.lists.last_mut() {
            frame.in_item = true;
        }
        self.children(node, depth);
        let item = tidy(std::mem::take(&mut self.runs));
        if let Some(frame) = self.lists.last_mut() {
            if !item.is_empty() {
                frame.items.push(item);
            }
            frame.in_item = false;
        }
        self.pending_space = None;
    }

    /// Rows, top to bottom. A row of short inline cells stays one line;
    /// otherwise its cells stack. Nested tables walk the same way.
    fn table(&mut self, node: &Handle, depth: usize) {
        self.end_block();
        let mut rows = Vec::new();
        collect_rows(node, &mut rows, 0);
        for row in rows {
            let row_mute = zero_font(&row).unwrap_or(self.mute);
            let mut cells = Vec::new();
            for cell in row.children.borrow().iter() {
                let Some(tag) = cell.tag() else {
                    continue;
                };
                if !matches!(tag.as_ref(), "td" | "th") || hidden(cell) {
                    continue;
                }
                let mut sub = self.sub();
                sub.mute = zero_font(cell).unwrap_or(row_mute);
                // A header cell is bold unless its style says otherwise
                // (layouts use `th` columns set to `font-weight: normal`).
                let (b, i, _) = inline_style(tag.as_ref(), cell);
                sub.bold += b as u32;
                sub.italic += i as u32;
                sub.children(cell, depth + 1);
                let walked = sub.finish();
                self.data_images += walked
                    .images
                    .iter()
                    .filter(|(_, s)| matches!(s, ImageSource::Data(_)))
                    .count();
                self.images.extend(walked.images);
                if !walked.blocks.is_empty() {
                    cells.push(walked.blocks);
                }
            }
            if let Some(line) = row_as_line(&cells).or_else(|| row_of_pictures(&cells)) {
                self.blocks.push(line);
            } else {
                for blocks in cells {
                    self.blocks.extend(blocks);
                }
            }
        }
    }

    fn image(&mut self, node: &Handle) {
        let Some(src) = node.attr("src").map(|s| s.trim().to_string()) else {
            return;
        };
        // `max-width` caps the width (or stands in for it).
        let width = match (dimension(node, "width"), dimension(node, "max-width")) {
            (Some(w), Some(m)) => Some(w.min(m)),
            (w, m) => w.or(m),
        };
        let height = dimension(node, "height");
        // Tracking pixels and spacers.
        if width.is_some_and(|w| w <= 2) || height.is_some_and(|h| h <= 2) {
            return;
        }
        let alt = node
            .attr("alt")
            .map(|a| a.split_whitespace().collect::<Vec<_>>().join(" "))
            .unwrap_or_default();
        let icon = width.is_some_and(|w| w <= ICON_MAX) && height.is_some_and(|h| h <= ICON_MAX);
        let source = image_source(&src);
        if icon || source.is_none() || self.in_item() || self.heading.is_some() {
            if !alt.is_empty() {
                self.space();
                self.text(&alt);
                self.space();
            }
            return;
        }
        let Some(source) = source else {
            return;
        };
        let key = match &source {
            ImageSource::Cid(id) => format!("cid:{id}"),
            ImageSource::Remote(url) => url.clone(),
            ImageSource::Data(_) => {
                self.data_images += 1;
                format!("data:{}", self.data_images)
            }
        };
        self.end_block();
        if !self.images.iter().any(|(k, _)| *k == key) {
            self.images.push((key.clone(), source));
        }
        let picture = ProseImage {
            key,
            alt,
            width,
            height,
            url: self.link.clone(),
        };
        match self.blocks.last_mut() {
            Some(ProseBlock::Images(line)) if self.joinable => line.push(picture),
            _ => self.blocks.push(ProseBlock::Images(vec![picture])),
        }
        self.joinable = true;
    }
}

/// Elements dropped with everything inside them.
const SKIPPED: &[&str] = &[
    "head", "title", "style", "script", "noscript", "template", "iframe", "object", "embed",
    "frame", "frameset", "svg", "math", "canvas", "video", "audio", "input", "select",
    "textarea", "option", "map", "area", "meta", "link", "base", "param", "source", "track",
];

/// Elements with margins: each is a block of its own.
const PARAGRAPHS: &[&str] = &["p", "address", "figure", "form", "fieldset", "dl", "details"];

/// Elements without margins: each begins and ends a line.
const LINES: &[&str] = &[
    "div", "section", "article", "header", "footer", "main", "nav", "aside", "center",
    "figcaption", "summary", "dt", "dd", "caption", "body", "html", "tr", "td", "th", "tbody",
    "thead", "tfoot", "hgroup", "dialog", "legend",
];

/// Characters with no width that newsletters pad preheaders with.
const INVISIBLE: &[char] = &[
    '\u{200b}', '\u{200c}', '\u{200d}', '\u{2060}', '\u{feff}', '\u{34f}', '\u{ad}',
];

/// Bold / italic / code from the tag and its inline style.
fn inline_style(tag: &str, node: &Handle) -> (bool, bool, bool) {
    let mut bold = matches!(tag, "b" | "strong" | "th");
    let mut italic = matches!(tag, "i" | "em" | "cite" | "var" | "dfn");
    let code = matches!(tag, "code" | "tt" | "kbd" | "samp");
    if let Some(style) = node.attr("style") {
        let style = squash(&style);
        if let Some(weight) = property(&style, "font-weight") {
            bold = weight == "bold"
                || weight == "bolder"
                || weight.parse::<u32>().is_ok_and(|w| w >= 600);
        }
        if property(&style, "font-style").is_some_and(|s| s == "italic" || s == "oblique") {
            italic = true;
        }
    }
    (bold, italic, code)
}

/// Not shown by the sender: `hidden`, `display:none`, zero height, width
/// or opacity, `mso-hide:all` (the preheader idiom). A zero font size is
/// not here: it hides only text that does not set its own ([`zero_font`]).
fn hidden(node: &Handle) -> bool {
    if node.attr("hidden").is_some() {
        return true;
    }
    let Some(style) = node.attr("style") else {
        return false;
    };
    let style = squash(&style);
    property(&style, "display").is_some_and(|v| v == "none")
        || property(&style, "visibility").is_some_and(|v| v == "hidden")
        || property(&style, "mso-hide").is_some_and(|v| v == "all")
        || ["max-height", "opacity", "max-width"]
            .iter()
            .any(|p| property(&style, p).is_some_and(is_zero))
}

/// Whether the `font-size` this element sets is zero; `None` when it sets
/// none (it inherits).
fn zero_font(node: &Handle) -> Option<bool> {
    let style = squash(&node.attr("style")?);
    let size = property(&style, "font-size")?;
    (!matches!(size, "inherit" | "unset")).then(|| is_zero(size))
}

/// Lowercase with the whitespace removed, `!important` dropped.
fn squash(style: &str) -> String {
    style
        .to_ascii_lowercase()
        .replace("!important", "")
        .chars()
        .filter(|c| !c.is_whitespace())
        .collect()
}

/// A declaration's value in a squashed style (the last one wins).
fn property<'a>(style: &'a str, name: &str) -> Option<&'a str> {
    style.split(';').rev().find_map(|decl| {
        let (k, v) = decl.split_once(':')?;
        (k == name).then_some(v)
    })
}

fn is_zero(value: &str) -> bool {
    let digits = value.trim_end_matches(|c: char| c.is_ascii_alphabetic() || c == '%');
    digits.parse::<f32>().is_ok_and(|n| n == 0.0)
}

/// `width="600"`, `width="600px"`, or `style="width:600px"`; not `%`.
fn dimension(node: &Handle, name: &str) -> Option<u32> {
    let from_style = node
        .attr("style")
        .and_then(|s| property(&squash(&s), name).map(str::to_string));
    let raw = from_style.or_else(|| node.attr(name))?;
    let raw = raw.trim();
    if raw.ends_with('%') {
        return None;
    }
    let digits = raw.trim_end_matches("px");
    digits.parse::<f32>().ok().map(|n| n.round().max(0.0) as u32)
}

fn image_source(src: &str) -> Option<ImageSource> {
    let lower = src.to_ascii_lowercase();
    if let Some(id) = lower.strip_prefix("cid:").map(|_| &src[4..]) {
        let id = id.trim().trim_start_matches('<').trim_end_matches('>');
        return (!id.is_empty()).then(|| ImageSource::Cid(id.to_string()));
    }
    if lower.starts_with("https://") || lower.starts_with("http://") {
        return Some(ImageSource::Remote(src.to_string()));
    }
    if lower.starts_with("data:image/") {
        let (meta, payload) = src.split_once(',')?;
        if !meta.to_ascii_lowercase().ends_with(";base64") {
            return None;
        }
        use base64::Engine;
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(payload.trim())
            .ok()?;
        return Some(ImageSource::Data(bytes));
    }
    None
}

/// Links a reader can follow: the web and mail. Never `javascript:`,
/// `data:` or `file:`.
fn safe_href(href: &str) -> Option<String> {
    let href = href.trim();
    let lower = href.to_ascii_lowercase();
    let ok = (lower.starts_with("https://") || lower.starts_with("http://")) && href.len() > 10
        || lower.starts_with("mailto:") && href.len() > 7;
    ok.then(|| href.to_string())
}

/// `pre`'s text exactly as written (`<br>` as a newline).
fn raw_text(node: &Handle, out: &mut String) {
    for child in node.children.borrow().iter() {
        match &child.data {
            Data::Text(t) => out.push_str(&t.borrow()),
            Data::Element { .. } => {
                if child.tag().is_some_and(|t| t.as_ref() == "br") {
                    out.push('\n');
                } else if !hidden(child) {
                    raw_text(child, out);
                }
            }
            _ => {}
        }
    }
}

/// A table's rows in order (through `thead` / `tbody` / `tfoot`), not
/// descending into cells (their own tables walk on their own).
fn collect_rows(node: &Handle, rows: &mut Vec<Handle>, depth: usize) {
    if depth > 4 {
        return;
    }
    for child in node.children.borrow().iter() {
        match child.tag().map(|t| t.as_ref().to_string()).as_deref() {
            Some("tr") => rows.push(child.clone()),
            Some("thead" | "tbody" | "tfoot") => collect_rows(child, rows, depth + 1),
            _ => {}
        }
    }
}

/// Several short, inline cells as one line, the cells apart.
fn row_as_line(cells: &[Vec<ProseBlock>]) -> Option<ProseBlock> {
    if cells.len() < 2 {
        return None;
    }
    let mut total = 0;
    let mut runs: Vec<ProseRun> = Vec::new();
    for (i, blocks) in cells.iter().enumerate() {
        let [ProseBlock::Paragraph(cell)] = blocks.as_slice() else {
            return None;
        };
        let len: usize = cell.iter().map(|r| r.text.chars().count()).sum();
        if cell.iter().any(|r| r.text.contains('\n')) {
            return None;
        }
        total += len;
        if total > ROW_LINE_MAX {
            return None;
        }
        if i > 0 {
            runs.push(ProseRun::text(CELL_GAP));
        }
        runs.extend(cell.iter().cloned());
    }
    Some(ProseBlock::Paragraph(runs))
}

/// Cells holding only pictures, as one line of them.
fn row_of_pictures(cells: &[Vec<ProseBlock>]) -> Option<ProseBlock> {
    if cells.len() < 2 {
        return None;
    }
    let mut line = Vec::new();
    for blocks in cells {
        let [ProseBlock::Images(pictures)] = blocks.as_slice() else {
            return None;
        };
        line.extend(pictures.iter().cloned());
    }
    Some(ProseBlock::Images(line))
}

/// Trim a block's runs at both ends (spaces, line breaks); drop runs left
/// empty. Nothing visible → nothing.
fn tidy(mut runs: Vec<ProseRun>) -> Vec<ProseRun> {
    while let Some(first) = runs.first_mut() {
        let t = first.text.trim_start_matches([' ', '\n']).to_string();
        if t.is_empty() {
            runs.remove(0);
        } else {
            first.text = t;
            break;
        }
    }
    while let Some(last) = runs.last_mut() {
        let t = last.text.trim_end_matches([' ', '\n']).to_string();
        if t.is_empty() {
            runs.pop();
        } else {
            last.text = t;
            break;
        }
    }
    if runs.iter().all(|r| r.text.trim().is_empty()) {
        return Vec::new();
    }
    runs
}

#[cfg(test)]
mod tests {
    use super::super::dom::parse;
    use super::*;

    fn blocks(html: &str) -> Vec<ProseBlock> {
        walk(&parse(html)).blocks
    }

    fn texts(html: &str) -> Vec<String> {
        blocks(html)
            .iter()
            .map(|b| match b {
                ProseBlock::Paragraph(r) | ProseBlock::Quote(r) => {
                    r.iter().map(|r| r.text.as_str()).collect()
                }
                ProseBlock::Heading { runs, .. } => {
                    format!("# {}", runs.iter().map(|r| r.text.as_str()).collect::<String>())
                }
                ProseBlock::List { items, .. } => items
                    .iter()
                    .map(|i| format!("- {}", i.iter().map(|r| r.text.as_str()).collect::<String>()))
                    .collect::<Vec<_>>()
                    .join(" "),
                ProseBlock::Code { text, .. } => format!("```{text}```"),
                ProseBlock::Rule => "---".into(),
                ProseBlock::Images(v) => v
                    .iter()
                    .map(|i| format!("[img {}]", i.key))
                    .collect::<Vec<_>>()
                    .join(" "),
            })
            .collect()
    }

    #[test]
    fn whitespace_between_inline_elements_is_a_space() {
        // The old converter dropped these: "APIplatform", "TwitterFacebook".
        assert_eq!(
            texts("<p>the Rainforest <a href=\"https://x.example/a\">API</a>\n platform</p>"),
            ["the Rainforest API platform"]
        );
        assert_eq!(
            texts(
                "<p><a href=\"https://t.example\">Twitter</a>\n<a href=\"https://f.example\">Facebook</a></p>"
            ),
            ["Twitter Facebook"]
        );
        assert_eq!(texts("<p>up to<span>\n</span>125 characters</p>"), ["up to 125 characters"]);
        assert_eq!(texts("<p>  lots   of\n\t space  </p>"), ["lots of space"]);
    }

    #[test]
    fn divs_are_lines_paragraphs_are_blocks() {
        // Gmail: a div per line, `<div><br></div>` a blank line.
        assert_eq!(
            texts("<div>Hi Nicola,</div><div><br></div><div>I'd like to serve.</div><div>Thanks,</div>"),
            ["Hi Nicola,\n\nI'd like to serve.\nThanks,"]
        );
        // Outlook: MsoNormal paragraphs are lines; one of `&nbsp;` is blank.
        assert_eq!(
            texts(
                "<p class=MsoNormal>Kim Hill</p><p class=MsoNormal>Election Clerk</p>\
                 <p class=MsoNormal>&nbsp;</p><p class=MsoNormal>Next</p>"
            ),
            ["Kim Hill\nElection Clerk\n \nNext"]
        );
        assert_eq!(texts("<p>one</p><p>two</p>"), ["one", "two"]);
    }

    #[test]
    fn blocks_break_and_inline_flows() {
        assert_eq!(texts("<div>one</div><p>two</p>"), ["one", "two"]);
        assert_eq!(texts("<p>a<br>b</p>"), ["a\nb"]);
        assert_eq!(texts("<h2>Title</h2><p>Body</p>"), ["# Title", "Body"]);
        assert_eq!(texts("<blockquote><p>said</p></blockquote>"), ["said"]);
        assert!(matches!(blocks("<blockquote>x</blockquote>")[0], ProseBlock::Quote(_)));
        assert_eq!(texts("<hr>"), ["---"]);
        assert_eq!(texts("<pre>a  b\n c</pre>"), ["```a  b\n c```"]);
    }

    #[test]
    fn styles_and_links() {
        let b = blocks(
            "<p><b>bold</b> <i>it</i> <span style=\"font-weight: 700\">heavy</span> \
             <a href=\"javascript:alert(1)\">bad</a> <a href=\"mailto:a@b.co\">mail</a></p>",
        );
        let ProseBlock::Paragraph(runs) = &b[0] else {
            panic!("{b:?}")
        };
        assert!(runs.iter().any(|r| r.text == "bold" && r.bold));
        assert!(runs.iter().any(|r| r.text == "it" && r.italic));
        assert!(runs.iter().any(|r| r.text == "heavy" && r.bold));
        assert!(runs.iter().all(|r| r.url.as_deref() != Some("javascript:alert(1)")));
        assert!(runs.iter().any(|r| r.url.as_deref() == Some("mailto:a@b.co")));
    }

    #[test]
    fn nothing_hidden_or_active_is_drawn() {
        assert_eq!(
            texts(
                "<style>p{color:red}</style><script>alert(1)</script>\
                 <div style=\"display:none;max-height:0\">preheader</div>\
                 <span style=\"font-size: 0px\">x</span><p>shown&zwnj;&nbsp;&#847;</p>"
            ),
            ["shown"]
        );
        assert!(texts("<form><input value=v><textarea>t</textarea></form>").is_empty());
    }

    /// Steam's wishlist mail zeroes the font of the cell around the whole
    /// letter; only its footer, outside it, was drawn.
    #[test]
    fn a_zero_font_hides_only_text_that_keeps_it() {
        let b = blocks(
            "<table><tr><td style=\"font-size:0pt; line-height:0pt\">&nbsp;\
               <img src=\"cid:logo\" alt=\"Logo\" width=\"600\" height=\"80\">\
               <table><tr><td style=\"font-size:28px\"><strong>On sale</strong></td></tr></table>\
               <div style=\"font-size:14px\">A game <span style=\"font-size:0\">gap</span>you wished for</div>\
             </td></tr></table>",
        );
        assert!(matches!(&b[0], ProseBlock::Images(i) if i[0].alt == "Logo"), "{b:?}");
        let words: Vec<String> = b
            .iter()
            .filter_map(|b| match b {
                ProseBlock::Paragraph(runs) => Some(runs.iter().map(|r| r.text.as_str()).collect()),
                _ => None,
            })
            .collect();
        assert_eq!(words, ["On sale", "A game you wished for"]);
    }

    #[test]
    fn a_header_cell_takes_its_own_weight() {
        let b = blocks(
            "<table><tr><th>Head</th><th style=\"font-weight:normal\">Plain</th></tr></table>",
        );
        let ProseBlock::Paragraph(runs) = &b[0] else {
            panic!("{b:?}")
        };
        assert!(runs.iter().any(|r| r.text == "Head" && r.bold), "{runs:?}");
        assert!(runs.iter().any(|r| r.text == "Plain" && !r.bold), "{runs:?}");
    }

    #[test]
    fn lists_keep_their_items() {
        assert_eq!(
            texts("<ul><li>one</li><li><p>two</p></li></ul>"),
            ["- one - two"]
        );
        assert_eq!(
            texts("<ol><li>a<ul><li>b</li></ul></li><li>c</li></ol>"),
            ["- a", "- b", "- c"]
        );
    }

    #[test]
    fn tables_read_as_rows() {
        // Short cells: one line. Layout tables: stacked.
        assert_eq!(
            texts(
                "<table><tr><td><a href=\"https://t.example/x\">Twitter</a></td>\
                 <td><a href=\"https://f.example/y\">Facebook</a></td></tr></table>"
            ),
            ["Twitter\u{2003}Facebook"]
        );
        assert_eq!(
            texts(
                "<table><tr><td><p>Hello</p><p>World</p></td><td>side</td></tr>\
                 <tr><td><table><tr><td>nested</td></tr></table></td></tr></table>"
            ),
            ["Hello", "World", "side", "nested"]
        );
    }

    #[test]
    fn pictures_in_rows_share_a_line() {
        let cells = |n| {
            (0..n)
                .map(|i| format!("<td><a href=\"https://s.example/{i}\"><img src=\"https://cdn.example/{i}.png\" alt=\"s{i}\"></a></td>"))
                .collect::<String>()
        };
        assert_eq!(
            texts(&format!("<table><tr>{}</tr></table>", cells(3))),
            ["[img https://cdn.example/0.png] [img https://cdn.example/1.png] [img https://cdn.example/2.png]"]
        );
        // Blocks between pictures keep them apart.
        assert_eq!(
            texts("<div><img src=\"https://a.example/1.png\"></div><div><img src=\"https://a.example/2.png\"></div>"),
            ["[img https://a.example/1.png]", "[img https://a.example/2.png]"]
        );
    }

    #[test]
    fn pictures_by_source() {
        let w = walk(&parse(
            "<p>Hi</p><img src=\"cid:logo@x\" alt=\"Logo\" width=\"200\">\
             <a href=\"https://shop.example/p\"><img src=\"https://cdn.example/a.png\" width=\"600\" height=\"300\"></a>\
             <img src=\"https://t.example/open.gif\" width=\"1\" height=\"1\">\
             <p>Follow <img src=\"https://cdn.example/tw.png\" width=\"24\" height=\"24\" alt=\"Twitter\"></p>",
        ));
        let keys: Vec<_> = w.images.iter().map(|(k, _)| k.as_str()).collect();
        assert_eq!(keys, ["cid:logo@x", "https://cdn.example/a.png"]);
        // Inline, one after the other: one line of two.
        let ProseBlock::Images(line) = &w.blocks[1] else {
            panic!("{:?}", w.blocks)
        };
        assert_eq!(line.len(), 2);
        let linked = &line[1];
        assert_eq!(linked.url.as_deref(), Some("https://shop.example/p"));
        assert_eq!((linked.width, linked.height), (Some(600), Some(300)));
        // The icon reads as its alt text; the pixel is gone.
        let last: Vec<_> = texts(
            "<p>Follow <img src=\"https://cdn.example/tw.png\" width=\"24\" height=\"24\" alt=\"Twitter\"></p>",
        );
        assert_eq!(last, ["Follow Twitter"]);
    }
}
