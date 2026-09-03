//! Page paste: compositor clipboard → CEF focused frame.
//!
//! Images go through data-control (`sola_kit::clipboard`), not iced.
//! A smithay text receive can drop the current offer; `frame.paste()`
//! is empty on windowless CEF and can replace the offer with nothing.

use std::sync::mpsc::Sender;

use iced::Task;
use sola_kit::clipboard::Offer;

use crate::engine::{Cmd, Engine};

/// Blocking data-control read on a worker thread.
pub fn read_task() -> Task<Offer> {
    Task::perform(
        async {
            tokio::task::spawn_blocking(sola_kit::clipboard::read_offer)
                .await
                .unwrap_or(Offer::Empty)
        },
        |offer| offer,
    )
}

/// Ship a non-empty offer to the engine. `Empty` is a no-op (caller may
/// fall back to iced text read).
pub fn send<E: Engine>(tx: &Sender<Cmd<E>>, offer: Offer) {
    match offer {
        Offer::Empty => {}
        Offer::Text(s) => {
            if let Some(s) = crate::util::usable_clipboard_text(Some(s)) {
                let _ = tx.send(Cmd::PasteText(s));
            }
        }
        Offer::Image {
            mime,
            bytes,
            filename,
        } => {
            let _ = tx.send(Cmd::PasteImage {
                mime,
                filename,
                bytes: bytes.to_vec(),
            });
        }
    }
}
