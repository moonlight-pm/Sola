//! The launcher's Browser field, as the shell runs it: entering it from
//! the launcher's Browser row (with the browser's history read once), and
//! opening a pick in a new page window — through `sola_core::open_url`,
//! which hands it to a running browser or starts one. The field's data and
//! rows are [`crate::launcher::web`].

use super::{Msg, Shell};
use crate::launcher::web::{APP_ID, WebChooser};

impl Shell {
    /// The launcher's Browser row: an address or search field instead of
    /// a blank page.
    pub(crate) fn enter_web_chooser(&mut self) -> iced::Task<Msg> {
        self.launcher.web = Some(WebChooser::new(sola_core::omnibox::history::load()));
        self.launcher.query.clear();
        self.launcher.selected = 0;
        iced::widget::operation::focus::<Msg>(iced::widget::Id::new(
            crate::launcher::view::QUERY_INPUT_ID,
        ))
    }

    /// Open Browser-field row `i` (Return on it, or a click): the launcher
    /// closes, the page opens in a new window, and its ghost row stands
    /// where it will land.
    pub(crate) fn web_pick(&mut self, i: usize) -> iced::Task<Msg> {
        let Some(web) = self.launcher.web.as_ref() else {
            return iced::Task::none();
        };
        let Some(url) = web.rows().get(i).and_then(|row| web.url(row)) else {
            return iced::Task::none();
        };
        tracing::info!(%url, "launcher: open in the browser");
        // A socket write to a running browser, or a spawn: off the iced thread.
        std::thread::spawn(move || {
            if let Err(e) = sola_core::open_url::open(&url) {
                tracing::warn!(%url, "launcher: open failed: {e}");
            }
        });
        let label = self
            .applications
            .get(APP_ID)
            .map(|a| a.label.clone())
            .unwrap_or_else(|| "Browser".into());
        let opening = self.begin_opening(APP_ID, &label);
        iced::Task::batch([iced::Task::done(Msg::CloseLauncher), opening])
    }
}
