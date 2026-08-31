//! Client-side titlebar (traffic-light close + centered title).

use crate::dom::Elem;
use crate::markup;

pub fn titlebar(next: &mut u32, title: &str) -> Elem {
    let mut bar = markup::node(next, &["titlebar"], Some("drag"), Some("csd"), "");
    let mut traffic = markup::node(next, &["traffic"], None, None, "");
    traffic
        .children
        .push(markup::node(next, &["close"], Some("close"), None, ""));
    bar.children.push(traffic);
    bar.children
        .push(markup::node(next, &["title"], None, None, title));
    bar.children
        .push(markup::node(next, &["traffic-spacer"], None, None, ""));
    bar
}

/// Storybook chrome: title filled by `data-bind=title`.
pub fn titlebar_bound(next: &mut u32) -> Elem {
    let mut bar = titlebar(next, "");
    if let Some(title) = bar.children.get_mut(1) {
        title.data_bind = Some("title".into());
    }
    bar
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn emits_csd_close() {
        let mut n = 1u32;
        let bar = titlebar(&mut n, "Monitor (lab)");
        assert!(bar.has_class("titlebar"));
        assert_eq!(bar.data_id.as_deref(), Some("csd"));
        assert_eq!(
            bar.children[0].children[0].data_action.as_deref(),
            Some("close")
        );
    }
}
