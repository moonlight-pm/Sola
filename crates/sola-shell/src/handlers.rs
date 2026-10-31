//! An app asking to be the handler for a link scheme or a file type
//! (`Topic::DefaultHandlerRequest`): a card above the desk cards asks you,
//! and only **Use** changes it (`docs/specs/2026-10-31-sola-open-design.md`).

use sola_bus::topics::DefaultHandlerRequest;
use sola_kit::sola_open::Kind;
use sola_kit::sola_open::resolve::{self, Handler, Sources};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Prompt {
    pub app_id: String,
    pub kind: Kind,
    /// The asking app's name.
    pub app: String,
    /// The name of what opens it now (empty: nothing does).
    pub now: String,
}

impl Prompt {
    pub fn question(&self) -> String {
        if self.now.is_empty() {
            format!("Open {} with {}?", self.kind, self.app)
        } else {
            format!("Open {} with {} instead of {}?", self.kind, self.app, self.now)
        }
    }

    pub fn keep_label(&self) -> String {
        if self.now.is_empty() {
            "Not Now".into()
        } else {
            format!("Keep {}", self.now)
        }
    }

    pub fn use_label(&self) -> String {
        format!("Use {}", self.app)
    }
}

/// The prompt for `req`, or `None` with nothing to ask: a kind it cannot
/// read, an app that does not claim it, or one that already opens it.
pub fn prompt_for(
    req: &DefaultHandlerRequest,
    sources: &Sources,
    label: &dyn Fn(&str) -> String,
) -> Option<Prompt> {
    let kind = Kind::from_key(&req.kind)?;
    if !resolve::claimants(&kind, sources).contains(&req.app_id) {
        tracing::warn!(app_id = %req.app_id, kind = %req.kind, "asked to open what it does not claim: no prompt");
        return None;
    }
    let now = match resolve::resolve(&kind, sources) {
        Some(Handler::App(id)) if id == req.app_id => return None,
        Some(Handler::App(id)) => label(&id),
        Some(Handler::Desktop(e)) => e.name,
        None => String::new(),
    };
    Some(Prompt {
        app_id: req.app_id.clone(),
        app: label(&req.app_id),
        kind,
        now,
    })
}

/// Your answer: `use_it` makes the asking app the handler.
pub fn answer(prompt: &Prompt, use_it: bool) {
    if !use_it {
        tracing::info!(app_id = %prompt.app_id, kind = %prompt.kind.key(), "kept the handler");
        return;
    }
    match sola_kit::sola_open::defaults::set(&prompt.kind, &prompt.app_id) {
        Ok(()) => tracing::info!(app_id = %prompt.app_id, kind = %prompt.kind.key(), "the handler is now this app"),
        Err(e) => tracing::warn!(app_id = %prompt.app_id, kind = %prompt.kind.key(), error = %e, "could not change the handler"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sola_kit::sola_open::claims::Claim;
    use sola_kit::sola_open::defaults::Defaults;

    fn all(_: &str) -> bool {
        true
    }

    fn sources(recorded: Vec<Claim>) -> Sources<'static> {
        Sources {
            defaults: Defaults::default(),
            builtin: sola_kit::sola_open::claims::builtin(),
            recorded,
            installed: &all,
            desktop: Vec::new(),
            mimeapps: "/nonexistent".into(),
        }
    }

    fn label(id: &str) -> String {
        match id {
            "sola-mail" => "Mail".into(),
            other => other.to_uppercase(),
        }
    }

    fn ask(app_id: &str, kind: &str) -> DefaultHandlerRequest {
        DefaultHandlerRequest {
            app_id: app_id.into(),
            kind: kind.into(),
        }
    }

    #[test]
    fn a_claimant_is_asked_about() {
        let s = sources(vec![Claim::new("foo", &["mailto"], &[])]);
        let p = prompt_for(&ask("foo", "mailto:"), &s, &label).unwrap();
        assert_eq!(p.question(), "Open mailto: links with FOO instead of Mail?");
        assert_eq!((p.keep_label(), p.use_label()), ("Keep Mail".into(), "Use FOO".into()));
    }

    #[test]
    fn no_prompt_for_the_handler_or_a_stranger() {
        let s = sources(vec![]);
        assert!(prompt_for(&ask("sola-mail", "mailto:"), &s, &label).is_none());
        assert!(prompt_for(&ask("foo", "mailto:"), &s, &label).is_none());
        assert!(prompt_for(&ask("foo", "mailto"), &s, &label).is_none());
    }
}
