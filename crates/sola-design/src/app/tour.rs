//! Tours, notes, and the camera glide on the canvas on screen (see
//! [`crate::tour`]): following an exit, stepping a flow, the tour bar, and
//! the flows list above the pill.

use std::time::Instant;

use iced::widget::{button, column, container, row, text};
use iced::{Alignment, Element, Length};
use sola_kit::components::{button as kit_button, icon::icon_handle, toolbar};
use sola_kit::fonts;

use super::{App, Msg};
use crate::boards::Board;
use crate::camera;
use crate::scene::{Choice, Scene};
use crate::tour::{self, Nav, Tour, Tween};

impl App {
    /// The annotations of the canvas on screen, as drawn.
    pub(super) fn shown_scene(&self) -> Scene {
        self.shown_live()
            .map(|l| Scene::build(&l.boards, &l.annot))
            .unwrap_or_default()
    }

    /// After a re-layout of the canvas on screen: a tour keeps only the
    /// boards that are still there, and ends when its step is gone.
    pub(super) fn check_tour(&mut self) {
        let ids: Vec<String> = self.shown_boards().iter().map(|b| b.art.id.clone()).collect();
        if let Some(t) = &mut self.tour {
            if !t.keep(|b| ids.iter().any(|id| id == b)) {
                self.tour = None;
            }
        }
        let notes = self.shown_live().map_or(0, |l| l.annot.notes.len());
        self.open_notes.retain(|n| *n <= notes);
    }

    /// An exit (or a board link) in board `tab` was clicked: go to `to`.
    /// Outside a tour this starts one, so Back returns.
    pub(super) fn follow(&mut self, tab: u64, to: &str) {
        let Some(from) = self.board_of(tab).map(|b| b.art.id.clone()) else {
            return;
        };
        let t = self.tour.get_or_insert_with(|| Tour::new(None, from.clone()));
        t.go(&from);
        t.go(to);
        self.glide_to(to);
    }

    pub(super) fn start_flow(&mut self, id: &str) {
        self.flows_open = false;
        let Some(flow) = self.shown_live().and_then(|l| l.annot.flow(id)).cloned() else {
            return;
        };
        self.tour = Some(Tour::new(Some(flow.title), flow.start.clone()));
        self.glide_to(&flow.start);
    }

    pub(super) fn nav(&mut self, nav: Nav) {
        match nav {
            Nav::Notes => self.notes_open = !self.notes_open,
            Nav::End => {
                self.tour = None;
                self.flows_open = false;
            }
            Nav::Back => {
                let Some(t) = self.tour.as_mut() else {
                    return;
                };
                if t.back() {
                    let step = t.step().to_string();
                    self.glide_to(&step);
                }
            }
            Nav::Next => {
                if let [only] = self.tour_choices().as_slice() {
                    let to = only.to.clone();
                    self.step_to(&to);
                }
            }
            Nav::Pick(d) => {
                if let Some(c) = self.tour_choices().get(d.wrapping_sub(1)) {
                    let to = c.to.clone();
                    self.step_to(&to);
                }
            }
        }
    }

    /// Frame the tour's step again (after looking around).
    pub(super) fn tour_return(&mut self) {
        if let Some(step) = self.tour.as_ref().map(|t| t.step().to_string()) {
            self.glide_to(&step);
        }
    }

    fn step_to(&mut self, to: &str) {
        if let Some(t) = &mut self.tour {
            t.go(to);
            self.glide_to(to);
        }
    }

    /// Where the tour can go from its step.
    fn tour_choices(&self) -> Vec<Choice> {
        match &self.tour {
            Some(t) => self.shown_scene().choices(t.step()),
            None => Vec::new(),
        }
    }

    /// Glide the camera to frame board `id`. The human's move: an agent's
    /// `view` no longer steers this canvas.
    fn glide_to(&mut self, id: &str) {
        let rect = self.shown_boards().iter().find(|b| b.art.id == id).map(Board::rect);
        let (Some(view), Some(rect)) = (self.plane_size, rect) else {
            return;
        };
        let to = camera::frame_board(rect, view, tour::BAR_RESERVE);
        self.take_camera();
        self.focus_board(None);
        self.tween = Some(Tween::new(self.camera, to, Instant::now()));
    }

    /// One animation frame of the glide.
    pub(super) fn glide(&mut self, now: Instant) {
        let (Some(t), Some(view)) = (self.tween, self.plane_size) else {
            self.tween = None;
            return;
        };
        let (cam, done) = t.at(now, view);
        self.camera = cam;
        if done {
            self.tween = None;
        }
    }

    // ── view ──────────────────────────────────────────────────────

    fn board_title(&self, id: &str) -> String {
        self.shown_boards()
            .iter()
            .find(|b| b.art.id == id)
            .map_or_else(|| id.to_string(), |b| b.art.title.clone())
    }

    /// The tour bar above the pill: back, the step, next or the fork's
    /// choices, return, end.
    pub(super) fn tour_bar(&self) -> Option<Element<'_, Msg>> {
        let t = self.tour.as_ref()?;
        let choices = self.tour_choices();
        let tool = |icon: &str, tip: &'static str, msg: Option<Msg>| {
            toolbar::toolbar_icon_tip(icon_handle(icon), tip, msg)
        };
        let small = |s: String| text(s).size(12).font(fonts::ui()).style(sola_kit::components::text::muted);
        let mut place = column![].spacing(1);
        let mut head = format!("Step {}", t.number());
        if let Some(flow) = &t.flow {
            head = format!("{flow} · {head}");
        }
        place = place
            .push(small(head))
            .push(text(self.board_title(t.step())).size(13).font(fonts::ui_medium()));
        let next = match choices.as_slice() {
            [_] => Some(Msg::Tour(Nav::Next)),
            _ => None,
        };
        let mut bar = row![
            tool("lucide/chevron-left", "Back (←)", (t.number() > 1).then_some(Msg::Tour(Nav::Back))),
            container(place).padding([0, 6]),
            tool("lucide/chevron-right", "Next (→)", next),
        ]
        .spacing(4)
        .align_y(Alignment::Center);
        if choices.is_empty() {
            bar = bar.push(small("End of the flow".into()));
        }
        bar = bar
            .push(tool("lucide/locate", "Back to this step", Some(Msg::TourReturn)))
            .push(tool("lucide/x", "End the tour (Esc)", Some(Msg::Tour(Nav::End))));
        let mut body = column![].spacing(6).align_x(Alignment::Center);
        if choices.len() > 1 {
            let mut picks = row![].spacing(4);
            for (i, c) in choices.iter().enumerate().take(9) {
                let target = self.board_title(&c.to);
                let words = match &c.name {
                    Some(name) => format!("{}  {name} → {target}", i + 1),
                    None => format!("{}  {target}", i + 1),
                };
                picks = picks.push(
                    kit_button::labeled_sm(words, kit_button::secondary).on_press(Msg::Tour(Nav::Pick(i + 1))),
                );
            }
            body = body.push(picks);
        }
        body = body.push(bar);
        Some(floating(container(body).padding([6, 8]).style(super::pill_style)))
    }

    /// The canvas's flows, above the pill.
    pub(super) fn flows_list(&self) -> Option<Element<'_, Msg>> {
        if !self.flows_open || self.tour.is_some() {
            return None;
        }
        let flows = &self.shown_live()?.annot.flows;
        let mut list = column![].spacing(2).width(Length::Shrink);
        for f in flows {
            let mut words = column![text(f.title.clone()).size(13).font(fonts::ui_medium())].spacing(1);
            if let Some(note) = &f.note {
                words = words.push(
                    text(note.clone()).size(12).font(fonts::ui()).style(sola_kit::components::text::muted),
                );
            }
            list = list.push(
                button(words)
                    .padding([6, 10])
                    .width(Length::Fill)
                    .style(kit_button::ghost)
                    .on_press(Msg::StartFlow(f.id.clone())),
            );
        }
        let body = container(list.width(Length::Fixed(320.0))).padding(4).style(super::pill_style);
        Some(floating(body))
    }
}

/// Bottom-centre, just above the pill.
fn floating<'a>(body: impl Into<Element<'a, Msg>>) -> Element<'a, Msg> {
    container(body)
        .width(Length::Fill)
        .height(Length::Fill)
        .align_x(Alignment::Center)
        .align_y(Alignment::End)
        .padding(iced::Padding {
            top: 16.0,
            right: 16.0,
            bottom: 62.0,
            left: 16.0,
        })
        .into()
}
