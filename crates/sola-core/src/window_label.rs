//! A window title in parts: the project, the session in it (when it is not
//! the project's main one) and the computer it runs on. Projects writes its
//! windows' titles with [`WindowLabel::title`]; the shell's tab bar reads
//! them back with [`WindowLabel::parse`] and draws each part on its own (the
//! host as a chip in its color, [`crate::host_color`], unless it is this
//! computer: [`WindowLabel::is_here`]).
//!
//! The xdg title is the one thing that crosses from an app to the shell,
//! so the parts ride in it: `Oath › fix login | novus`. The host is split
//! off the right and the session at the first `›`, so a session's title
//! may hold any text; that takes every title having a host, this
//! computer's too. Only a Projects window's title is read this way: a
//! page titled `Foo | Bar` elsewhere is just text.

const HOST_SEP: &str = " | ";
const SESSION_SEP: &str = " › ";

/// This computer's hostname (what its prompt names).
pub fn this_host() -> &'static str {
    // Read once: every window's title asks, on every update.
    static HOST: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    HOST.get_or_init(|| {
        std::fs::read_to_string("/proc/sys/kernel/hostname")
            .map(|h| h.trim().to_string())
            .ok()
            .filter(|h| !h.is_empty())
            .unwrap_or_else(|| "here".into())
    })
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WindowLabel {
    pub project: String,
    /// The session's title (or name); `None` for the project's main one.
    pub session: Option<String>,
    pub host: String,
}

impl WindowLabel {
    /// The xdg title carrying these parts.
    pub fn title(&self) -> String {
        format!("{}{HOST_SEP}{}", self.head(), self.host)
    }

    /// The name a person reads: [`Self::title`], less the host when it is
    /// this computer.
    pub fn shown(&self) -> String {
        if self.is_here() {
            self.head()
        } else {
            self.title()
        }
    }

    /// It runs on this computer, so its host goes unnamed.
    pub fn is_here(&self) -> bool {
        self.host == this_host()
    }

    fn head(&self) -> String {
        match &self.session {
            Some(s) => format!("{}{SESSION_SEP}{s}", self.project),
            None => self.project.clone(),
        }
    }

    /// The parts of a title written by [`Self::title`]; `None` when it has
    /// no host part.
    pub fn parse(title: &str) -> Option<Self> {
        let (head, host) = title.trim().rsplit_once(HOST_SEP)?;
        let host = host.trim();
        if host.is_empty() || host.contains(char::is_whitespace) {
            return None;
        }
        let (project, session) = match head.split_once(SESSION_SEP) {
            Some((p, s)) => (p.trim(), Some(s.trim()).filter(|s| !s.is_empty())),
            None => (head.trim(), None),
        };
        if project.is_empty() {
            return None;
        }
        Some(Self {
            project: project.to_string(),
            session: session.map(str::to_string),
            host: host.to_string(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn label(project: &str, session: Option<&str>, host: &str) -> WindowLabel {
        WindowLabel {
            project: project.into(),
            session: session.map(Into::into),
            host: host.into(),
        }
    }

    #[test]
    fn round_trips() {
        for l in [
            label("Oath", None, "novus"),
            label("Oath", Some("fix login"), "buccina"),
            label("Sola", Some("a › b | c"), "canto"),
        ] {
            assert_eq!(WindowLabel::parse(&l.title()), Some(l));
        }
        assert_eq!(label("Oath", Some("fix login"), "novus").title(), "Oath › fix login | novus");
    }

    #[test]
    fn this_computer_goes_unnamed() {
        let here = label("Oath", Some("fix login"), this_host());
        assert!(here.is_here());
        assert_eq!(here.shown(), "Oath › fix login");
        // Still in the title: the shell parses by it.
        assert_eq!(WindowLabel::parse(&here.title()), Some(here));
        let there = label("Oath", None, "not-this-computer");
        assert!(!there.is_here());
        assert_eq!(there.shown(), "Oath | not-this-computer");
    }

    #[test]
    fn plain_titles_do_not_parse() {
        assert_eq!(WindowLabel::parse("Oath"), None);
        assert_eq!(WindowLabel::parse("Oath | "), None);
        assert_eq!(WindowLabel::parse(" | novus"), None);
        assert_eq!(WindowLabel::parse("Docs | Example Site"), None);
    }
}
