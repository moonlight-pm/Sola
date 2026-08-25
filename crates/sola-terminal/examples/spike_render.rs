//! Phase 2.1 render SPIKE — THROWAWAY prototype.
//!
//! Proves two things ahead of the real sola-terminal iced port:
//!   1. The real `alacritty_terminal` 0.26 API surface (Term, the
//!      vte re-export, FairMutex, renderable_content, cell fields).
//!   2. That an iced `canvas` can render a live terminal grid fed by a
//!      raw PTY at usable frame rates on the user's RTX 3090 Ti.
//!
//! It opens a raw `bash` PTY (no tmux — the spike isolates rendering),
//! feeds its output through `vte::ansi::Processor` into a shared
//! `Term`, and renders `term.renderable_content()` on a canvas. A
//! background reader thread parses bytes and pings a wakeup channel; an
//! iced subscription turns wakeups into redraw messages that clear the
//! canvas `Cache`.
//!
//! THIS IS NOT PRODUCTION CODE. It cuts every corner that does not bear
//! on the render-perf question (no reflow on resize, fixed grid, no
//! scrollback UI, minimal input encoding, leaked fds on exit).
//!
//! Run instructions + the decision gate live in
//! docs/specs/2026-06-03-sola-terminal-iced-engine-research.md.
//!
//! Build (cannot run here — no GPU/Wayland):
//!   cargo build -p sola-terminal --example spike_render

use std::os::unix::io::IntoRawFd;
use std::os::unix::process::CommandExt;
use std::sync::Arc;
use std::time::Instant;

use iced::futures::channel::mpsc::{self, UnboundedReceiver};
use iced::futures::{Stream, StreamExt};
use iced::widget::canvas::{self, Cache, Canvas, Frame, Geometry, Path, Text};
use iced::widget::container;
use iced::{
    Color, Element, Font, Length, Point, Rectangle, Renderer, Size, Subscription, Theme, keyboard,
    mouse,
};

use alacritty_terminal::event::{Event as TermEvent, EventListener};
use alacritty_terminal::grid::Dimensions;
use alacritty_terminal::index::Point as GridPoint;
use alacritty_terminal::sync::FairMutex;
use alacritty_terminal::term::cell::Flags;
use alacritty_terminal::term::{Config as TermConfig, Term};
// vte is re-exported by alacritty_terminal (`pub use vte;`), so we do NOT
// take a separate `vte` dependency — this is the canonical 0.26 coupling.
use alacritty_terminal::vte::ansi::{Color as AnsiColor, NamedColor, Processor, Rgb};

// ── Fixed spike geometry ───────────────────────────────────────────
const COLS: usize = 80;
const ROWS: usize = 24;
const CELL_W: f32 = 9.0;
const CELL_H: f32 = 18.0;
const FONT_PX: f32 = 14.0;
const PAD: f32 = 6.0;

// ── alacritty plumbing ─────────────────────────────────────────────

/// Minimal `Dimensions` impl. The real `TermSize` lives under
/// `alacritty_terminal::term::test` (a `pub mod test`, not `#[cfg(test)]`
/// gated, but a "test helper" by name) — for a self-contained spike we
/// define our own rather than lean on that. The `Dimensions` trait only
/// requires `total_lines`, `screen_lines`, `columns`.
struct SpikeDims {
    cols: usize,
    rows: usize,
}

impl Dimensions for SpikeDims {
    fn total_lines(&self) -> usize {
        self.rows
    }
    fn screen_lines(&self) -> usize {
        self.rows
    }
    fn columns(&self) -> usize {
        self.cols
    }
}

/// The `EventListener` `Term` calls into. `send_event(&self, Event)` is
/// the whole trait. We only react to `PtyWrite` (apps replying to DSR /
/// cursor-position queries etc. — needed so a real shell session works)
/// and ignore the rest for the spike. The PtyWrite payload must reach
/// the PTY master, so we hold a write fd here.
#[derive(Clone)]
struct SpikeListener {
    write_fd: i32,
}

impl EventListener for SpikeListener {
    fn send_event(&self, event: TermEvent) {
        if let TermEvent::PtyWrite(text) = event {
            write_fd(self.write_fd, text.as_bytes());
        }
        // Wakeup / Title / Bell / ChildExit etc. are no-ops for the spike;
        // redraw is driven by the reader thread's wakeup channel instead.
    }
}

type SharedTerm = Arc<FairMutex<Term<SpikeListener>>>;

// ── PTY (raw bash, no tmux) ────────────────────────────────────────

struct Pty {
    master_fd: i32,
}

/// Open a raw PTY running `bash` directly. Cribbed from
/// crates/sola-terminal/src/pty.rs but stripped of tmux. Sets
/// TERM=xterm-256color and an 80x24 winsize.
fn open_pty() -> Pty {
    let winsize = libc::winsize {
        ws_row: ROWS as u16,
        ws_col: COLS as u16,
        ws_xpixel: 0,
        ws_ypixel: 0,
    };

    let pty = nix::pty::openpty(Some(&winsize), None).expect("openpty failed");
    let slave_fd = pty.slave.into_raw_fd();

    let mut cmd = std::process::Command::new("bash");
    cmd.env("TERM", "xterm-256color");
    // SAFETY: the pre-exec callback runs in the child between fork and the
    // exec; the libc calls here are async-signal-safe.
    unsafe {
        cmd.pre_exec(move || {
            libc::setsid();
            libc::dup2(slave_fd, 0);
            libc::dup2(slave_fd, 1);
            libc::dup2(slave_fd, 2);
            if slave_fd > 2 {
                libc::close(slave_fd);
            }
            libc::ioctl(0, libc::TIOCSCTTY, 0);
            Ok(())
        });
    }
    cmd.spawn().expect("failed to spawn bash");

    // Close slave in parent — the child has its own dup'd copies.
    unsafe { libc::close(slave_fd) };

    let master_fd = pty.master.into_raw_fd();
    Pty { master_fd }
}

fn write_fd(fd: i32, data: &[u8]) {
    if data.is_empty() {
        return;
    }
    unsafe {
        libc::write(fd, data.as_ptr() as *const libc::c_void, data.len());
    }
}

// ── Wakeup channel: reader thread → iced subscription ──────────────

/// Process-global wakeup receiver, handed to the iced subscription once.
/// The reader thread owns the sender. `Mutex<Option<…>>` so the
/// subscription can `take()` it exactly once (one receiver per process).
static WAKEUP_RX: std::sync::Mutex<Option<UnboundedReceiver<()>>> = std::sync::Mutex::new(None);

/// Spawn the background reader: blocking `read()` on the PTY master,
/// `processor.advance(&mut term, bytes)` under the FairMutex, then ping
/// the wakeup channel so iced redraws.
fn spawn_reader(read_fd: i32, term: SharedTerm) {
    let (tx, rx) = mpsc::unbounded::<()>();
    *WAKEUP_RX.lock().unwrap() = Some(rx);

    std::thread::spawn(move || {
        let mut processor: Processor = Processor::new();
        let mut buf = [0u8; 65536];
        loop {
            let n =
                unsafe { libc::read(read_fd, buf.as_mut_ptr() as *mut libc::c_void, buf.len()) };
            if n <= 0 {
                break;
            }
            let bytes = &buf[..n as usize];
            {
                let mut t = term.lock();
                processor.advance(&mut *t, bytes);
            }
            // Coalesce: a non-blocking send is fine; if the channel
            // already has a pending wakeup iced will redraw once anyway.
            let _ = tx.unbounded_send(());
        }
    });
}

fn wakeup_stream() -> impl Stream<Item = Msg> {
    let rx = WAKEUP_RX
        .lock()
        .unwrap()
        .take()
        .expect("wakeup receiver already taken (one per process)");
    rx.map(|()| Msg::PtyOutput)
}

// ── iced application ───────────────────────────────────────────────

#[derive(Debug, Clone)]
enum Msg {
    Noop,
    PtyOutput,
    /// A key press: the logical key, active modifiers, and the platform
    /// text for the key (printable chars / IME).
    Key(keyboard::Key, keyboard::Modifiers, Option<String>),
}

struct App {
    term: SharedTerm,
    master_fd: i32,
    cache: Cache,
    /// EMA of frame render time (ms) for the FPS readout.
    frame_ema_ms: std::cell::Cell<f32>,
}

impl App {
    fn new() -> Self {
        let pty = open_pty();
        let master_fd = pty.master_fd;

        // Listener needs a write fd for PtyWrite replies; dup the master.
        let listener_fd = unsafe { libc::dup(master_fd) };
        let listener = SpikeListener {
            write_fd: listener_fd,
        };

        let dims = SpikeDims {
            cols: COLS,
            rows: ROWS,
        };
        let term = Term::new(TermConfig::default(), &dims, listener);
        let term: SharedTerm = Arc::new(FairMutex::new(term));

        // Reader reads from a dup of the master so closing one side is
        // independent (the spike never closes them — it leaks on exit).
        let read_fd = unsafe { libc::dup(master_fd) };
        spawn_reader(read_fd, term.clone());

        App {
            term,
            master_fd,
            cache: Cache::new(),
            frame_ema_ms: std::cell::Cell::new(0.0),
        }
    }

    fn title(&self) -> String {
        "sola-terminal-spike".into()
    }

    fn update(&mut self, msg: Msg) {
        match msg {
            Msg::Noop => {}
            Msg::PtyOutput => {
                // New grid content — drop the cached geometry so draw()
                // re-renders from the live Term.
                self.cache.clear();
            }
            Msg::Key(key, mods, text) => {
                if let Some(bytes) = encode_key(&key, mods, text.as_deref()) {
                    write_fd(self.master_fd, &bytes);
                }
            }
        }
    }

    fn view(&self) -> Element<'_, Msg> {
        let canvas = Canvas::new(self)
            .width(Length::Fixed(COLS as f32 * CELL_W + PAD * 2.0))
            .height(Length::Fixed(ROWS as f32 * CELL_H + PAD * 2.0));
        container(canvas)
            .width(Length::Fill)
            .height(Length::Fill)
            .into()
    }

    fn subscription(&self) -> Subscription<Msg> {
        Subscription::batch([
            Subscription::run(wakeup_stream),
            // iced 0.14 exposes only `keyboard::listen()` → a stream of
            // raw `keyboard::Event`s; we filter to KeyPressed and carry
            // its `text` field (printable chars) alongside the key.
            iced::keyboard::listen().map(|event| match event {
                keyboard::Event::KeyPressed {
                    key,
                    modifiers,
                    text,
                    ..
                } => Msg::Key(key, modifiers, text.map(|s| s.to_string())),
                _ => Msg::Noop,
            }),
        ])
    }

    fn theme(&self) -> Theme {
        Theme::Dark
    }
}

// ── canvas::Program: render the grid ───────────────────────────────

impl canvas::Program<Msg> for App {
    type State = ();

    fn draw(
        &self,
        _state: &(),
        renderer: &Renderer,
        _theme: &Theme,
        bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Vec<Geometry<Renderer>> {
        let start = Instant::now();

        let geometry = self.cache.draw(renderer, bounds.size(), |frame| {
            // Backdrop.
            frame.fill_rectangle(
                Point::ORIGIN,
                frame.size(),
                Color::from_rgb8(0x0a, 0x0c, 0x10),
            );

            let term = self.term.lock();
            let content = term.renderable_content();
            let colors = content.colors;
            let cursor = content.cursor;

            // Pass 1: batched background rects. Only paint cells whose bg
            // differs from the backdrop — avoids a fill per blank cell.
            for indexed in term.renderable_content().display_iter {
                let cell = indexed.cell;
                let point = indexed.point;
                if cell.flags.contains(Flags::WIDE_CHAR_SPACER) {
                    continue;
                }
                let (mut fg, mut bg) = (
                    resolve(cell.fg, colors, true),
                    resolve(cell.bg, colors, false),
                );
                if cell.flags.contains(Flags::INVERSE) {
                    std::mem::swap(&mut fg, &mut bg);
                }
                if bg != DEFAULT_BG {
                    let (x, y) = cell_xy(point);
                    frame.fill_rectangle(Point::new(x, y), Size::new(CELL_W, CELL_H), bg);
                }
            }

            // Pass 2: glyphs. fill_text per non-blank cell. The real port
            // will batch this (glyphon) — the spike measures whether the
            // naive per-cell path is already fast enough.
            for indexed in term.renderable_content().display_iter {
                let cell = indexed.cell;
                let point = indexed.point;
                if cell.c == ' ' || cell.c == '\0' {
                    continue;
                }
                if cell.flags.contains(Flags::WIDE_CHAR_SPACER)
                    || cell.flags.contains(Flags::HIDDEN)
                {
                    continue;
                }
                let mut fg = resolve(cell.fg, colors, true);
                let bg = resolve(cell.bg, colors, false);
                if cell.flags.contains(Flags::INVERSE) {
                    fg = bg;
                }
                let (x, y) = cell_xy(point);
                let text = Text {
                    content: cell.c.to_string(),
                    position: Point::new(x, y),
                    color: fg,
                    size: FONT_PX.into(),
                    font: Font::MONOSPACE,
                    line_height: iced::widget::text::LineHeight::Absolute(CELL_H.into()),
                    shaping: iced::widget::text::Shaping::Advanced,
                    ..Text::default()
                };
                frame.fill_text(text);
            }

            // Block cursor.
            draw_cursor(frame, cursor.point);
        });

        // FPS / frame-time readout (EMA) to stderr — the user reads this
        // while running the torture commands.
        let dt_ms = start.elapsed().as_secs_f32() * 1000.0;
        let prev = self.frame_ema_ms.get();
        let ema = if prev == 0.0 {
            dt_ms
        } else {
            prev * 0.9 + dt_ms * 0.1
        };
        self.frame_ema_ms.set(ema);
        eprintln!(
            "[spike] frame build {dt_ms:6.2}ms  ema {ema:6.2}ms  (~{:5.1} fps)",
            if ema > 0.0 { 1000.0 / ema } else { 0.0 }
        );

        vec![geometry]
    }
}

const DEFAULT_BG: Color = Color {
    r: 0x0a as f32 / 255.0,
    g: 0x0c as f32 / 255.0,
    b: 0x10 as f32 / 255.0,
    a: 1.0,
};
const DEFAULT_FG: Color = Color {
    r: 0xc8 as f32 / 255.0,
    g: 0xcd as f32 / 255.0,
    b: 0xd6 as f32 / 255.0,
    a: 1.0,
};

fn cell_xy(point: GridPoint) -> (f32, f32) {
    let col = point.column.0 as f32;
    let line = point.line.0 as f32; // visible lines are >= 0 in display_iter
    (PAD + col * CELL_W, PAD + line * CELL_H)
}

fn draw_cursor(frame: &mut Frame<Renderer>, point: GridPoint) {
    let (x, y) = cell_xy(point);
    let rect = Path::rectangle(Point::new(x, y), Size::new(CELL_W, CELL_H));
    frame.fill(&rect, Color::from_rgba8(0xc8, 0xcd, 0xd6, 0.5));
}

/// Resolve an alacritty `Color` to an iced `Color`.
///
/// `term.renderable_content().colors` is an `&Colors` whose entries are
/// `Option<Rgb>` and are mostly `None` by default (alacritty doesn't ship
/// a palette — the embedder supplies one). So for `Named`/`Indexed` we
/// fall back to a built-in 256-colour table.
fn resolve(
    color: AnsiColor,
    colors: &alacritty_terminal::term::color::Colors,
    _is_fg: bool,
) -> Color {
    match color {
        AnsiColor::Spec(rgb) => rgb_to_iced(rgb),
        AnsiColor::Named(named) => {
            if let Some(rgb) = colors[named] {
                return rgb_to_iced(rgb);
            }
            match named {
                NamedColor::Foreground => DEFAULT_FG,
                NamedColor::Background => DEFAULT_BG,
                other => rgb_to_iced(named_rgb(other as usize)),
            }
        }
        AnsiColor::Indexed(idx) => {
            if let Some(rgb) = colors[idx as usize] {
                return rgb_to_iced(rgb);
            }
            rgb_to_iced(indexed_rgb(idx))
        }
    }
}

fn rgb_to_iced(rgb: Rgb) -> Color {
    Color::from_rgb8(rgb.r, rgb.g, rgb.b)
}

/// The 16 ANSI base colours, indexed by `NamedColor as usize` for the
/// 0..=15 range.
fn named_rgb(idx: usize) -> Rgb {
    indexed_rgb(idx.min(15) as u8)
}

/// Standard xterm 256-colour cube. Covers indices the terminal didn't
/// override. 0-15 base, 16-231 6x6x6 cube, 232-255 grayscale ramp.
fn indexed_rgb(idx: u8) -> Rgb {
    const BASE: [(u8, u8, u8); 16] = [
        (0x00, 0x00, 0x00),
        (0xcc, 0x33, 0x33),
        (0x33, 0xaa, 0x33),
        (0xcc, 0xaa, 0x33),
        (0x33, 0x66, 0xcc),
        (0xaa, 0x33, 0xaa),
        (0x33, 0xaa, 0xaa),
        (0xcc, 0xcc, 0xcc),
        (0x55, 0x55, 0x55),
        (0xff, 0x55, 0x55),
        (0x55, 0xff, 0x55),
        (0xff, 0xff, 0x55),
        (0x55, 0x88, 0xff),
        (0xff, 0x55, 0xff),
        (0x55, 0xff, 0xff),
        (0xff, 0xff, 0xff),
    ];
    let i = idx as usize;
    if i < 16 {
        let (r, g, b) = BASE[i];
        return Rgb { r, g, b };
    }
    if i < 232 {
        let i = i - 16;
        let r = (i / 36) as u8;
        let g = ((i / 6) % 6) as u8;
        let b = (i % 6) as u8;
        let conv = |v: u8| if v == 0 { 0 } else { 55 + v * 40 };
        return Rgb {
            r: conv(r),
            g: conv(g),
            b: conv(b),
        };
    }
    let v = 8 + (i - 232) as u8 * 10;
    Rgb { r: v, g: v, b: v }
}

// ── keyboard → PTY bytes ───────────────────────────────────────────

/// Minimal input encoding — enough to *use* a shell for the benchmark:
/// printable chars, Enter, Backspace, Tab, Esc, Ctrl-letter, arrows.
/// Not a full xterm encoder (no kitty protocol, no app-cursor mode).
fn encode_key(
    key: &keyboard::Key,
    mods: keyboard::Modifiers,
    text: Option<&str>,
) -> Option<Vec<u8>> {
    use keyboard::key::Named;

    // Ctrl-letter → control byte (0x01..=0x1a).
    if mods.control() {
        if let keyboard::Key::Character(s) = key {
            if let Some(c) = s.chars().next() {
                let lc = c.to_ascii_lowercase();
                if lc.is_ascii_alphabetic() {
                    return Some(vec![(lc as u8 - b'a') + 1]);
                }
            }
        }
    }

    match key {
        keyboard::Key::Named(named) => match named {
            Named::Enter => Some(b"\r".to_vec()),
            Named::Backspace => Some(vec![0x7f]),
            Named::Tab => Some(b"\t".to_vec()),
            Named::Escape => Some(vec![0x1b]),
            Named::Space => Some(b" ".to_vec()),
            Named::ArrowUp => Some(b"\x1b[A".to_vec()),
            Named::ArrowDown => Some(b"\x1b[B".to_vec()),
            Named::ArrowRight => Some(b"\x1b[C".to_vec()),
            Named::ArrowLeft => Some(b"\x1b[D".to_vec()),
            Named::Home => Some(b"\x1b[H".to_vec()),
            Named::End => Some(b"\x1b[F".to_vec()),
            Named::Delete => Some(b"\x1b[3~".to_vec()),
            // Fall back to the platform text for any other named key
            // (covers e.g. numpad / punctuation that arrive as Named).
            _ => text
                .filter(|t| !t.is_empty())
                .map(|t| t.as_bytes().to_vec()),
        },
        // Prefer the platform-produced text (handles shifted symbols and
        // dead keys); fall back to the raw character.
        keyboard::Key::Character(s) => Some(
            text.filter(|t| !t.is_empty())
                .unwrap_or(s)
                .as_bytes()
                .to_vec(),
        ),
        _ => text
            .filter(|t| !t.is_empty())
            .map(|t| t.as_bytes().to_vec()),
    }
}

// ── main ───────────────────────────────────────────────────────────

fn main() -> iced::Result {
    // Skip the binary self-watcher: this example isn't installed at
    // /opt/sola/bin, so watch_own_binary would have nothing to watch.
    // startup() honours this env var and cleanly skips the watcher while
    // still doing wayland-session + GPU env activation, which is what the
    // wgpu/EGL backend needs to come up from a bare TTY.
    unsafe {
        std::env::set_var("SOLA_NO_SELF_WATCH", "1");
    }
    sola_kit::app::startup("sola-terminal-spike");

    iced::application(App::new, App::update, App::view)
        .title(App::title)
        .subscription(App::subscription)
        .theme(App::theme)
        .run()
}
