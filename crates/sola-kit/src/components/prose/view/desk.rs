//! The letter's desk menu (`crate::desk_menu`): a right-click asks the
//! shell for it — Open Link and Copy Link over a link (or a linked
//! picture), Copy with a selection, Select All — and the pick comes back
//! here. No app code: every kit letter has it.

use iced::Point;
use iced::advanced::text;
use iced::advanced::{Clipboard, Shell};
use sola_bus::topics::{DeskEdit, DeskMenuContext};

use super::{ProseView, State, has_range, publish_sel, selected_visible};
use crate::desk_menu::{self, field, field::FieldOp};

const OPEN_LINK: &str = "prose.open-link";
const COPY_LINK: &str = "prose.copy-link";

impl<Message> ProseView<'_, Message> {
    /// A right press at `at` on the letter: ask for the menu. The
    /// selection stays as it is.
    pub(super) fn desk_press<P: text::Paragraph>(
        &self,
        state: &mut State<P>,
        at: Point,
        shell: &mut Shell<'_, Message>,
    ) {
        let link = state.hovered_link.clone();
        let selection = has_range(state.sel);
        let token = desk_menu::next_token();
        field::opened(token);
        let items = match link {
            Some(_) => vec![
                desk_menu::item(OPEN_LINK, "Open Link", true),
                desk_menu::item(COPY_LINK, "Copy Link", true),
            ],
            None => Vec::new(),
        };
        state.desk = Some((token, link));
        let mut edit = Vec::with_capacity(2);
        if selection {
            edit.push(DeskEdit::Copy);
        }
        edit.push(DeskEdit::SelectAll);
        let window_at = desk_menu::pointer().unwrap_or(at);
        desk_menu::open(
            token,
            window_at.x,
            window_at.y,
            DeskMenuContext {
                editable: false,
                secret: false,
                selection,
                site: None,
            },
            items,
            edit,
        );
        shell.capture_event();
    }

    /// Apply the pick this letter's desk menu brought, if one waits.
    pub(super) fn desk_apply<P: text::Paragraph>(
        &self,
        state: &mut State<P>,
        clipboard: &mut dyn Clipboard,
        shell: &mut Shell<'_, Message>,
    ) {
        let Some(op) = state.desk.as_ref().and_then(|(token, _)| field::take(*token)) else {
            return;
        };
        let link = state.desk.take().and_then(|(_, link)| link);
        match op {
            FieldOp::Action(id) if id == OPEN_LINK => {
                if let Some(url) = link {
                    shell.publish((self.on_link)(url));
                }
            }
            FieldOp::Action(id) if id == COPY_LINK => {
                if let Some(url) = link {
                    desk_menu::copy_in(url, clipboard);
                }
            }
            FieldOp::Copy => {
                if let Some((a, b)) = state.sel.filter(|s| has_range(Some(*s))) {
                    desk_menu::copy_in(selected_visible(&self.blocks, a, b), clipboard);
                }
            }
            FieldOp::SelectAll => {
                if !state.doc.is_empty() {
                    state.sel = Some((0, state.doc.len()));
                    publish_sel(self, state, shell);
                    shell.invalidate_layout();
                }
            }
            _ => {}
        }
        shell.request_redraw();
    }
}
