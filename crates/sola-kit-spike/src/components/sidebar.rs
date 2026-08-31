//! List-etch sidebar — one builder, every kit consumer.
//!
//! Markup matches settings-lab: `aside.sidebar > .nav > .row > .etch > .label`
//! (optional `.nav-stack` + `.sub` when a row has a subtitle). Apps pass
//! items; they do not invent a parallel rail.

use crate::dom::Elem;
use crate::markup;

pub struct SidebarItem {
    pub id: Option<String>,
    pub label: String,
    pub subtitle: Option<String>,
    pub header: bool,
    pub active: bool,
    pub action: Option<String>,
}

impl SidebarItem {
    pub fn new(id: impl Into<String>, label: impl Into<String>) -> Self {
        Self {
            id: Some(id.into()),
            label: label.into(),
            subtitle: None,
            header: false,
            active: false,
            action: None,
        }
    }

    pub fn header(label: impl Into<String>) -> Self {
        Self {
            id: None,
            label: label.into(),
            subtitle: None,
            header: true,
            active: false,
            action: None,
        }
    }

    pub fn subtitle(mut self, s: impl Into<String>) -> Self {
        let s = s.into();
        if !s.is_empty() {
            self.subtitle = Some(s);
        }
        self
    }

    pub fn active(mut self, on: bool) -> Self {
        self.active = on;
        self
    }

    pub fn action(mut self, action: impl Into<String>) -> Self {
        self.action = Some(action.into());
        self
    }
}

pub struct Sidebar {
    data_id: Option<String>,
    nav_id: Option<String>,
    extra: Vec<String>,
    nav_extra: Vec<String>,
    items: Vec<SidebarItem>,
}

impl Sidebar {
    pub fn new(items: impl IntoIterator<Item = SidebarItem>) -> Self {
        Self {
            data_id: None,
            nav_id: None,
            extra: Vec::new(),
            nav_extra: Vec::new(),
            items: items.into_iter().collect(),
        }
    }

    pub fn data_id(mut self, id: impl Into<String>) -> Self {
        self.data_id = Some(id.into());
        self
    }

    pub fn nav_id(mut self, id: impl Into<String>) -> Self {
        self.nav_id = Some(id.into());
        self
    }

    pub fn class(mut self, class: impl Into<String>) -> Self {
        self.extra.push(class.into());
        self
    }

    pub fn nav_class(mut self, class: impl Into<String>) -> Self {
        self.nav_extra.push(class.into());
        self
    }

    /// Fill the parent (last-known / owners rail) instead of the 220px column.
    pub fn fill(self) -> Self {
        self.class("is-fill")
    }

    pub fn build(self, next: &mut u32) -> Elem {
        let mut side_classes = vec!["sidebar"];
        for c in &self.extra {
            side_classes.push(c.as_str());
        }
        let mut aside = markup::tagged(
            next,
            "aside",
            &side_classes,
            None,
            self.data_id.as_deref(),
            "",
        );
        let mut nav_classes = vec!["nav"];
        for c in &self.nav_extra {
            nav_classes.push(c.as_str());
        }
        let mut nav = markup::node(next, &nav_classes, None, self.nav_id.as_deref(), "");
        for item in &self.items {
            nav.children.push(row(next, item));
        }
        aside.children.push(nav);
        aside
    }
}

pub fn sidebar(next: &mut u32, items: impl IntoIterator<Item = SidebarItem>) -> Elem {
    Sidebar::new(items).build(next)
}

fn row(next: &mut u32, item: &SidebarItem) -> Elem {
    let mut classes = vec!["row"];
    if item.header {
        classes.push("is-header");
    }
    if item.active {
        classes.push("is-active");
    }
    if item.subtitle.is_some() {
        classes.push("has-sub");
    }
    let kind = if item.header { "header" } else { "item" };
    let mut row = markup::node(
        next,
        &classes,
        item.action.as_deref(),
        item.id.as_deref(),
        "",
    );
    row.data_kind = Some(kind.into());
    let mut etch = markup::node(next, &["etch"], None, None, "");
    if let Some(sub) = &item.subtitle {
        let mut stack = markup::node(next, &["nav-stack"], None, None, "");
        stack
            .children
            .push(markup::node(next, &["label"], None, None, &item.label));
        stack
            .children
            .push(markup::node(next, &["sub"], None, None, sub));
        etch.children.push(stack);
    } else {
        etch.children
            .push(markup::node(next, &["label"], None, None, &item.label));
    }
    row.children.push(etch);
    row
}

#[cfg(test)]
mod tests {
    use super::*;

    fn labels(el: &Elem) -> Vec<String> {
        let mut out = Vec::new();
        fn walk(el: &Elem, out: &mut Vec<String>) {
            if el.classes.iter().any(|c| c == "label") {
                out.push(el.text.clone());
            }
            for c in &el.children {
                walk(c, out);
            }
        }
        walk(el, &mut out);
        out
    }

    #[test]
    fn settings_and_monitor_share_etch_markup() {
        let mut next = 1u32;
        let settings = Sidebar::new([
            SidebarItem::header("SETTINGS"),
            SidebarItem::new("apps", "Applications")
                .action("panel")
                .active(true),
            SidebarItem::new("mail", "Mail").action("panel"),
        ])
        .class("sidebar-settings")
        .nav_class("settings-nav")
        .build(&mut next);
        let monitor = Sidebar::new([
            SidebarItem::new("bus", "Bus")
                .action("plane")
                .subtitle("Fan-out facts")
                .active(true),
            SidebarItem::new("call", "Call")
                .action("plane")
                .subtitle("Request / reply"),
        ])
        .data_id("sidebar")
        .build(&mut next);

        for (name, el) in [("settings", &settings), ("monitor", &monitor)] {
            assert_eq!(el.tag, "aside", "{name} root");
            assert!(el.has_class("sidebar"), "{name} class");
            let nav = el.children.first().expect("nav");
            assert!(nav.has_class("nav"), "{name} nav");
            let row = nav.children.first().expect("row");
            assert!(row.has_class("row"), "{name} row");
            let etch = row.children.first().expect("etch");
            assert!(etch.has_class("etch"), "{name} etch");
        }
        assert!(settings.has_class("sidebar-settings"));
        assert_eq!(labels(&settings), ["SETTINGS", "Applications", "Mail"]);
        assert_eq!(labels(&monitor), ["Bus", "Call"]);
        let bus = &monitor.children[0].children[0];
        assert!(bus.has_class("has-sub"));
        assert!(bus.has_class("is-active"));
    }
}
