//! Which handler opens a scheme or a type. In order, the first that still
//! exists: the one you chose, Sola's own claimant, other claimants (the
//! earliest claim first — a later claim never moves ahead), then for a
//! scheme the `.desktop` handlers.

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

/// What resolution reads; [`Sources::live`] for this user and machine.
pub struct Sources<'a> {
    pub defaults: Defaults,
    pub builtin: Vec<Claim>,
    pub recorded: Vec<Claim>,
    /// An app id whose program is installed.
    pub installed: &'a dyn Fn(&str) -> bool,
    /// The `.desktop` handler for a scheme.
    pub desktop: &'a dyn Fn(&str) -> Option<Entry>,
}

impl Sources<'static> {
    pub fn live() -> Sources<'static> {
        Sources {
            defaults: Defaults::load(),
            builtin: crate::claims::builtin(),
            recorded: crate::claims::recorded(),
            installed: &|app_id| sola_core::paths::bin_path(app_id).is_file(),
            desktop: &|scheme| {
                crate::desktop::handler_for_scheme(
                    scheme,
                    &crate::desktop::application_dirs(),
                    &crate::desktop::mimeapps_list(),
                )
            },
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

pub fn resolve(kind: &Kind, s: &Sources) -> Option<Handler> {
    if let Some(app) = s.defaults.of(kind).filter(|a| (s.installed)(a)) {
        return Some(Handler::App(app.to_string()));
    }
    if let Some(app) = claimants(kind, s).into_iter().next() {
        return Some(Handler::App(app));
    }
    match kind {
        Kind::Scheme(scheme) => (s.desktop)(scheme).map(Handler::Desktop),
        Kind::Type(_) => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sources<'a>(
        defaults: &[(&str, &str)],
        recorded: Vec<Claim>,
        installed: &'a dyn Fn(&str) -> bool,
        desktop: &'a dyn Fn(&str) -> Option<Entry>,
    ) -> Sources<'a> {
        Sources {
            defaults: Defaults {
                defaults: defaults.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect(),
            },
            builtin: crate::claims::builtin(),
            recorded,
            installed,
            desktop,
        }
    }

    fn claim(app: &str, schemes: &[&str], since: u64) -> Claim {
        Claim { since, ..Claim::new(app, schemes, &[]) }
    }

    fn all(_: &str) -> bool {
        true
    }
    fn no_desktop(_: &str) -> Option<Entry> {
        None
    }
    const ALL: &dyn Fn(&str) -> bool = &all;
    const NO_DESKTOP: &dyn Fn(&str) -> Option<Entry> = &no_desktop;
    fn mailto() -> Kind {
        Kind::Scheme("mailto".into())
    }

    #[test]
    fn a_new_claim_never_takes_over() {
        let s = sources(&[], vec![claim("foo-mail", &["mailto"], 5)], ALL, NO_DESKTOP);
        assert_eq!(resolve(&mailto(), &s), Some(Handler::App("sola-mail".into())));
        assert_eq!(claimants(&mailto(), &s), ["sola-mail", "foo-mail"]);
    }

    #[test]
    fn the_earlier_claim_comes_first() {
        let s = sources(
            &[],
            vec![claim("first", &["news"], 1), claim("second", &["news"], 2)],
            ALL,
            NO_DESKTOP,
        );
        assert_eq!(resolve(&Kind::Scheme("news".into()), &s), Some(Handler::App("first".into())));
    }

    #[test]
    fn your_choice_wins_while_its_app_is_there() {
        let s = sources(&[("mailto:", "foo-mail")], vec![], ALL, NO_DESKTOP);
        assert_eq!(resolve(&mailto(), &s), Some(Handler::App("foo-mail".into())));
        let gone: &dyn Fn(&str) -> bool = &|a| a != "foo-mail";
        let s = sources(&[("mailto:", "foo-mail")], vec![], gone, NO_DESKTOP);
        assert_eq!(resolve(&mailto(), &s), Some(Handler::App("sola-mail".into())));
    }

    #[test]
    fn unclaimed_schemes_go_to_desktop_handlers_and_types_to_none() {
        let entry = Entry::new("c.desktop", "c %u");
        let e2 = entry.clone();
        let desk: &dyn Fn(&str) -> Option<Entry> = &move |s| (s == "claude-cli").then(|| e2.clone());
        let s = sources(&[], vec![], ALL, desk);
        assert_eq!(resolve(&Kind::Scheme("claude-cli".into()), &s), Some(Handler::Desktop(entry)));
        assert_eq!(resolve(&Kind::Type("zip".into()), &s), None);
        assert_eq!(resolve(&Kind::Type("png".into()), &s), Some(Handler::App("sola-paint".into())));
    }
}
