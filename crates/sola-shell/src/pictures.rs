//! Each background picture keeps the look it was worn with
//! (`Appearance.pictures`): when the picture changes, the shell keeps the
//! outgoing one's look and puts the incoming one's back, so going back to a
//! picture brings back its theme. Only for a picture set while this shell
//! runs: the one replayed at its start is the picture already worn.

use std::time::{SystemTime, UNIX_EPOCH};

use sola_bus::topics::{Appearance, Background};

pub struct Pictures {
    /// The last `Topic::Appearance` (the defaults once retracted); `None`
    /// until the first.
    pub look: Option<Appearance>,
    /// When this shell started (unix ms): a picture set before it is the
    /// one already worn.
    since_ms: u64,
}

impl Default for Pictures {
    fn default() -> Self {
        Self {
            look: None,
            since_ms: now_ms(),
        }
    }
}

impl Pictures {
    /// The picture goes from `old` to `new`: the settings to emit, with the
    /// outgoing picture's look kept and the incoming one's worn; `None`
    /// when nothing changes.
    pub fn changed(&self, old: Option<&Background>, new: Option<&Background>) -> Option<Appearance> {
        let path = |b: Option<&Background>| b.map(|b| b.path.clone());
        if path(old) == path(new) || new.is_some_and(|b| b.set_at < self.since_ms) {
            return None;
        }
        swap(self.look.as_ref()?, path(old).as_deref(), path(new).as_deref())
    }
}

/// `look` with the picture `old` keeping what it was worn with, and
/// `new`'s kept look worn; `None` when that changes nothing.
fn swap(look: &Appearance, old: Option<&str>, new: Option<&str>) -> Option<Appearance> {
    let mut out = look.clone();
    if let Some(old) = old {
        out.pictures.insert(old.to_string(), look.picture_look());
    }
    if let Some(kept) = new.and_then(|new| out.pictures.get(new)).cloned() {
        out.wear(&kept);
    }
    (out != *look).then_some(out)
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_millis() as u64)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn picture(path: &str, set_at: u64) -> Background {
        Background {
            path: path.into(),
            set_at,
        }
    }

    fn shell(look: Appearance) -> Pictures {
        Pictures {
            look: Some(look),
            since_ms: 100,
        }
    }

    #[test]
    fn going_back_to_a_picture_brings_back_its_look() {
        let a = picture("/bg/a.jpg", 200);
        let b = picture("/bg/b.jpg", 300);
        let on_a = Appearance {
            adapt_theme: true,
            variation: 2,
            ..Appearance::default()
        };
        // A to B, a picture never worn: the look stays, A keeps it.
        let p = shell(on_a.clone());
        let to_b = p.changed(Some(&a), Some(&b)).expect("A's look kept");
        assert_eq!(to_b.variation, 2);
        assert_eq!(to_b.pictures["/bg/a.jpg"], on_a.picture_look());

        // On B the look changes; back to A brings A's back.
        let on_b = Appearance {
            adapt_theme: false,
            variation: 0,
            ..to_b
        };
        let p = shell(on_b.clone());
        let back = p.changed(Some(&b), Some(&picture("/bg/a.jpg", 400))).unwrap();
        assert!(back.adapt_theme);
        assert_eq!(back.variation, 2);
        assert_eq!(back.pictures["/bg/b.jpg"], on_b.picture_look());
        // The rest of the settings stay.
        assert_eq!(back.opacity, on_b.opacity);
    }

    #[test]
    fn the_picture_replayed_at_start_changes_nothing() {
        let mut look = Appearance::default();
        look.pictures.insert(
            "/bg/a.jpg".into(),
            Appearance {
                variation: 3,
                ..Appearance::default()
            }
            .picture_look(),
        );
        let p = shell(look);
        assert_eq!(p.changed(None, Some(&picture("/bg/a.jpg", 50))), None);
        // The same picture again: nothing.
        let a = picture("/bg/a.jpg", 200);
        assert_eq!(p.changed(Some(&a), Some(&a)), None);
        // Cleared: A keeps its look.
        let cleared = p.changed(Some(&a), None).unwrap();
        assert_eq!(cleared.pictures["/bg/a.jpg"], Appearance::default().picture_look());
    }
}
