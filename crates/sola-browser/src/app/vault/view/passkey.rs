//! When a site asks: sign in with a passkey, or save one. The site is
//! the head; rows are the accounts. Enter signs in, Escape hands the page
//! its error so it can offer another way.

use iced::widget::column;
use iced::Length;
use sola_kit::components::button as kit_button;
use sola_kit::components::scroll;

use super::super::update::VaultMsg;
use super::parts::{self, Trailing, caps, icon_button, ink2, item_row, item_row_parts, label};
use super::{Head, LIST_MAX_H, Sheet, actions, body, foot_line};
use crate::app::App;
use crate::engine::Engine;
use crate::vault::{create_account_hint, log_host};

impl<E: Engine> App<E> {
    fn passkey_head(&self, title: impl Fn(&str) -> String, sub: String) -> Head<'_> {
        let host = self
            .vault
            .passkey
            .as_ref()
            .map(|p| log_host(&p.req.origin))
            .filter(|h| !h.is_empty())
            .unwrap_or_else(|| "this site".into());
        Head::new(title(&host))
            .plate(parts::letter_plate(&host, 22.0))
            .sub(sub)
            .button(icon_button("x", 15, Some(VaultMsg::PasskeyCancel)))
    }

    pub(super) fn sheet_passkey_pick(&self) -> Sheet<'_> {
        let v = &self.vault;
        let head = self.passkey_head(|h| format!("Sign in to {h}"), "with a passkey".into());
        let Some(p) = v.passkey.as_ref() else {
            return Sheet {
                head,
                body: body(label("The site stopped asking.", 12.0, ink2), true),
                foot: None,
            };
        };
        let mut col = column![].spacing(parts::ROW_GAP).width(Length::Fill);
        for e in [v.error.as_ref(), p.error.as_ref()].into_iter().flatten() {
            col = col.push(parts::error_line(e));
        }
        if p.loading {
            col = col.push(label("Looking up passkeys…", 12.0, ink2));
        } else if p.candidates.is_empty() {
            let cancel = kit_button::labeled_sm("Cancel", kit_button::secondary)
                .on_press(parts::msg(VaultMsg::PasskeyCancel));
            col = col.push(parts::empty(
                "fingerprint-pattern",
                "No passkey saved for this site",
                "Cancel and the site offers its other ways to sign in.",
                Some(cancel.into()),
            ));
        } else {
            for (i, c) in p.candidates.iter().enumerate() {
                let on = i == v.list.chosen;
                let who = c
                    .username
                    .clone()
                    .or_else(|| c.user_display_name.clone())
                    .filter(|s| !s.is_empty())
                    .unwrap_or_else(|| c.rp_id.clone());
                col = col.push(item_row_parts(
                    parts::kind_plate("fingerprint-pattern", 28.0),
                    who,
                    c.name.clone(),
                    false,
                    false,
                    on,
                    if on && !v.busy {
                        Trailing::Hint("Sign in")
                    } else {
                        Trailing::None
                    },
                    (!v.busy).then(|| VaultMsg::PasskeyPick(c.cipher_id.clone())),
                ));
            }
        }
        let foot = foot_line(
            iced::widget::Space::new().width(0).into(),
            if v.busy {
                "Signing in…".into()
            } else {
                "The site is waiting".into()
            },
            Some("Esc cancels".into()),
        );
        Sheet {
            head,
            body: body(
                iced::widget::container(scroll::scrollable(col).width(Length::Fill))
                    .max_height(LIST_MAX_H),
                false,
            ),
            foot: Some(foot),
        }
    }

    pub(super) fn sheet_passkey_create(&self) -> Sheet<'_> {
        let v = &self.vault;
        let account = v
            .passkey
            .as_ref()
            .and_then(|p| create_account_hint(&p.req.public_key_json));
        let head = self.passkey_head(
            |h| format!("Save a passkey for {h}"),
            account.map(|a| format!("for {a}")).unwrap_or_default(),
        );
        let mut col = column![].spacing(parts::ROW_GAP).width(Length::Fill);
        let p_error = v.passkey.as_ref().and_then(|p| p.error.as_ref());
        for e in [v.error.as_ref(), p_error].into_iter().flatten() {
            col = col.push(parts::error_line(e));
        }
        let onto = !v.site_logins.is_empty();
        if v.site_logins_loading {
            col = col.push(label("Looking up this site's logins…", 12.0, ink2));
        } else if onto {
            col = col.push(caps("Add it to a login here", None));
            for (i, item) in v.site_logins.iter().enumerate() {
                let on = i == v.list.chosen;
                let mut it = item.clone();
                if it.has_passkey {
                    it.subtitle = format!("{} · has one", it.subtitle);
                }
                col = col.push(item_row(
                    &it,
                    on,
                    if on && !v.busy {
                        Trailing::Hint("Add")
                    } else {
                        Trailing::None
                    },
                    (!v.busy).then(|| VaultMsg::PasskeyOnto(item.id.clone())),
                ));
            }
        }
        let cancel = kit_button::labeled("Cancel", kit_button::ghost)
            .on_press(parts::msg(VaultMsg::PasskeyCancel));
        let word = if v.busy {
            "Saving…"
        } else if onto {
            "Save as a new login"
        } else {
            "Save passkey"
        };
        let mut new = kit_button::labeled(
            word,
            if onto {
                kit_button::secondary
            } else {
                kit_button::primary
            },
        );
        if !v.busy {
            new = new.on_press(parts::msg(VaultMsg::PasskeyNew));
        }
        Sheet {
            head,
            body: body(
                iced::widget::container(scroll::scrollable(col).width(Length::Fill))
                    .max_height(LIST_MAX_H),
                false,
            ),
            foot: Some(actions(vec![cancel.into(), new.into()], true)),
        }
    }
}
