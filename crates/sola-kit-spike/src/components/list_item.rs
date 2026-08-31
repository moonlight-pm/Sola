//! Two-line selectable list row (mail, pickers, catalogs).
//!
//! Graphite lift when selected — same language as sidebar etch, not an
//! accent wash.

use crate::dom::Elem;
use crate::markup;

pub struct ListItem {
    pub id: Option<String>,
    pub action: Option<String>,
    pub title: String,
    pub subtitle: Option<String>,
    pub meta: Option<String>,
    pub selected: bool,
    pub strong: bool,
}

impl ListItem {
    pub fn new(id: impl Into<String>, title: impl Into<String>) -> Self {
        Self {
            id: Some(id.into()),
            action: None,
            title: title.into(),
            subtitle: None,
            meta: None,
            selected: false,
            strong: false,
        }
    }

    pub fn action(mut self, action: impl Into<String>) -> Self {
        self.action = Some(action.into());
        self
    }

    pub fn subtitle(mut self, s: impl Into<String>) -> Self {
        let s = s.into();
        if !s.is_empty() {
            self.subtitle = Some(s);
        }
        self
    }

    pub fn meta(mut self, s: impl Into<String>) -> Self {
        let s = s.into();
        if !s.is_empty() {
            self.meta = Some(s);
        }
        self
    }

    pub fn selected(mut self, on: bool) -> Self {
        self.selected = on;
        self
    }

    pub fn strong(mut self, on: bool) -> Self {
        self.strong = on;
        self
    }

    pub fn build(self, next: &mut u32) -> Elem {
        let mut classes = vec!["list-item"];
        if self.selected {
            classes.push("is-active");
        }
        if self.strong {
            classes.push("is-unread");
        }
        let mut row = markup::node(
            next,
            &classes,
            self.action.as_deref(),
            self.id.as_deref(),
            "",
        );
        let mut top = markup::node(next, &["list-item-top"], None, None, "");
        top.children.push(markup::node(
            next,
            &["list-item-title"],
            None,
            None,
            &self.title,
        ));
        if let Some(meta) = &self.meta {
            top.children.push(markup::node(
                next,
                &["t-caption", "list-item-meta"],
                None,
                None,
                meta,
            ));
        }
        row.children.push(top);
        if let Some(sub) = &self.subtitle {
            row.children
                .push(markup::node(next, &["list-item-sub"], None, None, sub));
        }
        row
    }
}

pub fn list_item(next: &mut u32, item: ListItem) -> Elem {
    item.build(next)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unread_row_is_strong() {
        let mut n = 1u32;
        let el = ListItem::new("12", "Wicket")
            .action("msg")
            .subtitle("Sign in")
            .meta("28 Jul")
            .strong(true)
            .selected(true)
            .build(&mut n);
        assert!(el.has_class("list-item"));
        assert!(el.has_class("is-active"));
        assert!(el.has_class("is-unread"));
        assert_eq!(el.data_action.as_deref(), Some("msg"));
        assert_eq!(el.children[0].children[0].text, "Wicket");
        assert_eq!(el.children[1].text, "Sign in");
    }
}
