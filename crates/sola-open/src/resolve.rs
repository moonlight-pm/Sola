//! Which handler opens a scheme or a type. In order, the first that still
//! exists: the one you chose, Sola's own claimant, other claimants (the
//! earliest claim first — a later claim never moves ahead), then for a
//! scheme the `.desktop` handlers.

use std::path::PathBuf;

use crate::claims::Claim;
use crate::defaults::Defaults;
use crate::desktop::Entry;
use crate::target::Kind;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Handler {
    /// A Sola app, by its app id.
    App(String),
    /// A `.desktop` handler.
    Desktop(Entry),
}

impl Handler {
    /// What `defaults.json` records for it: the app id, or the `.desktop`
    /// file's name.
    pub fn id(&self) -> &str {
        match self {
            Handler::App(id) => id,
            Handler::Desktop(e) => &e.id,
        }
    }
}

/// What resolution reads; [`Sources::live`] for this user and machine.
pub struct Sources<'a> {
    pub defaults: Defaults,
    pub builtin: Vec<Claim>,
    pub recorded: Vec<Claim>,
    /// An app id whose program is installed.
    pub installed: &'a dyn Fn(&str) -> bool,
    /// The `.desktop` handlers found.
    pub desktop: Vec<Entry>,
    /// The `mimeapps.list` that names default `.desktop` handlers.
    pub mimeapps: PathBuf,
}

impl Sources<'static> {
    pub fn live() -> Sources<'static> {
        Sources {
            defaults: Defaults::load(),
            builtin: crate::claims::builtin(),
            recorded: crate::claims::recorded(),
            installed: &|app_id| sola_core::paths::bin_path(app_id).is_file(),
            desktop: crate::desktop::entries(&crate::desktop::application_dirs()),
            mimeapps: crate::desktop::mimeapps_list(),
        }
    }
}

/// The apps that claim `kind`, installed ones only: Sola's own first, then
/// the others earliest first.
pub fn claimants(kind: &Kind, s: &Sources) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for c in s.builtin.iter().chain(&s.recorded) {
        if c.covers(kind) && !out.contains(&c.app_id) && (s.installed)(&c.app_id) {
            out.push(c.app_id.clone());
        }
    }
    out
}

/// Everything that could open `kind`: its claimants, then for a scheme the
/// `.desktop` handlers that list it.
pub fn choices(kind: &Kind, s: &Sources) -> Vec<Handler> {
    let mut out: Vec<Handler> = claimants(kind, s).into_iter().map(Handler::App).collect();
    if let Kind::Scheme(scheme) = kind {
        out.extend(
            s.desktop
                .iter()
                .filter(|e| e.opens_scheme(scheme))
                .cloned()
                .map(Handler::Desktop),
        );
    }
    out
}

pub fn resolve(kind: &Kind, s: &Sources) -> Option<Handler> {
    if let Some(chosen) = s.defaults.of(kind) {
        if chosen.ends_with(".desktop") {
            if let Some(e) = s.desktop.iter().find(|e| e.id == chosen) {
                return Some(Handler::Desktop(e.clone()));
            }
        } else if (s.installed)(chosen) {
            return Some(Handler::App(chosen.to_string()));
        }
    }
    if let Some(app) = claimants(kind, s).into_iter().next() {
        return Some(Handler::App(app));
    }
    match kind {
        Kind::Scheme(scheme) => {
            crate::desktop::handler_for_scheme_in(&s.desktop, scheme, &s.mimeapps).map(Handler::Desktop)
        }
        Kind::Type(_) => None,
    }
}

/// A scheme or a type, the handler that opens it, and what else could.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Row {
    pub kind: Kind,
    pub handler: Option<Handler>,
    pub choices: Vec<Handler>,
}

/// Every scheme and type something opens: the schemes first, then the
/// types, each in name order.
pub fn table(s: &Sources) -> Vec<Row> {
    let mut kinds: Vec<Kind> = Vec::new();
    let mut add = |k: Kind| {
        if !kinds.contains(&k) {
            kinds.push(k);
        }
    };
    for c in s.builtin.iter().chain(&s.recorded).filter(|c| (s.installed)(&c.app_id)) {
        c.schemes.iter().for_each(|x| add(Kind::Scheme(x.clone())));
        c.types.iter().for_each(|x| add(Kind::Type(x.clone())));
    }
    for e in &s.desktop {
        e.schemes().for_each(|x| add(Kind::Scheme(x.to_ascii_lowercase())));
    }
    kinds.sort_by_key(|k| match k {
        Kind::Scheme(x) => (0, x.clone()),
        Kind::Type(x) => (1, x.clone()),
    });
    kinds
        .into_iter()
        .map(|kind| Row {
            handler: resolve(&kind, s),
            choices: choices(&kind, s),
            kind,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sources<'a>(
        defaults: &[(&str, &str)],
        recorded: Vec<Claim>,
        installed: &'a dyn Fn(&str) -> bool,
        desktop: Vec<Entry>,
    ) -> Sources<'a> {
        Sources {
            defaults: Defaults {
                defaults: defaults.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect(),
            },
            builtin: crate::claims::builtin(),
            recorded,
            installed,
            desktop,
            mimeapps: PathBuf::from("/nonexistent/mimeapps.list"),
        }
    }

    fn claim(app: &str, schemes: &[&str], since: u64) -> Claim {
        Claim { since, ..Claim::new(app, schemes, &[]) }
    }

    fn all(_: &str) -> bool {
        true
    }
    const ALL: &dyn Fn(&str) -> bool = &all;
    fn mailto() -> Kind {
        Kind::Scheme("mailto".into())
    }

    #[test]
    fn a_new_claim_never_takes_over() {
        let s = sources(&[], vec![claim("foo-mail", &["mailto"], 5)], ALL, vec![]);
        assert_eq!(resolve(&mailto(), &s), Some(Handler::App("sola-mail".into())));
        assert_eq!(claimants(&mailto(), &s), ["sola-mail", "foo-mail"]);
    }

    #[test]
    fn the_earlier_claim_comes_first() {
        let s = sources(
            &[],
            vec![claim("first", &["news"], 1), claim("second", &["news"], 2)],
            ALL,
            vec![],
        );
        assert_eq!(resolve(&Kind::Scheme("news".into()), &s), Some(Handler::App("first".into())));
    }

    #[test]
    fn your_choice_wins_while_its_app_is_there() {
        let s = sources(&[("mailto:", "foo-mail")], vec![], ALL, vec![]);
        assert_eq!(resolve(&mailto(), &s), Some(Handler::App("foo-mail".into())));
        let gone: &dyn Fn(&str) -> bool = &|a| a != "foo-mail";
        let s = sources(&[("mailto:", "foo-mail")], vec![], gone, vec![]);
        assert_eq!(resolve(&mailto(), &s), Some(Handler::App("sola-mail".into())));
    }

    #[test]
    fn unclaimed_schemes_go_to_desktop_handlers_and_types_to_none() {
        let entry = Entry::new("c.desktop", "c %u", &["claude-cli"]);
        let s = sources(&[], vec![], ALL, vec![entry.clone()]);
        assert_eq!(resolve(&Kind::Scheme("claude-cli".into()), &s), Some(Handler::Desktop(entry)));
        assert_eq!(resolve(&Kind::Type("zip".into()), &s), None);
        assert_eq!(resolve(&Kind::Type("png".into()), &s), Some(Handler::App("sola-paint".into())));
    }

    #[test]
    fn a_desktop_handler_can_be_your_choice() {
        let mailer = Entry::new("mutt.desktop", "mutt %u", &["mailto"]);
        let s = sources(&[("mailto:", "mutt.desktop")], vec![], ALL, vec![mailer.clone()]);
        assert_eq!(resolve(&mailto(), &s), Some(Handler::Desktop(mailer.clone())));
        assert_eq!(
            choices(&mailto(), &s),
            [Handler::App("sola-mail".into()), Handler::Desktop(mailer)]
        );
    }

    #[test]
    fn the_table_lists_schemes_then_types_with_their_handlers() {
        let s = sources(&[], vec![], ALL, vec![Entry::new("c.desktop", "c %u", &["claude-cli"])]);
        let rows = table(&s);
        let keys: Vec<String> = rows.iter().map(|r| r.kind.key()).collect();
        assert_eq!(keys.first().map(String::as_str), Some("claude-cli:"));
        assert!(keys.iter().position(|k| k == "mailto:") < keys.iter().position(|k| k == ".png"));
        let png = rows.iter().find(|r| r.kind.key() == ".png").unwrap();
        assert_eq!(png.handler, Some(Handler::App("sola-paint".into())));
    }
}
