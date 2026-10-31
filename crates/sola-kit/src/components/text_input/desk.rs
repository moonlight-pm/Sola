//! The field's desk menu (`crate::desk_menu`): a right-click asks the
//! shell for it — Edit by the field's state, and the vault in an editable
//! one — and the pick comes back here, to be applied as the keys would
//! apply it. No app code: every kit field has it.

use iced_core::clipboard::{self, Clipboard};
use iced_core::mouse;
use iced_core::text::{self, paragraph::Paragraph as _};
use iced_core::time::Instant;
use iced_core::{Layout, Shell};
use sola_bus::topics::DeskMenuContext;

use super::editor::Editor;
use super::{
    Catalog, Focus, State, TextInput, Value, alignment_offset, find_cursor_position,
    replace_paragraph,
};
use crate::desk_menu::{self, field, field::FieldOp};

impl<Message, Theme, Renderer> TextInput<'_, Message, Theme, Renderer>
where
    Message: Clone,
    Theme: Catalog,
    Renderer: text::Renderer,
{
    /// A right press. On the field: focus it, put the caret at the click
    /// (unless it lands in the selection) and ask for the desk menu.
    /// Elsewhere: the field lets go of focus, as a left press would have it.
    pub(super) fn desk_press(
        &self,
        state: &mut State<Renderer::Paragraph>,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        shell: &mut Shell<'_, Message>,
    ) {
        let Some(at) = cursor.position_over(layout.bounds()) else {
            if state.is_focused.take().is_some() {
                state.is_dragging = None;
                shell.request_redraw();
            }
            return;
        };
        let was_focused = state.is_focused.is_some();
        let now = Instant::now();
        state.is_focused = Some(Focus {
            updated_at: now,
            now,
            is_window_focused: true,
        });
        state.is_dragging = None;

        let text_bounds = layout.children().next().expect("text layout").bounds();
        let target = at.x
            - text_bounds.x
            - alignment_offset(text_bounds.width, state.value.raw().min_width(), self.alignment);
        let shown = if self.is_secure {
            self.value.secure()
        } else {
            self.value.clone()
        };
        let position = (target > 0.0)
            .then(|| find_cursor_position(text_bounds, &shown, state, target))
            .flatten()
            .unwrap_or(0);
        let in_selection = state
            .cursor
            .selection(&self.value)
            .is_some_and(|(start, end)| (start..=end).contains(&position));
        if !(was_focused && in_selection) {
            state.cursor.move_to(position);
        }

        let editable = self.on_input.is_some();
        let selection = state.cursor.selection(&self.value).is_some();
        let token = desk_menu::next_token();
        state.desk = Some(token);
        field::opened(token);
        let window_at = desk_menu::pointer().unwrap_or(at);
        desk_menu::open(
            token,
            window_at.x,
            window_at.y,
            DeskMenuContext {
                editable,
                secret: self.is_secure,
                selection,
                site: None,
            },
            Vec::new(),
            field::edit_rows(editable, self.is_secure, selection),
        );
        shell.request_redraw();
        shell.capture_event();
    }

    /// Apply the pick this field's desk menu brought, if one waits.
    pub(super) fn desk_apply(
        &mut self,
        state: &mut State<Renderer::Paragraph>,
        layout: Layout<'_>,
        renderer: &Renderer,
        clipboard: &mut dyn Clipboard,
        shell: &mut Shell<'_, Message>,
    ) {
        let Some(op) = state.desk.and_then(field::take) else {
            return;
        };
        state.desk = None;
        let selected = state
            .cursor
            .selection(&self.value)
            .map(|(start, end)| self.value.select(start, end).to_string());
        let mut changed = None;
        match op {
            FieldOp::Paste(text) => {
                if self.on_input.is_some() {
                    let text = text.or_else(|| clipboard.read(clipboard::Kind::Standard));
                    // As the key path: a field is one line.
                    let text: String = text.unwrap_or_default().chars().filter(|c| !c.is_control()).collect();
                    let mut editor = Editor::new(&mut self.value, &mut state.cursor);
                    editor.paste(Value::new(&text));
                    let value = editor.contents();
                    changed = Some(match &self.on_paste {
                        Some(on_paste) => on_paste(value),
                        None => (self.on_input.as_ref().expect("editable"))(value),
                    });
                }
            }
            FieldOp::Copy if !self.is_secure => {
                if let Some(text) = selected {
                    copy(text, clipboard);
                }
            }
            FieldOp::Cut if !self.is_secure => {
                if let (Some(on_input), Some(text)) = (&self.on_input, selected) {
                    copy(text, clipboard);
                    let mut editor = Editor::new(&mut self.value, &mut state.cursor);
                    editor.delete();
                    changed = Some(on_input(editor.contents()));
                }
            }
            FieldOp::SelectAll => state.cursor.select_all(&self.value),
            FieldOp::Insert(secret) => {
                if let Some(on_input) = &self.on_input {
                    let text = zeroize::Zeroizing::new(
                        secret
                            .as_str()
                            .chars()
                            .filter(|c| !c.is_control())
                            .collect::<String>(),
                    );
                    let mut editor = Editor::new(&mut self.value, &mut state.cursor);
                    editor.paste(Value::new(&text));
                    changed = Some(on_input(editor.contents()));
                }
            }
            FieldOp::Copy | FieldOp::Cut => {}
        }
        if let Some(message) = changed {
            shell.publish(message);
            replace_paragraph(
                renderer,
                state,
                layout,
                &self.value,
                self.font,
                self.size,
                self.line_height,
            );
        }
        if let Some(focus) = &mut state.is_focused {
            focus.updated_at = Instant::now();
        }
        shell.request_redraw();
    }
}

/// Put `text` on the clipboard from the compositor's side (the window may
/// not have had the keyboard back yet); iced's clipboard if that fails.
fn copy(text: String, clipboard: &mut dyn Clipboard) {
    if let Err(e) = crate::clipboard::write_text(text.clone()) {
        tracing::debug!(%e, "desk menu: data-control copy failed; using the window's clipboard");
        clipboard.write(clipboard::Kind::Standard, text);
    }
}
