//! Reserved-slot status mark — the scan signature for sidebar rows.
//!
//! Shape carries state so a column can be read without reading labels:
//! working ring (motion + accent), waiting diamond (warning), done check
//! (success), idle dim disc (layout reserved). Who (agent name) stays
//! off this mark.
//!
//! [`SidebarIndicator::Active`] keeps the older filled success disc so
//! generic apps that already used it do not change.

use std::sync::OnceLock;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use iced::advanced::layout::{self, Layout};
use iced::advanced::svg::{Handle, Renderer as _, Svg};
use iced::advanced::widget::{Tree, Widget};
use iced::advanced::{Clipboard, Shell, renderer};
use iced::theme::palette::Extended;
use iced::window;
use iced::{Color, Element, Event, Length, Radians, Rectangle, Size, Theme, mouse};



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
/// Working-ring cadence. `request_redraw()` (NextFrame) presents the whole
/// window at vsync; `At` keeps the spin readable without a 60 Hz loop.
const RING_TICK: Duration = Duration::from_millis(50);

/// Leading status mark sized to [`STATUS_MARK_SLOT`].
///
/// Drawn as tinted SVG, not canvas geometry: on wgpu's GL backend
/// (canto before Oath shipped RADV, or `WGPU_BACKEND=gl`) each small
/// canvas drew with another mesh's transform, so marks landed elsewhere
/// on the window or not at all. The same canvas draws right on Vulkan.
pub fn status_mark<'a, Message: 'a>(indicator: SidebarIndicator) -> Element<'a, Message> {
    Element::new(Mark { kind: indicator })
}

struct Mark {
    kind: SidebarIndicator,
}

impl<Message> Widget<Message, Theme, iced::Renderer> for Mark {
    fn size(&self) -> Size<Length> {
        Size::new(
            Length::Fixed(STATUS_MARK_SLOT),
            Length::Fixed(STATUS_MARK_SLOT),
        )
    }

    fn layout(
        &mut self,
        _tree: &mut Tree,
        _renderer: &iced::Renderer,
        _limits: &layout::Limits,
    ) -> layout::Node {
        layout::Node::new(Size::new(STATUS_MARK_SLOT, STATUS_MARK_SLOT))
    }

    fn draw(
        &self,
        _tree: &Tree,
        renderer: &mut iced::Renderer,
        theme: &Theme,
        _style: &renderer::Style,
        layout: Layout<'_>,
        _cursor: mouse::Cursor,
        _viewport: &Rectangle,
    ) {
        let bounds = layout.bounds();
        let color = mark_color(self.kind, theme.extended_palette());
        let (handle, rotation) = match self.kind {
            SidebarIndicator::Working => (ring(), working_angle()),
            SidebarIndicator::Waiting => (diamond(), Radians(0.0)),
            SidebarIndicator::Done => (check(), Radians(0.0)),
            SidebarIndicator::Active => (dot_active(), Radians(0.0)),
            SidebarIndicator::Idle => (dot_idle(), Radians(0.0)),
        };
        renderer.draw_svg(Svg::new(handle).color(color).rotation(rotation), bounds, bounds);
    }

    fn update(
        &mut self,
        _tree: &mut Tree,
        event: &Event,
        _layout: Layout<'_>,
        _cursor: mouse::Cursor,
        _renderer: &iced::Renderer,
        _clipboard: &mut dyn Clipboard,
        shell: &mut Shell<'_, Message>,
        _viewport: &Rectangle,
    ) {
        if let Some(at) = spin_redraw(self.kind, event) {
            shell.request_redraw_at(at);
        }
    }
}

/// When the working ring wants its next frame. Only rescheduled from the
/// redraw itself so pointer motion does not fight the cadence; the first
/// paint after a status change still delivers `RedrawRequested`.
fn spin_redraw(kind: SidebarIndicator, event: &Event) -> Option<window::RedrawRequest> {
    (kind == SidebarIndicator::Working
        && matches!(event, Event::Window(window::Event::RedrawRequested(_))))
    .then(|| window::RedrawRequest::At(Instant::now() + RING_TICK))
}

/// White shapes on a 12×12 view box; [`Svg::color`] tints them.
fn mark_svg(body: &str) -> Handle {
    Handle::from_memory(
        format!(r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 12 12">{body}</svg>"#)
            .into_bytes(),
    )
}

/// Open ring: radius 4.15, a 72% sweep from 0.4 rad, 1.65 stroke; turned
/// by [`working_angle`] when drawn.
fn ring() -> Handle {
    static H: OnceLock<Handle> = OnceLock::new();
    H.get_or_init(|| {
        mark_svg(
            r##"<path d="M 9.822 7.616 A 4.15 4.15 0 1 1 6.871 1.942" fill="none" stroke="#fff" stroke-width="1.65" stroke-linecap="round"/>"##,
        )
    })
    .clone()
}

fn diamond() -> Handle {
    static H: OnceLock<Handle> = OnceLock::new();
    H.get_or_init(|| mark_svg(r##"<path d="M 6 1.8 L 10.2 6 L 6 10.2 L 1.8 6 Z" fill="#fff"/>"##))
        .clone()
}

fn check() -> Handle {
    static H: OnceLock<Handle> = OnceLock::new();
    H.get_or_init(|| {
        mark_svg(
            r##"<path d="M 2.6 6.15 L 4.95 8.55 L 9.55 3.45" fill="none" stroke="#fff" stroke-width="1.65" stroke-linecap="round" stroke-linejoin="round"/>"##,
        )
    })
    .clone()
}

fn dot_active() -> Handle {
    static H: OnceLock<Handle> = OnceLock::new();
    H.get_or_init(|| mark_svg(r##"<circle cx="6" cy="6" r="3" fill="#fff"/>"##))
        .clone()
}

fn dot_idle() -> Handle {
    static H: OnceLock<Handle> = OnceLock::new();
    H.get_or_init(|| mark_svg(r##"<circle cx="6" cy="6" r="2.25" fill="#fff"/>"##))
        .clone()
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

fn working_angle() -> Radians {
    Radians(working_phase(unix_millis()) * std::f32::consts::TAU)
}

fn unix_millis() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0)
}

/// Phase in `0..1` over [`RING_PERIOD_S`]. Uses millisecond modulo so
/// the spin does not die to f32 precision at UNIX-epoch magnitude
/// (`as_secs_f32()` only steps every ~128s around 2026).
fn working_phase(ms: u128) -> f32 {
    let period_ms = (RING_PERIOD_S * 1000.0) as u128;
    if period_ms == 0 {
        return 0.0;
    }
    (ms % period_ms) as f32 / period_ms as f32
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

    #[test]
    fn working_phase_moves_within_a_second() {
        let a = working_phase(1_700_000_000_000);
        let b = working_phase(1_700_000_000_200);
        assert!(
            (b - a).abs() > 0.15,
            "200ms should advance the 0.85s ring: {a} → {b}"
        );
    }

    #[test]
    fn working_mark_schedules_at_not_next_frame() {
        let event = Event::Window(window::Event::RedrawRequested(Instant::now()));
        let redraw = spin_redraw(SidebarIndicator::Working, &event)
            .expect("working mark must keep spinning");
        assert!(
            matches!(redraw, window::RedrawRequest::At(_)),
            "expected At cadence, got {redraw:?}"
        );
    }

    #[test]
    fn idle_mark_does_not_request_redraw() {
        let event = Event::Window(window::Event::RedrawRequested(Instant::now()));
        assert!(spin_redraw(SidebarIndicator::Idle, &event).is_none());
        let moved = Event::Mouse(mouse::Event::CursorLeft);
        assert!(spin_redraw(SidebarIndicator::Working, &moved).is_none());
    }
}
