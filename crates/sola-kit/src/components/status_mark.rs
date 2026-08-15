//! Reserved-slot status mark — the scan signature for sidebar rows.
//!
//! Shape carries state so a column can be read without reading labels:
//! working ring (motion + accent), waiting diamond (warning), done check
//! (success), idle dim disc (layout reserved). Who (agent name) stays
//! off this mark.
//!
//! [`SidebarIndicator::Active`] keeps the older filled success disc so
//! generic apps that already used it do not change.

use std::time::{SystemTime, UNIX_EPOCH};

use iced::theme::palette::Extended;
use iced::widget::canvas::{self, Frame, Geometry, LineCap, LineJoin, Path, Stroke};
use iced::widget::canvas::path::Arc;
use iced::{Color, Element, Event, Length, Point, Radians, Rectangle, Theme, Vector, mouse};

/// Leading status mark for a sidebar row (activity / health, not selection).
///
/// Prefer always showing a mark so the title does not shift horizontally
/// when activity starts/stops. [`Self::Idle`] is the reserved empty slot.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SidebarIndicator {
    /// Turn in flight (tools, streaming). Open ring, accent, spinning.
    Working,
    /// Needs a human (question, permission). Warning diamond.
    Waiting,
    /// Turn finished. Success check.
    Done,
    /// Session is actively working (generic apps — streaming, recent writes).
    Active,
    /// Present but idle — dim placeholder so layout stays fixed.
    #[default]
    Idle,
}

/// Square reserved for every mark. Rows that always pass an indicator
/// keep a stable title origin.
pub const STATUS_MARK_SLOT: f32 = 12.0;

const RING_PERIOD_S: f32 = 0.85;
const RING_SWEEP: f32 = std::f32::consts::TAU * 0.72;
const STROKE_W: f32 = 1.65;

/// Leading status mark sized to [`STATUS_MARK_SLOT`].
pub fn status_mark<'a, Message: 'a>(indicator: SidebarIndicator) -> Element<'a, Message> {
    iced::widget::canvas(Mark { kind: indicator })
        .width(Length::Fixed(STATUS_MARK_SLOT))
        .height(Length::Fixed(STATUS_MARK_SLOT))
        .into()
}

struct Mark {
    kind: SidebarIndicator,
}

impl<Message> canvas::Program<Message> for Mark {
    type State = ();

    fn update(
        &self,
        _state: &mut Self::State,
        _event: &Event,
        _bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Option<canvas::Action<Message>> {
        if self.kind == SidebarIndicator::Working {
            Some(canvas::Action::request_redraw())
        } else {
            None
        }
    }

    fn draw(
        &self,
        _state: &Self::State,
        renderer: &iced::Renderer,
        theme: &Theme,
        bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Vec<Geometry> {
        let mut frame = Frame::new(renderer, bounds.size());
        let p = theme.extended_palette();
        let color = mark_color(self.kind, p);
        let cx = bounds.width * 0.5;
        let cy = bounds.height * 0.5;
        match self.kind {
            SidebarIndicator::Working => draw_working(&mut frame, cx, cy, color),
            SidebarIndicator::Waiting => draw_waiting(&mut frame, cx, cy, color),
            SidebarIndicator::Done => draw_done(&mut frame, cx, cy, color),
            SidebarIndicator::Active => {
                frame.fill(&Path::circle(Point::new(cx, cy), 3.0), color);
            }
            SidebarIndicator::Idle => {
                frame.fill(&Path::circle(Point::new(cx, cy), 2.25), color);
            }
        }
        vec![frame.into_geometry()]
    }
}

fn mark_color(kind: SidebarIndicator, p: &Extended) -> Color {
    match kind {
        SidebarIndicator::Working => p.primary.base.color,
        SidebarIndicator::Waiting => p.warning.base.color,
        SidebarIndicator::Done | SidebarIndicator::Active => p.success.base.color,
        SidebarIndicator::Idle => Color {
            a: 0.40,
            ..p.background.base.text
        },
    }
}

fn draw_working(frame: &mut Frame, cx: f32, cy: f32, color: Color) {
    let angle = working_angle();
    frame.with_save(|frame| {
        frame.translate(Vector::new(cx, cy));
        frame.rotate(angle);
        let path = Path::new(|b| {
            b.arc(Arc {
                center: Point::new(0.0, 0.0),
                radius: 4.15,
                start_angle: Radians(0.4),
                end_angle: Radians(0.4 + RING_SWEEP),
            });
        });
        frame.stroke(
            &path,
            Stroke::default()
                .with_width(STROKE_W)
                .with_color(color)
                .with_line_cap(LineCap::Round),
        );
    });
}

fn draw_waiting(frame: &mut Frame, cx: f32, cy: f32, color: Color) {
    let r = 4.2;
    let path = Path::new(|b| {
        b.move_to(Point::new(cx, cy - r));
        b.line_to(Point::new(cx + r, cy));
        b.line_to(Point::new(cx, cy + r));
        b.line_to(Point::new(cx - r, cy));
        b.close();
    });
    frame.fill(&path, color);
}

fn draw_done(frame: &mut Frame, cx: f32, cy: f32, color: Color) {
    let path = Path::new(|b| {
        b.move_to(Point::new(cx - 3.4, cy + 0.15));
        b.line_to(Point::new(cx - 1.05, cy + 2.55));
        b.line_to(Point::new(cx + 3.55, cy - 2.55));
    });
    frame.stroke(
        &path,
        Stroke::default()
            .with_width(STROKE_W)
            .with_color(color)
            .with_line_cap(LineCap::Round)
            .with_line_join(LineJoin::Round),
    );
}

fn working_angle() -> Radians {
    let t = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs_f32())
        .unwrap_or(0.0);
    Radians(t * (std::f32::consts::TAU / RING_PERIOD_S))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theme::default_theme;

    #[test]
    fn working_waiting_done_use_distinct_roles() {
        let theme = default_theme();
        let p = theme.extended_palette();
        let working = mark_color(SidebarIndicator::Working, p);
        let waiting = mark_color(SidebarIndicator::Waiting, p);
        let done = mark_color(SidebarIndicator::Done, p);
        let idle = mark_color(SidebarIndicator::Idle, p);
        assert_ne!(working, waiting, "working is accent, waiting is warning");
        assert_ne!(waiting, done, "waiting is warning, done is success");
        assert_ne!(working, idle);
        assert!(idle.a < 0.6, "idle must stay quiet");
    }

    #[test]
    fn active_shares_success_with_done_not_waiting() {
        let theme = default_theme();
        let p = theme.extended_palette();
        assert_eq!(
            mark_color(SidebarIndicator::Active, p),
            mark_color(SidebarIndicator::Done, p)
        );
        assert_ne!(
            mark_color(SidebarIndicator::Active, p),
            mark_color(SidebarIndicator::Waiting, p)
        );
    }
}
