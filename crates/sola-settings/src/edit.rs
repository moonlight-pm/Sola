//! Edit menu helpers. Super+X/C/V/A are stolen by River + the shell;
//! they arrive as `Topic::MenuAction` and must be applied here.

use iced::advanced::widget::operate;
use iced::advanced::widget::operation::{Operation, Outcome, focusable::Focusable};
use iced::widget::Id;
use iced::{Rectangle, Task};

pub fn find_focused_id() -> Task<Option<Id>> {
    operate(FindFocused { id: None })
}

struct FindFocused {
    id: Option<Id>,
}

impl Operation<Option<Id>> for FindFocused {
    fn traverse(&mut self, operate: &mut dyn FnMut(&mut dyn Operation<Option<Id>>)) {
        operate(self);
    }

    fn focusable(&mut self, id: Option<&Id>, _bounds: Rectangle, state: &mut dyn Focusable) {
        if state.is_focused() {
            self.id = id.cloned();
        }
    }

    fn finish(&self) -> Outcome<Option<Id>> {
        Outcome::Some(self.id.clone())
    }
}

pub fn select_all(id: Id) -> Task<()> {
    operate(iced::advanced::widget::operation::text_input::select_all(
        id,
    ))
}
