//! Minimal HTML parser for the tags we emit (div/aside/main + attrs).

#[derive(Clone, Debug)]
pub struct Elem {
    pub uid: u32,
    pub tag: String,
    pub classes: Vec<String>,
    pub data_id: Option<String>,
    pub data_kind: Option<String>,
    pub data_surface: Option<String>,
    pub data_input: Option<String>,
    pub data_template: Option<String>,
    pub data_slot: Option<String>,
    pub data_bind: Option<String>,
    pub data_action: Option<String>,
    pub style_attr: Option<String>,
    pub text: String,
    pub children: Vec<Elem>,
}

pub fn parse_html(html: &str) -> Elem {
    let mut p = Parser {
        s: html,
        i: 0,
        next_uid: 1,
    };
    let kids = p.nodes();
    kids.into_iter().next().unwrap_or(Elem {
        uid: 0,
        tag: "div".into(),
        classes: vec!["app".into()],
        data_id: None,
        data_kind: None,
        data_surface: None,
        data_input: None,
        data_template: None,
        data_slot: None,
        data_bind: None,
        data_action: None,
        style_attr: None,
        text: String::new(),
        children: Vec::new(),
    })
}

struct Parser<'a> {
    s: &'a str,
    i: usize,
    next_uid: u32,
}

impl<'a> Parser<'a> {
    fn rest(&self) -> &'a str {
        &self.s[self.i..]
    }

    fn bump(&mut self, n: usize) {
        self.i += n;
    }

    fn skip_ws(&mut self) {
        while self.rest().starts_with(|c: char| c.is_whitespace()) {
            self.bump(1);
        }
    }

    fn nodes(&mut self) -> Vec<Elem> {
        let mut out = Vec::new();
        loop {
            self.skip_ws();
            if self.rest().starts_with("<!--") {
                if let Some(rel) = self.rest().find("-->") {
                    self.bump(rel + 3);
                    continue;
                }
                self.bump(self.rest().len());
                break;
            }
            if self.rest().is_empty() || self.rest().starts_with("</") {
                break;
            }
            if let Some(el) = self.node() {
                out.push(el);
            } else {
                break;
            }
        }
        out
    }

    fn node(&mut self) -> Option<Elem> {
        self.skip_ws();
        if self.rest().starts_with('<') {
            self.element()
        } else {
            let text = self.text_until_tag();
            if text.trim().is_empty() {
                return None;
            }
            let uid = self.next_uid;
            self.next_uid += 1;
            Some(Elem {
                uid,
                tag: "#text".into(),
                classes: Vec::new(),
                data_id: None,
                data_kind: None,
                data_surface: None,
                data_input: None,
                data_template: None,
                data_slot: None,
                data_bind: None,
                data_action: None,
                style_attr: None,
                text,
                children: Vec::new(),
            })
        }
    }

    fn text_until_tag(&mut self) -> String {
        let r = self.rest();
        let end = r.find('<').unwrap_or(r.len());
        let raw = &r[..end];
        self.bump(end);
        unescape(raw)
    }

    fn element(&mut self) -> Option<Elem> {
        if !self.rest().starts_with('<') || self.rest().starts_with("</") {
            return None;
        }
        self.bump(1);
        let tag = self.ident();
        if tag.is_empty() {
            return None;
        }
        let mut classes = Vec::new();
        let mut data_id = None;
        let mut data_kind = None;
        let mut data_surface = None;
        let mut data_input = None;
        let mut data_template = None;
        let mut data_slot = None;
        let mut data_bind = None;
        let mut data_action = None;
        let mut style_attr = None;
        loop {
            self.skip_ws();
            if self.rest().starts_with('>') {
                self.bump(1);
                break;
            }
            if self.rest().starts_with("/>") {
                self.bump(2);
                let uid = self.next_uid;
                self.next_uid += 1;
                return Some(Elem {
                    uid,
                    tag,
                    classes,
                    data_id,
                    data_kind,
                    data_surface,
                    data_input,
                    data_template,
                    data_slot,
                    data_bind,
                    data_action,
                    style_attr,
                    text: String::new(),
                    children: Vec::new(),
                });
            }
            let name = self.ident();
            self.skip_ws();
            if !self.rest().starts_with('=') {
                continue;
            }
            self.bump(1);
            self.skip_ws();
            let val = self.quoted();
            match name.as_str() {
                "class" => classes = val.split_whitespace().map(str::to_string).collect(),
                "data-id" => data_id = Some(val),
                "data-kind" => data_kind = Some(val),
                "data-surface" => data_surface = Some(val),
                "data-input" => data_input = Some(val),
                "data-template" => data_template = Some(val),
                "data-slot" => data_slot = Some(val),
                "data-bind" => data_bind = Some(val),
                "data-action" => data_action = Some(val),
                "style" => style_attr = Some(val),
                _ => {}
            }
        }
        let children_raw = self.nodes();
        if self.rest().starts_with("</") {
            self.bump(2);
            let _ = self.ident();
            if self.rest().starts_with('>') {
                self.bump(1);
            }
        }
        let text = children_raw
            .iter()
            .filter(|c| c.tag == "#text")
            .map(|c| c.text.as_str())
            .collect::<String>();
        let children = children_raw
            .into_iter()
            .filter(|c| c.tag != "#text")
            .collect();
        let uid = self.next_uid;
        self.next_uid += 1;
        Some(Elem {
            uid,
            tag,
            classes,
            data_id,
            data_kind,
            data_surface,
            data_input,
            data_template,
            data_slot,
            data_bind,
            data_action,
            style_attr,
            text: text.trim().to_string(),
            children,
        })
    }

    fn ident(&mut self) -> String {
        let r = self.rest();
        let n = r
            .chars()
            .take_while(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_')
            .count();
        let s = r[..n].to_string();
        self.bump(n);
        s
    }

    fn quoted(&mut self) -> String {
        let r = self.rest();
        let quote = r.chars().next();
        if quote != Some('"') && quote != Some('\'') {
            return String::new();
        }
        self.bump(1);
        let r = self.rest();
        let end = r.find(quote.unwrap()).unwrap_or(r.len());
        let s = unescape(&r[..end]);
        self.bump(end + 1);
        s
    }
}

fn unescape(s: &str) -> String {
    s.replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
}

impl Elem {
    pub fn has_class(&self, class: &str) -> bool {
        self.classes.iter().any(|c| c == class)
    }
}
