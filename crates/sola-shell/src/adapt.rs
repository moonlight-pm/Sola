//! The theme's colors following the background picture (Settings →
//! Appearance, `Topic::Appearance.adapt_theme`). The shell owns it because
//! it always runs: when on and a picture is set, it samples the picture
//! ([`sample`], off the UI thread) and writes the adapted colors into
//! `Topic::Theme` (`sola_core::theme::Theme::adapt`), so every app follows
//! as it follows any theme edit. Off, or the picture cleared, puts the
//! seed's greys back. Spec: `docs/specs/2026-10-24-background-design.md`
//! § Adapting the theme.

use std::path::Path;

use sola_bus::topics::{Background, ThemeVariation, ThemeVariations};
use sola_core::theme::{Scheme, Theme as BusTheme};

/// The longest side of the picture as sampled (px).
const SAMPLE_SIDE: u32 = 96;

pub struct Adapt {
    /// The last `Topic::Theme`.
    pub theme: Option<BusTheme>,
    /// `Appearance.adapt_theme`.
    pub on: bool,
    /// `Appearance.strength`, 0–1.
    pub strength: f32,
    /// `Appearance.variation`: which of the picture's schemes.
    pub variation: usize,
    /// `Appearance.colors`: your own, over the rest.
    pub colors: std::collections::BTreeMap<String, String>,
    /// `Topic::Background`.
    pub picture: Option<Background>,
    /// The picture's schemes, once sampled (the dominant first).
    schemes: Option<(Background, Vec<Scheme>)>,
    /// The picture being sampled (or whose sampling failed).
    sampling: Option<Background>,
    /// The theme carries adapted colors, so going off puts the greys back.
    in_effect: bool,
}

impl Default for Adapt {
    fn default() -> Self {
        Self {
            theme: None,
            on: false,
            strength: sola_bus::topics::Appearance::DEFAULT_STRENGTH as f32 / 100.0,
            variation: 0,
            colors: Default::default(),
            picture: None,
            schemes: None,
            sampling: None,
            in_effect: false,
        }
    }
}

/// What the shell should do next.
#[derive(Debug, PartialEq)]
pub enum Step {
    /// Sample this picture ([`sample`]), then [`Adapt::sampled`].
    Sample(Background),
    /// Emit this theme.
    Emit(BusTheme),
    Nothing,
}

impl Adapt {
    /// Decide, after any of the inputs changed. The colors the shell
    /// manages are made from scratch each time: the seed's greys, the
    /// picture's scheme over them when adapting, your colors over that.
    pub fn step(&mut self) -> Step {
        let Some(theme) = self.theme.clone() else {
            return Step::Nothing;
        };
        let picture = self.picture.clone().filter(|_| self.on);
        if picture.is_none() && self.colors.is_empty() {
            if !self.in_effect {
                return Step::Nothing;
            }
            self.in_effect = false;
            let mut grey = theme;
            if grey.unadapt() {
                self.theme = Some(grey.clone());
                return Step::Emit(grey);
            }
            return Step::Nothing;
        }
        let mut desired = theme.clone();
        desired.unadapt();
        if let Some(picture) = picture {
            match &self.schemes {
                Some((sampled, schemes)) if *sampled == picture && !schemes.is_empty() => {
                    let scheme = schemes[self.variation.min(schemes.len() - 1)];
                    desired.adapt(&scheme, self.strength);
                }
                _ if self.sampling.as_ref() != Some(&picture) => {
                    self.sampling = Some(picture.clone());
                    return Step::Sample(picture);
                }
                // Sampling (or it failed): wait.
                _ => return Step::Nothing,
            }
        }
        desired.customize(&self.colors);
        self.in_effect = true;
        if desired == theme {
            return Step::Nothing;
        }
        self.theme = Some(desired.clone());
        Step::Emit(desired)
    }

    /// The picture's schemes as Settings shows them, at the current
    /// strength; `None` until sampled.
    pub fn variations(&self) -> Option<ThemeVariations> {
        let (picture, schemes) = self.schemes.as_ref()?;
        Some(ThemeVariations {
            path: picture.path.clone(),
            choices: schemes
                .iter()
                .map(|s| {
                    let [base, raised, accent] = s.preview(self.strength);
                    ThemeVariation {
                        name: s.name().into(),
                        base,
                        raised,
                        accent,
                        source: s.source_hex(),
                    }
                })
                .collect(),
        })
    }

    /// A sample finished. An error leaves the picture marked sampled, so it
    /// is not retried until the picture changes.
    pub fn sampled(&mut self, picture: Background, schemes: Result<Vec<Scheme>, String>) {
        match schemes {
            Ok(schemes) => {
                if self.sampling.as_ref() == Some(&picture) {
                    self.sampling = None;
                }
                self.schemes = Some((picture, schemes));
            }
            Err(e) => tracing::warn!(path = %picture.path, %e, "adapt: sample the picture"),
        }
    }
}

/// Decode the picture small and read its schemes. Blocking.
pub fn sample(path: &Path) -> Result<Vec<Scheme>, String> {
    let img = image::open(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let small = img.thumbnail(SAMPLE_SIDE, SAMPLE_SIDE).to_rgb8();
    Ok(Scheme::variations(small.as_raw()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn picture() -> Background {
        Background {
            path: "/p.jpg".into(),
            set_at: 1,
        }
    }

    fn blue() -> Scheme {
        Scheme::from_pixels(&[30, 60, 160].repeat(50))
    }

    fn orange() -> Scheme {
        Scheme::from_pixels(&[160, 80, 30].repeat(50))
    }

    #[test]
    fn on_with_a_picture_samples_then_emits_once() {
        let mut a = Adapt {
            theme: Some(BusTheme::default()),
            on: true,
            picture: Some(picture()),
            ..Adapt::default()
        };
        assert_eq!(a.step(), Step::Sample(picture()));
        assert_eq!(a.step(), Step::Nothing, "one sample in flight");
        a.sampled(picture(), Ok(vec![blue()]));
        let Step::Emit(adapted) = a.step() else {
            panic!("expected an emit");
        };
        // The echo changes nothing.
        a.theme = Some(adapted);
        assert_eq!(a.step(), Step::Nothing);
    }

    #[test]
    fn going_off_puts_the_greys_back() {
        let mut a = Adapt {
            theme: Some(BusTheme::default()),
            on: true,
            picture: Some(picture()),
            ..Adapt::default()
        };
        a.step();
        a.sampled(picture(), Ok(vec![blue()]));
        a.step();
        a.on = false;
        assert_eq!(a.step(), Step::Emit(BusTheme::default()));
        assert_eq!(a.step(), Step::Nothing);
    }

    #[test]
    fn off_from_the_start_leaves_the_theme_alone() {
        let mut theme = BusTheme::default();
        theme.adapt(&blue(), 0.6);
        let mut a = Adapt {
            theme: Some(theme),
            picture: Some(picture()),
            ..Adapt::default()
        };
        assert_eq!(a.step(), Step::Nothing);
    }

    #[test]
    fn the_variation_picks_the_scheme_and_past_the_end_the_last() {
        let mut a = Adapt {
            theme: Some(BusTheme::default()),
            on: true,
            picture: Some(picture()),
            ..Adapt::default()
        };
        a.step();
        a.sampled(picture(), Ok(vec![orange(), blue()]));
        let theme_for = |a: &mut Adapt| match a.step() {
            Step::Emit(t) => t,
            other => panic!("expected an emit, got {other:?}"),
        };
        let first = theme_for(&mut a);
        a.variation = 1;
        let second = theme_for(&mut a);
        assert_ne!(first, second);
        a.variation = 9;
        assert_eq!(a.step(), Step::Nothing, "past the end: the last, unchanged");
        let v = a.variations().unwrap();
        assert_eq!(v.choices.len(), 2);
        assert_eq!(v.choices[1].name, "Blue");
    }

    #[test]
    fn your_colors_apply_without_a_picture_and_go_with_them() {
        let mut a = Adapt {
            theme: Some(BusTheme::default()),
            ..Adapt::default()
        };
        a.colors.insert("accent".into(), "#ff8800".into());
        let Step::Emit(t) = a.step() else {
            panic!("expected an emit");
        };
        assert_eq!(t.palette.tokens["accent"].value, "#ff8800");
        a.theme = Some(t);
        assert_eq!(a.step(), Step::Nothing);
        a.colors.clear();
        assert_eq!(a.step(), Step::Emit(BusTheme::default()));
    }

    #[test]
    fn a_failed_sample_is_not_retried() {
        let mut a = Adapt {
            theme: Some(BusTheme::default()),
            on: true,
            picture: Some(picture()),
            ..Adapt::default()
        };
        a.step();
        a.sampled(picture(), Err("bad".into()));
        assert_eq!(a.step(), Step::Nothing);
    }
}
