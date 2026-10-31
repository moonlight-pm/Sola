//! Which rows the sheet lists, in what order — pure, so it is tested.
//!
//! On a page with logins of its own the sheet opens on them ("On this
//! site"). Otherwise, or past them, it lists the whole vault by kind:
//! recently used first, then A–Z. Typing searches every item.

use sola_vault_proto::{ItemKind, ItemSummary};

/// How many recently used items lead the whole vault.
const RECENT: usize = 5;

/// The kind tabs over the whole vault.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Kind {
    #[default]
    All,
    Logins,
    Cards,
    Identities,
    Notes,
}

impl Kind {
    pub const ALL: [Kind; 5] = [
        Kind::All,
        Kind::Logins,
        Kind::Cards,
        Kind::Identities,
        Kind::Notes,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::All => "All",
            Self::Logins => "Logins",
            Self::Cards => "Cards",
            Self::Identities => "Identities",
            Self::Notes => "Notes",
        }
    }

    pub fn takes(self, kind: ItemKind) -> bool {
        match self {
            Self::All => true,
            Self::Logins => kind == ItemKind::Login,
            Self::Cards => kind == ItemKind::Card,
            Self::Identities => kind == ItemKind::Identity,
            Self::Notes => kind == ItemKind::SecureNote,
        }
    }
}

/// A heading over a run of rows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Section {
    Site,
    Recent,
    AToZ,
    Results,
}

impl Section {
    pub fn label(self) -> &'static str {
        match self {
            Self::Site => "On this site",
            Self::Recent => "Recently used",
            Self::AToZ => "A–Z",
            Self::Results => "",
        }
    }
}

/// How the sheet is looking at the vault.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Mode {
    /// The page's logins.
    Site,
    /// The page has a site and no logins for it.
    NoneHere,
    /// Every item, by kind.
    Whole,
    /// Every item matching the query.
    Search,
}

/// The mode for a page (`has_site`), a query and whether the user went
/// past the site's logins.
pub(crate) fn mode(items: &[ItemSummary], has_site: bool, query: &str, whole: bool) -> Mode {
    if !query.trim().is_empty() {
        Mode::Search
    } else if !has_site || whole {
        Mode::Whole
    } else if site(items).is_empty() {
        Mode::NoneHere
    } else {
        Mode::Site
    }
}

fn live(items: &[ItemSummary]) -> impl Iterator<Item = &ItemSummary> {
    items.iter().filter(|i| !i.deleted)
}

fn by_name(a: &ItemSummary, b: &ItemSummary) -> std::cmp::Ordering {
    a.name
        .to_lowercase()
        .cmp(&b.name.to_lowercase())
        .then_with(|| a.subtitle.cmp(&b.subtitle))
}

/// The page's logins: most recently used first.
pub(crate) fn site(items: &[ItemSummary]) -> Vec<&ItemSummary> {
    let mut rows: Vec<&ItemSummary> = live(items)
        .filter(|i| i.uri_match && i.kind == ItemKind::Login)
        .collect();
    rows.sort_by(|a, b| b.last_used.cmp(&a.last_used).then_with(|| by_name(a, b)));
    rows
}

/// The rows the sheet lists, each with its section, in order.
pub(crate) fn rows<'a>(
    items: &'a [ItemSummary],
    mode: Mode,
    kind: Kind,
    query: &str,
) -> Vec<(Section, &'a ItemSummary)> {
    match mode {
        Mode::Site => site(items).into_iter().map(|i| (Section::Site, i)).collect(),
        Mode::NoneHere => Vec::new(),
        Mode::Search => {
            let mut hits: Vec<&ItemSummary> =
                live(items).filter(|i| i.matches_query(query)).collect();
            // The page's own first, then by name.
            hits.sort_by(|a, b| b.uri_match.cmp(&a.uri_match).then_with(|| by_name(a, b)));
            hits.into_iter().map(|i| (Section::Results, i)).collect()
        }
        Mode::Whole => {
            let of_kind: Vec<&ItemSummary> = live(items).filter(|i| kind.takes(i.kind)).collect();
            let mut recent: Vec<&ItemSummary> =
                of_kind.iter().copied().filter(|i| i.last_used > 0).collect();
            recent.sort_by(|a, b| b.last_used.cmp(&a.last_used).then_with(|| by_name(a, b)));
            recent.truncate(RECENT);
            let mut rest: Vec<&ItemSummary> = of_kind
                .into_iter()
                .filter(|i| !recent.iter().any(|r| r.id == i.id))
                .collect();
            rest.sort_by(|a, b| by_name(a, b));
            recent
                .into_iter()
                .map(|i| (Section::Recent, i))
                .chain(rest.into_iter().map(|i| (Section::AToZ, i)))
                .collect()
        }
    }
}

/// Items the vault holds (trash not counted).
pub(crate) fn count(items: &[ItemSummary]) -> usize {
    live(items).count()
}

/// The letter on an item's plate.
pub(crate) fn initial(name: &str) -> String {
    name.trim()
        .chars()
        .find(|c| c.is_alphanumeric())
        .map(|c| c.to_uppercase().collect())
        .unwrap_or_else(|| "·".into())
}

/// A one-time code in groups of three (`482 913`) or four (`1234 5678`).
pub(crate) fn grouped_code(code: &str) -> String {
    match code.len() {
        6 => format!("{} {}", &code[..3], &code[3..]),
        8 => format!("{} {}", &code[..4], &code[4..]),
        _ => code.to_string(),
    }
}

/// The foot's line: when the vault last synced, from `now` (Unix seconds).
pub(crate) fn synced_label(synced_at: Option<u64>, now: u64, failed: bool) -> String {
    let Some(at) = synced_at else {
        return if failed {
            "Not synced".into()
        } else {
            "Syncing…".into()
        };
    };
    let ago = now.saturating_sub(at);
    if failed {
        return format!("Last synced {}", ago_words(ago));
    }
    format!("Synced {}", ago_words(ago))
}

fn ago_words(secs: u64) -> String {
    match secs {
        0..=59 => "just now".into(),
        60..=119 => "1 min ago".into(),
        120..=3599 => format!("{} min ago", secs / 60),
        3600..=7199 => "1 hour ago".into(),
        7200..=86_399 => format!("{} hours ago", secs / 3600),
        _ => format!("{} days ago", secs / 86_400),
    }
}

/// `Edited Oct 30` from an RFC 3339 time (the date part only).
pub(crate) fn edited_label(revised: Option<&str>) -> Option<String> {
    let date = revised?.get(..10)?;
    let mut parts = date.split('-');
    let (_, month, day) = (parts.next()?, parts.next()?, parts.next()?);
    const MONTHS: [&str; 12] = [
        "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
    ];
    let m: usize = month.parse().ok()?;
    let d: u32 = day.parse().ok()?;
    Some(format!("Edited {} {d}", MONTHS.get(m.checked_sub(1)?)?))
}

/// A stored website as a link to open: `github.com` → `https://github.com`.
pub(crate) fn website_url(uri: &str) -> Option<String> {
    let t = uri.trim();
    if t.is_empty() {
        return None;
    }
    if t.starts_with("https://") || t.starts_with("http://") {
        return Some(t.to_string());
    }
    if t.contains("://") || !t.contains('.') {
        return None;
    }
    Some(format!("https://{t}"))
}

/// The host of a stored website, for a record's head.
pub(crate) fn website_host(uri: &str) -> String {
    let url = website_url(uri).unwrap_or_default();
    let host = crate::vault::log_host(&url);
    if host.is_empty() {
        uri.trim().to_string()
    } else {
        host.trim_start_matches("www.").to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(id: &str, name: &str, kind: ItemKind, used: i64, here: bool) -> ItemSummary {
        ItemSummary {
            id: id.into(),
            kind,
            name: name.into(),
            subtitle: String::new(),
            last_used: used,
            revised: 0,
            has_totp: false,
            has_passkey: false,
            uri_match: here,
            reprompt: false,
            favorite: false,
            folder_id: None,
            organization_id: None,
            collection_ids: Vec::new(),
            deleted: false,
            search: name.to_lowercase(),
        }
    }

    fn names(rows: &[(Section, &ItemSummary)]) -> Vec<String> {
        rows.iter().map(|(_, i)| i.name.clone()).collect()
    }

    #[test]
    fn a_page_with_logins_opens_on_them_most_recent_first() {
        let items = vec![
            item("1", "GitHub — work", ItemKind::Login, 10, true),
            item("2", "GitHub", ItemKind::Login, 20, true),
            item("3", "Amazon", ItemKind::Login, 30, false),
            item("4", "A card here", ItemKind::Card, 40, true),
        ];
        let m = mode(&items, true, "", false);
        assert_eq!(m, Mode::Site);
        assert_eq!(names(&rows(&items, m, Kind::All, "")), ["GitHub", "GitHub — work"]);
        assert_eq!(mode(&items, true, "", true), Mode::Whole);
        assert_eq!(mode(&items, false, "", false), Mode::Whole);
    }

    #[test]
    fn a_site_with_nothing_saved_says_so() {
        let items = vec![item("1", "Amazon", ItemKind::Login, 0, false)];
        assert_eq!(mode(&items, true, "", false), Mode::NoneHere);
        assert!(rows(&items, Mode::NoneHere, Kind::All, "").is_empty());
    }

    #[test]
    fn the_whole_vault_is_recent_then_a_to_z_without_repeats() {
        let mut items = vec![
            item("1", "figma", ItemKind::Login, 0, false),
            item("2", "Cloudflare", ItemKind::Login, 50, false),
            item("3", "Amazon", ItemKind::Login, 0, false),
            item("4", "Bank", ItemKind::Card, 0, false),
            item("5", "Old", ItemKind::Login, 0, false),
        ];
        items[4].deleted = true;
        let all = rows(&items, Mode::Whole, Kind::All, "");
        assert_eq!(names(&all), ["Cloudflare", "Amazon", "Bank", "figma"]);
        assert_eq!(all[0].0, Section::Recent);
        assert_eq!(all[1].0, Section::AToZ);
        let cards = rows(&items, Mode::Whole, Kind::Cards, "");
        assert_eq!(names(&cards), ["Bank"]);
        assert_eq!(count(&items), 4);
    }

    #[test]
    fn search_puts_the_page_first() {
        let items = vec![
            item("1", "Google Cloud", ItemKind::Login, 0, false),
            item("2", "Cloudflare", ItemKind::Login, 0, true),
            item("3", "Amazon", ItemKind::Login, 0, false),
        ];
        let m = mode(&items, true, "cloud", false);
        assert_eq!(m, Mode::Search);
        assert_eq!(names(&rows(&items, m, Kind::All, "cloud")), ["Cloudflare", "Google Cloud"]);
    }

    #[test]
    fn plates_codes_and_labels() {
        assert_eq!(initial("github"), "G");
        assert_eq!(initial("  (work) mail"), "W");
        assert_eq!(initial(""), "·");
        assert_eq!(grouped_code("482913"), "482 913");
        assert_eq!(grouped_code("12345678"), "1234 5678");
        assert_eq!(synced_label(Some(1000), 1130, false), "Synced 2 min ago");
        assert_eq!(synced_label(Some(1000), 1010, false), "Synced just now");
        assert_eq!(synced_label(Some(0), 7300, true), "Last synced 2 hours ago");
        assert_eq!(synced_label(None, 0, false), "Syncing…");
        assert_eq!(edited_label(Some("2026-10-30T12:00:00Z")).as_deref(), Some("Edited Oct 30"));
        assert_eq!(edited_label(Some("bad")), None);
    }

    #[test]
    fn websites_open_as_https() {
        assert_eq!(website_url("github.com").as_deref(), Some("https://github.com"));
        assert_eq!(website_url("http://a.com/x").as_deref(), Some("http://a.com/x"));
        assert_eq!(website_url("androidapp://x"), None);
        assert_eq!(website_url("localhost"), None);
        assert_eq!(website_host("https://www.github.com/login"), "github.com");
        assert_eq!(website_host("gist.github.com"), "gist.github.com");
    }
}
