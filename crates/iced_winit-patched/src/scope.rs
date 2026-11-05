//! Which windows a message changes (Sola's patch).
//!
//! After an update iced rebuilds and redraws every window, because any `view`
//! could read what changed. A program with many windows, and busy messages
//! that concern one of them, can say so while it handles the message, with
//! [`touch`] or [`touch_none`].
//!
//! A message that says nothing changes every window, as in stock iced, so a
//! program that never calls these behaves as it always did. A window no
//! message touched keeps its widget state and is built again when it next
//! gets an event or a redraw.

use std::cell::RefCell;

use crate::core::window;

/// The message being handled changes `window`, and no window it does not also
/// touch. Call it during `update`.
pub fn touch(window: window::Id) {
    BATCH.with_borrow_mut(|batch| {
        batch.declared = true;
        if !batch.touched.contains(&window) {
            batch.touched.push(window);
        }
    });
}

/// The message being handled changes no window, unless it also [`touch`]es one.
/// Call it during `update`.
pub fn touch_none() {
    BATCH.with_borrow_mut(|batch| batch.declared = true);
}

/// What an update changed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Scope {
    All,
    Only(Vec<window::Id>),
}

impl Scope {
    pub(crate) fn includes(&self, window: window::Id) -> bool {
        match self {
            Scope::All => true,
            Scope::Only(windows) => windows.contains(&window),
        }
    }
}

#[derive(Default)]
struct Batch {
    /// Messages handled since the last [`take`].
    handled: usize,
    /// One of them said nothing.
    all: bool,
    touched: Vec<window::Id>,
    /// The message being handled has said something.
    declared: bool,
}

thread_local! {
    static BATCH: RefCell<Batch> = RefCell::default();
}

pub(crate) fn begin_message() {
    BATCH.with_borrow_mut(|batch| batch.declared = false);
}

pub(crate) fn end_message() {
    BATCH.with_borrow_mut(|batch| {
        batch.handled += 1;
        batch.all |= !batch.declared;
    });
}

/// What the messages since the last call changed. No message at all (a widget
/// asked for a rebuild) is every window.
pub(crate) fn take() -> Scope {
    let batch = BATCH.with_borrow_mut(std::mem::take);
    if batch.handled == 0 || batch.all {
        Scope::All
    } else {
        Scope::Only(batch.touched)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn message(say: impl FnOnce()) {
        begin_message();
        say();
        end_message();
    }

    #[test]
    fn a_message_that_says_nothing_changes_every_window() {
        let a = window::Id::unique();
        message(|| touch(a));
        message(|| {});
        assert_eq!(take(), Scope::All);
    }

    #[test]
    fn no_message_is_every_window() {
        assert_eq!(take(), Scope::All);
    }

    #[test]
    fn what_messages_touch_adds_up_and_is_taken_once() {
        let (a, b) = (window::Id::unique(), window::Id::unique());
        message(|| touch(a));
        message(touch_none);
        message(|| {
            touch_none();
            touch(b);
            touch(a);
        });
        let scope = take();
        assert_eq!(scope, Scope::Only(vec![a, b]));
        assert!(scope.includes(a) && !scope.includes(window::Id::unique()));
        assert_eq!(take(), Scope::All);
    }

    #[test]
    fn touching_nothing_is_no_window() {
        message(touch_none);
        assert_eq!(take(), Scope::Only(vec![]));
    }
}
