//! The list's order: by name, or the last used first.

use std::cmp::Ordering;

use sola_vault_proto::ItemSummary;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Sort {
    #[default]
    Name,
    /// The last used first; never-used items after, by name.
    Recent,
}

/// Sort `items` in place.
pub fn sort(items: &mut [&ItemSummary], by: Sort) {
    match by {
        Sort::Name => items.sort_by(|a, b| by_name(a, b)),
        Sort::Recent => items.sort_by(|a, b| b.last_used.cmp(&a.last_used).then_with(|| by_name(a, b))),
    }
}

/// Case-insensitive, then exact (so the order is stable), then id.
fn by_name(a: &ItemSummary, b: &ItemSummary) -> Ordering {
    let key = |s: &str| s.trim().to_lowercase();
    key(&a.name)
        .cmp(&key(&b.name))
        .then_with(|| a.name.cmp(&b.name))
        .then_with(|| a.id.cmp(&b.id))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::section::tests::summary;
    use sola_vault_proto::ItemKind;

    fn names(items: &[&ItemSummary]) -> Vec<String> {
        items.iter().map(|i| i.name.clone()).collect()
    }

    #[test]
    fn by_name_ignores_case() {
        let a = summary("1", "github", ItemKind::Login);
        let b = summary("2", "Amazon", ItemKind::Login);
        let c = summary("3", "Figma", ItemKind::Login);
        let mut items = vec![&a, &b, &c];
        sort(&mut items, Sort::Name);
        assert_eq!(names(&items), ["Amazon", "Figma", "github"]);
    }

    #[test]
    fn recent_first_then_never_used_by_name() {
        let mut a = summary("1", "Zed", ItemKind::Login);
        a.last_used = 100;
        let mut b = summary("2", "Yak", ItemKind::Login);
        b.last_used = 200;
        let c = summary("3", "Beta", ItemKind::Login);
        let d = summary("4", "Alpha", ItemKind::Login);
        let mut items = vec![&a, &b, &c, &d];
        sort(&mut items, Sort::Recent);
        assert_eq!(names(&items), ["Yak", "Zed", "Alpha", "Beta"]);
    }
}
