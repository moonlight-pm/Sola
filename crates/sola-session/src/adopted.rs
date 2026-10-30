//! Apps a restarted sola-session finds already running: the saved session
//! lists them and they have windows, but this process did not start them,
//! so it has no process to watch. They stay in `SessionApps` while they have
//! windows; once their windows have been gone for [`GRACE`] they closed, or
//! (when their compositor changed) went with their display.

use std::collections::HashSet;
use std::time::{Duration, Instant};

/// How long an adopted app's windows must be gone before it counts as gone:
/// a restarting sola-river can publish the window list in pieces.
pub const GRACE: Duration = Duration::from_secs(3);

pub struct Adopted {
    pub app_id: String,
    pub command: String,
    /// The compositor it was found on.
    pub display: Option<String>,
    /// Since when it has had no window.
    pub missing_since: Option<Instant>,
}

/// A live window list (`Windows`, not a retraction): note which adopted
/// apps have none.
pub fn note_windows(adopted: &mut [Adopted], open: &HashSet<String>, now: Instant) {
    for a in adopted {
        if open.contains(&a.app_id) {
            a.missing_since = None;
        } else {
            a.missing_since.get_or_insert(now);
        }
    }
}

/// Take out the adopted apps whose windows have been gone for [`GRACE`].
pub fn gone(adopted: &mut Vec<Adopted>, now: Instant) -> Vec<Adopted> {
    let (gone, kept): (Vec<_>, Vec<_>) = std::mem::take(adopted)
        .into_iter()
        .partition(|a| a.missing_since.is_some_and(|t| now.duration_since(t) >= GRACE));
    *adopted = kept;
    gone
}

#[cfg(test)]
mod tests {
    use super::*;

    fn app(id: &str) -> Adopted {
        Adopted {
            app_id: id.into(),
            command: format!("/bin/{id}"),
            display: Some("1-1".into()),
            missing_since: None,
        }
    }

    #[test]
    fn an_adopted_app_is_gone_only_after_its_windows_stay_gone() {
        let t0 = Instant::now();
        let mut adopted = vec![app("sola-mail"), app("sola-browser")];
        let open: HashSet<String> = ["sola-browser".to_string()].into();
        note_windows(&mut adopted, &open, t0);
        assert!(gone(&mut adopted, t0 + Duration::from_secs(1)).is_empty(), "within the grace");
        // A list in pieces: mail is back before the grace ends.
        let all: HashSet<String> = ["sola-browser".into(), "sola-mail".into()].into();
        note_windows(&mut adopted, &all, t0 + Duration::from_secs(2));
        assert!(gone(&mut adopted, t0 + GRACE * 2).is_empty());
        note_windows(&mut adopted, &open, t0 + GRACE * 2);
        let out = gone(&mut adopted, t0 + GRACE * 3);
        assert_eq!(out.iter().map(|a| a.app_id.as_str()).collect::<Vec<_>>(), ["sola-mail"]);
        assert_eq!(adopted.len(), 1);
    }
}
