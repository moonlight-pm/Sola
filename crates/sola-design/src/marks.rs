//! What a page declares, read when it settles (see [`crate::inspect::settle`]):
//! its exits (`data-design-to`), its plain links, and where the notes
//! pinned into it sit. Rects are board-local CSS px.

use serde::Deserialize;
use serde_json::Value;

use crate::select::Rect;

/// An element that leads to another board.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct Exit {
    pub to: String,
    /// `data-design-label`.
    #[serde(default)]
    pub label: Option<String>,
    /// The element's text (trimmed, short).
    #[serde(default)]
    pub text: String,
    pub rect: Rect,
}

impl Exit {
    /// What a tour calls this choice: the label, else the element's text.
    pub fn name(&self) -> Option<&str> {
        self.label
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .or_else(|| Some(self.text.as_str()).filter(|s| !s.is_empty()))
    }
}

/// A plain `<a href>` (resolved URL); it moves the camera when its URL is
/// another board's page.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct PageLink {
    pub href: String,
    pub rect: Rect,
}

/// Where note `n`'s `at` element is (`None`: nothing matched).
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct Anchor {
    pub n: usize,
    pub rect: Option<Rect>,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Marks {
    pub exits: Vec<Exit>,
    pub links: Vec<PageLink>,
    pub anchors: Vec<Anchor>,
}

impl Marks {
    /// From a settle reply (missing parts are empty).
    pub fn from_settle(data: &Value) -> Self {
        fn list<T: for<'de> Deserialize<'de>>(data: &Value, key: &str) -> Vec<T> {
            data.get(key)
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .filter_map(|v| serde_json::from_value(v.clone()).ok())
                .collect()
        }
        Self {
            exits: list(data, "exits"),
            links: list(data, "links"),
            anchors: list(data, "anchors"),
        }
    }

    pub fn anchor(&self, n: usize) -> Option<&Anchor> {
        self.anchors.iter().find(|a| a.n == n)
    }
}

/// A board URL split for matching links: boards Design serves each have
/// their own `*.localhost` host, so local pages compare by path and query
/// only; web pages by host too.
#[derive(Debug, PartialEq)]
struct Key<'a> {
    host: &'a str,
    path: &'a str,
    query: &'a str,
    fragment: &'a str,
}

fn key(url: &str) -> Option<Key<'_>> {
    let (scheme, rest) = url.split_once("://")?;
    let cut = rest.find(['/', '?', '#']).unwrap_or(rest.len());
    let (authority, rest) = rest.split_at(cut);
    let (rest, fragment) = rest.split_once('#').unwrap_or((rest, ""));
    let (path, query) = rest.split_once('?').unwrap_or((rest, ""));
    let host_only = authority.rsplit_once(':').map_or(authority, |(h, _)| h);
    let local = scheme == "file" || host_only.ends_with(".localhost");
    Some(Key {
        host: if local { "" } else { authority },
        path: if path.is_empty() { "/" } else { path },
        query,
        fragment,
    })
}

/// The board (other than `from`) whose page `href` is: same path and query
/// (host too for web pages); the fragment decides between boards that
/// share a page. A link to `from`'s own page goes nowhere unless its
/// fragment names another board's; a link that fits several boards goes
/// nowhere.
pub fn board_for_url<'a>(href: &str, boards: &[(&'a str, &str)], from: &str) -> Option<&'a str> {
    let want = key(href)?;
    let same_page = |k: &Key| k.host == want.host && k.path == want.path && k.query == want.query;
    let own = boards.iter().find(|(id, _)| *id == from).and_then(|(_, url)| key(url));
    let own_page = own.as_ref().is_some_and(same_page);
    let hits: Vec<(&'a str, Key)> = boards
        .iter()
        .filter(|(id, _)| *id != from)
        .filter_map(|(id, url)| Some((*id, key(url)?)))
        .filter(|(_, k)| same_page(k))
        .collect();
    let by_fragment = || {
        let mut named = hits.iter().filter(|(_, k)| k.fragment == want.fragment);
        match (named.next(), named.next()) {
            (Some((id, _)), None) => Some(*id),
            _ => None,
        }
    };
    if own_page {
        return by_fragment().filter(|_| own.is_some_and(|o| o.fragment != want.fragment));
    }
    match hits.as_slice() {
        [(id, _)] => Some(id),
        _ => by_fragment(),
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    const HOST: &str = "http://overview--foundation.wicket.localhost:4100";

    fn boards() -> Vec<(&'static str, String)> {
        vec![
            ("overview", format!("{HOST}/design/foundation/overview.html")),
            ("backups", "http://backups--foundation.wicket.localhost:4100/design/foundation/backups.html".into()),
            ("light", "http://light--foundation.wicket.localhost:4100/design/foundation/theme.html#light".into()),
            ("dark", "http://dark--foundation.wicket.localhost:4100/design/foundation/theme.html#dark".into()),
            ("live", "http://localhost:5173/settings".into()),
            ("laptop", "http://laptop--foundation.wicket.localhost:4100/design/foundation/overview.html".into()),
            ("phone", "http://phone--foundation.wicket.localhost:4100/design/foundation/overview.html".into()),
        ]
    }

    fn find(href: &str, from: &str) -> Option<&'static str> {
        let b = boards();
        let refs: Vec<(&'static str, &str)> = b.iter().map(|(id, u)| (*id, u.as_str())).collect();
        board_for_url(href, &refs, from)
    }

    #[test]
    fn links_match_boards_by_page() {
        // A relative link resolves on the clicking board's own host.
        assert_eq!(find(&format!("{HOST}/design/foundation/backups.html"), "overview"), Some("backups"));
        assert_eq!(find(&format!("{HOST}/design/foundation/backups.html?x=1"), "overview"), None);
        assert_eq!(find(&format!("{HOST}/design/foundation/theme.html#dark"), "light"), Some("dark"));
        assert_eq!(find(&format!("{HOST}/design/foundation/theme.html"), "overview"), None, "ambiguous");
        assert_eq!(find("http://localhost:5173/settings", "overview"), Some("live"));
        assert_eq!(find("http://example.com/design/foundation/backups.html", "overview"), None);
        assert_eq!(find(&format!("{HOST}/design/foundation/overview.html#top"), "overview"), None, "itself");
        // Boards sharing overview.html: a link to it from one of them is
        // its own page, from elsewhere it fits three.
        assert_eq!(find(&format!("{HOST}/design/foundation/overview.html"), "laptop"), None);
        assert_eq!(find(&format!("{HOST}/design/foundation/overview.html"), "backups"), None);
        assert_eq!(find(&format!("{HOST}/design/foundation/theme.html#light"), "light"), None, "its own state");
        assert_eq!(find("mailto:a@b.c", "overview"), None);
    }

    #[test]
    fn reads_a_settle_reply() {
        let m = Marks::from_settle(&json!({
            "height": 900,
            "exits": [
                {"to": "b", "label": null, "text": "Send link", "rect": {"x": 1, "y": 2, "width": 3, "height": 4}},
                {"to": "c", "label": " Go ", "text": "", "rect": {"x": 0, "y": 0, "width": 1, "height": 1}},
                {"bad": true}
            ],
            "anchors": [{"n": 2, "rect": null}],
        }));
        assert_eq!(m.exits.len(), 2);
        assert_eq!(m.exits[0].name(), Some("Send link"));
        assert_eq!(m.exits[1].name(), Some("Go"));
        assert!(m.links.is_empty());
        assert_eq!(m.anchor(2).unwrap().rect, None);
    }
}
