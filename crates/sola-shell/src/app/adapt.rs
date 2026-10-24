//! The theme following the background picture: the bus inputs into
//! [`crate::adapt::Adapt`], and its steps run (a sample off the UI
//! thread, a `Topic::Theme` emit).

use sola_bus::topics::{Appearance, Background, Topic};
use sola_core::theme::Scheme;

use super::{Msg, Shell, with_bus};
use crate::adapt::{Step, sample};

impl Shell {
    /// `Topic::Background` (`None`: retracted).
    pub(crate) fn adapt_picture(&mut self, picture: Option<Background>) -> iced::Task<Msg> {
        self.adapt.picture = picture;
        self.adapt_step()
    }

    /// `Topic::Appearance` (`None`: retracted, the defaults).
    pub(crate) fn adapt_appearance(&mut self, look: Option<Appearance>) -> iced::Task<Msg> {
        let look = look.unwrap_or_default();
        let strength = look.strength.min(100) as f32 / 100.0;
        let restrength = strength != self.adapt.strength;
        self.adapt.on = look.adapt_theme;
        self.adapt.strength = strength;
        self.adapt.variation = look.variation as usize;
        if restrength {
            self.publish_variations();
        }
        self.adapt_step()
    }

    /// A sample of `picture` finished.
    pub(crate) fn adapt_sampled(
        &mut self,
        picture: Background,
        schemes: Result<Vec<Scheme>, String>,
    ) -> iced::Task<Msg> {
        if let Ok(s) = &schemes {
            tracing::info!(path = %picture.path, schemes = ?s, "adapt: picture sampled");
        }
        self.adapt.sampled(picture, schemes);
        self.publish_variations();
        self.adapt_step()
    }

    /// Tell Settings the picture's schemes (`Topic::ThemeVariations`).
    fn publish_variations(&self) {
        if let Some(v) = self.adapt.variations() {
            with_bus(|bus| {
                let _ = bus.emit(Topic::ThemeVariations(v));
            });
        }
    }

    /// Run what the adapter decides now.
    pub(crate) fn adapt_step(&mut self) -> iced::Task<Msg> {
        match self.adapt.step() {
            Step::Nothing => iced::Task::none(),
            Step::Emit(theme) => {
                let from_picture = self.adapt.on && self.adapt.picture.is_some();
                tracing::info!(from_picture, "adapt: theme colors rewritten");
                with_bus(|bus| {
                    let _ = bus.emit(Topic::Theme(theme));
                });
                iced::Task::none()
            }
            Step::Sample(picture) => {
                let path = std::path::PathBuf::from(&picture.path);
                iced::Task::perform(
                    async move {
                        let (tx, rx) = tokio::sync::oneshot::channel();
                        std::thread::spawn(move || {
                            let _ = tx.send(sample(&path));
                        });
                        let scheme = rx
                            .await
                            .unwrap_or_else(|_| Err("sample worker dropped".into()));
                        (picture, scheme)
                    },
                    |(picture, scheme)| Msg::AdaptSampled(picture, scheme),
                )
            }
        }
    }
}
