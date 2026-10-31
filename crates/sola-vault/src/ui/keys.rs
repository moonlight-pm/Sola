//! Keys and menus: Escape backs out one layer at a time, arrows walk the
//! list, Tab walks the fields, Enter submits; every titlebar menu item
//! arrives here as its action id.

use iced::Task;
use iced::keyboard::key::{Named, Physical};
use iced::keyboard::{Key, Modifiers};
use sola_kit::components::text_input::MenuEdit;

use super::copy::CopyWhat;
use super::fields::Field;
use super::{App, Msg, Phase};
use crate::menus;
use crate::model::sort::Sort;

impl App {
    pub(super) fn on_key(
        &mut self,
        key: Key,
        _physical: Physical,
        mods: Modifiers,
        captured: bool,
    ) -> Task<Msg> {
        // The file panel takes its own keys.
        if self.picker.is_some() {
            return Task::none();
        }
        match key {
            Key::Named(Named::Escape) => self.escape(),
            Key::Named(Named::Tab) if !captured => {
                if mods.shift() {
                    iced::widget::operation::focus_previous()
                } else {
                    iced::widget::operation::focus_next()
                }
            }
            Key::Named(Named::ArrowUp | Named::ArrowDown)
                if !captured
                    && self.phase() == Phase::Unlocked
                    && self.sheet.is_none()
                    && self.edit.is_none()
                    && self.popup.is_none() =>
            {
                self.step(matches!(key, Key::Named(Named::ArrowDown)))
            }
            _ => Task::none(),
        }
    }

    /// Escape: a menu, a sheet, the edit, the re-prompt, the search.
    fn escape(&mut self) -> Task<Msg> {
        if self.popup.is_some() {
            self.popup = None;
        } else if self.sheet.is_some() {
            self.close_sheet();
        } else if self.edit.is_some() {
            return self.cancel_edit();
        } else if self.reprompt.is_some() {
            return self.update(Msg::RepromptCancel);
        } else if !self.query.is_empty() {
            self.query.clear();
        } else if self.signin.two_step.is_some() {
            self.signin.two_step = None;
        }
        Task::none()
    }

    /// Enter in a field.
    pub(super) fn on_submit(&mut self, field: Field) -> Task<Msg> {
        match field {
            Field::Email => Field::Password.focus(),
            Field::Password | Field::ServerUrl => self.sign_in(),
            Field::Otp | Field::EmailCode => self.verify_two_step(),
            Field::Unlock => self.do_unlock(),
            Field::Reprompt => self.submit_reprompt(),
            Field::FolderName => self.save_folder(),
            Field::ExportPassword => Field::ExportConfirm.focus(),
            Field::ExportConfirm => Field::ExportMaster.focus(),
            Field::ExportMaster => self.update(Msg::Export(super::ExportMsg::Go)),
            Field::Search => match self.shown_items().first() {
                Some(first) => {
                    let id = first.id.clone();
                    self.select(id)
                }
                None => Task::none(),
            },
            Field::Name
            | Field::Row(_)
            | Field::Uri(_)
            | Field::CustomName(_)
            | Field::CustomValue(_) => self.save(),
        }
    }

    pub(super) fn on_menu_action(&mut self, action: &str) -> Task<Msg> {
        let unlocked = self.phase() == Phase::Unlocked;
        match action {
            menus::UNDO | menus::REDO => Task::none(),
            menus::CUT => self.menu_edit(MenuEdit::Cut),
            menus::COPY => self.menu_edit(MenuEdit::Copy),
            menus::PASTE => self.menu_paste(),
            menus::SELECT_ALL => self.menu_edit(MenuEdit::SelectAll),
            menus::FIND => self.update(Msg::SearchFocus),
            menus::QUIT => self.quit(),
            menus::SETTINGS => self.update(Msg::OpenSettings),
            menus::LOCK => self.lock(),
            menus::SIGN_OUT => self.ask_sign_out(),
            _ if !unlocked => Task::none(),
            menus::SYNC => self.update(Msg::Sync),
            menus::NEW_ITEM => {
                let kind = self.section.new_kind();
                self.new_item(kind)
            }
            menus::NEW_FOLDER => self.new_folder(),
            menus::GENERATOR => self.open_generator(None),
            menus::EXPORT => self.open_export(),
            menus::EDIT_ITEM => self.begin_edit(),
            menus::COPY_USERNAME => self.copy_from_menu(CopyWhat::Field("username".into())),
            menus::COPY_PASSWORD => self.copy_from_menu(CopyWhat::Field("password".into())),
            menus::COPY_CODE => self.copy_from_menu(CopyWhat::Code),
            menus::FAVORITE => self.toggle_favorite(),
            menus::CLONE => self.clone_item(),
            menus::MOVE_TO_FOLDER => self.ask_move(),
            menus::MOVE_TO_ORG => self.ask_share(),
            menus::TRASH => self.trash(),
            menus::SORT_NAME => self.update(Msg::SortBy(Sort::Name)),
            menus::SORT_RECENT => self.update(Msg::SortBy(Sort::Recent)),
            _ => Task::none(),
        }
    }
}
