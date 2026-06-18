//! Per-tab terminal emulator module.
//!
//! Each tab owns one [`Emulator`] — a thin owner of an
//! `alacritty_terminal::Term` plus a `vte::ansi::Processor`. The emulator
//! ingests raw PTY bytes via [`Emulator::advance`] and exposes the live
//! renderable grid through its `FairMutex<Term<Listener>>` handle.
//!
//! Process-wide output notification
//! ----------------------------------
//! iced Subscriptions are process-global, but emulators are per-tab. We
//! bridge them with a process-wide unbounded `std::sync::mpsc` channel of
//! tab-ids:
//!
//! - The reader thread (Task 2.4) calls `notify_sender()` to get a
//!   `Sender<String>` and sends `tab_id` whenever new bytes have been
//!   parsed.
//! - `output_subscription()` drains the `Receiver` on a background thread
//!   and delivers tab-ids into iced's stream so `App::update` can redraw.
//!
//! The receiver is taken exactly once (guarded by a `Mutex<Option<…>>`).
//! Calling `output_subscription()` a second time (iced rebuilds the
//! subscription set on every update) returns an empty stream rather than
//! racing on the single receiver — same pattern used by sola-kit's
//! `bus_subscription`.

use std::sync::{Arc, Mutex, OnceLock, mpsc};

use iced::futures::Stream;
use iced::Subscription;

use alacritty_terminal::event::{Event, EventListener};
use alacritty_terminal::grid::Dimensions;
use alacritty_terminal::sync::FairMutex;
use alacritty_terminal::term::{Config, Term};
use alacritty_terminal::vte::ansi::Processor;

// ── Process-wide output-notify channel ────────────────────────────────────────

/// The single global sender. Cloned for every tab's reader thread.
static NOTIFY_TX: OnceLock<mpsc::Sender<String>> = OnceLock::new();

/// The single global receiver, wrapped in `Mutex<Option<…>>` so
/// `output_subscription()` can `take()` it exactly once.
static NOTIFY_RX: Mutex<Option<mpsc::Receiver<String>>> = Mutex::new(None);

/// Initialise the global channel exactly once. Called lazily by both
/// `notify_sender()` and `output_subscription()`.
fn ensure_channel() {
    NOTIFY_TX.get_or_init(|| {
        let (tx, rx) = mpsc::channel::<String>();
        *NOTIFY_RX.lock().unwrap() = Some(rx);
        tx
    });
}

/// Returns a clone of the process-wide output-notify sender.
///
/// Reader threads (Task 2.4) call this to obtain their sender handle and
/// write `tab_id` after each `advance` so iced knows to redraw that tab.
pub fn notify_sender() -> mpsc::Sender<String> {
    ensure_channel();
    NOTIFY_TX.get().unwrap().clone()
}

/// iced `Subscription` that delivers tab-ids whenever a reader thread
/// reports new output.
///
/// A background thread does a blocking `recv` (no busy-poll or sleep loop)
/// and forwards tab-ids into an `iced::futures::channel::mpsc` unbounded
/// channel that feeds the subscription stream. If the receiver has already
/// been taken (iced rebuilds subscriptions on every update), an empty
/// stream is returned — same single-receiver guard used in sola-kit's
/// `bus_subscription`.
pub fn output_subscription() -> Subscription<String> {
    Subscription::run(output_stream)
}

fn output_stream() -> impl Stream<Item = String> {
    // Take the receiver exactly once.
    ensure_channel();
    let rx_opt = NOTIFY_RX.lock().unwrap().take();

    let (iced_tx, iced_rx) = iced::futures::channel::mpsc::unbounded::<String>();

    match rx_opt {
        Some(std_rx) => {
            std::thread::spawn(move || {
                loop {
                    // Exit if the iced side dropped the subscription.
                    if iced_tx.is_closed() {
                        break;
                    }
                    match std_rx.recv() {
                        Ok(tab_id) => {
                            if iced_tx.unbounded_send(tab_id).is_err() {
                                break;
                            }
                        }
                        // All senders dropped — no more tabs live. Stop.
                        Err(_) => break,
                    }
                }
            });
        }
        None => {
            // Receiver already taken (iced rebuilt the subscription set
            // while the thread is still running). Return the channel with
            // no live sender so iced gets an immediately-pending empty
            // stream — this is correct because the original thread is
            // already draining the real receiver.
            tracing::warn!(
                "output_subscription called while receiver is already taken; \
                 returning empty stream (one receiver per process)"
            );
            drop(iced_tx);
        }
    }

    iced_rx
}

// ── Process-wide exit-notify channel ──────────────────────────────────────────
//
// Mirrors the output-notify channel above. The per-tab reader thread sends the
// tab-id here when the PTY hits EOF (shell exited). `exit_subscription()` feeds
// those ids into iced as `Msg::PtyExit(tab_id)` so `App` can tear the tab down.

static EXIT_TX: OnceLock<mpsc::Sender<String>> = OnceLock::new();
static EXIT_RX: Mutex<Option<mpsc::Receiver<String>>> = Mutex::new(None);

fn ensure_exit_channel() {
    EXIT_TX.get_or_init(|| {
        let (tx, rx) = mpsc::channel::<String>();
        *EXIT_RX.lock().unwrap() = Some(rx);
        tx
    });
}

/// Returns a clone of the process-wide exit-notify sender. Reader threads send
/// their tab-id here on EOF.
pub fn exit_sender() -> mpsc::Sender<String> {
    ensure_exit_channel();
    EXIT_TX.get().unwrap().clone()
}

/// iced `Subscription` delivering tab-ids whose PTY reached EOF (shell exit).
pub fn exit_subscription() -> Subscription<String> {
    Subscription::run(exit_stream)
}

fn exit_stream() -> impl Stream<Item = String> {
    ensure_exit_channel();
    let rx_opt = EXIT_RX.lock().unwrap().take();
    let (iced_tx, iced_rx) = iced::futures::channel::mpsc::unbounded::<String>();
    match rx_opt {
        Some(std_rx) => {
            std::thread::spawn(move || {
                loop {
                    if iced_tx.is_closed() {
                        break;
                    }
                    match std_rx.recv() {
                        Ok(tab_id) => {
                            if iced_tx.unbounded_send(tab_id).is_err() {
                                break;
                            }
                        }
                        Err(_) => break,
                    }
                }
            });
        }
        None => {
            tracing::warn!(
                "exit_subscription called while receiver is already taken; \
                 returning empty stream (one receiver per process)"
            );
            drop(iced_tx);
        }
    }
    iced_rx
}

// ── Process-wide title channel ────────────────────────────────────────────────
//
// Mirrors the output/exit channels above. When the terminal emulator processes
// an OSC 0 or OSC 2 sequence (window-title), alacritty_terminal fires
// `Event::Title(s)`. The `Listener` catches this and forwards `(tab_id, title)`
// here. `title_subscription()` delivers those pairs to iced as
// `Msg::Title(tab_id, title)` so `App` can update the window title bar.

static TITLE_TX: OnceLock<mpsc::Sender<(String, String)>> = OnceLock::new();
static TITLE_RX: Mutex<Option<mpsc::Receiver<(String, String)>>> = Mutex::new(None);

fn ensure_title_channel() {
    TITLE_TX.get_or_init(|| {
        let (tx, rx) = mpsc::channel::<(String, String)>();
        *TITLE_RX.lock().unwrap() = Some(rx);
        tx
    });
}

/// Returns a clone of the process-wide title sender.
///
/// `Listener::send_event` calls this (via the stored clone) when it receives
/// `Event::Title` from the ANSI parser.
pub fn title_sender() -> mpsc::Sender<(String, String)> {
    ensure_title_channel();
    TITLE_TX.get().unwrap().clone()
}

/// iced `Subscription` delivering `(tab_id, title)` pairs whenever a tab's
/// OSC 0/2 title changes.
pub fn title_subscription() -> Subscription<(String, String)> {
    Subscription::run(title_stream)
}

fn title_stream() -> impl Stream<Item = (String, String)> {
    ensure_title_channel();
    let rx_opt = TITLE_RX.lock().unwrap().take();
    let (iced_tx, iced_rx) = iced::futures::channel::mpsc::unbounded::<(String, String)>();
    match rx_opt {
        Some(std_rx) => {
            std::thread::spawn(move || {
                loop {
                    if iced_tx.is_closed() {
                        break;
                    }
                    match std_rx.recv() {
                        Ok(pair) => {
                            if iced_tx.unbounded_send(pair).is_err() {
                                break;
                            }
                        }
                        Err(_) => break,
                    }
                }
            });
        }
        None => {
            tracing::warn!(
                "title_subscription called while receiver is already taken; \
                 returning empty stream (one receiver per process)"
            );
            drop(iced_tx);
        }
    }
    iced_rx
}

// ── Listener — EventListener impl ─────────────────────────────────────────────

/// Per-tab `EventListener` wired into `Term<Listener>`.
///
/// Holds three channels:
/// - `pty_write`: terminal replies (DSR / cursor-position / DA) that MUST
///   be written back to the PTY master fd. Task 2.4 drains this and writes
///   to the fd. Dropping these breaks TUIs that depend on device-attribute
///   replies.
/// - `notify`: wakes iced on `Event::Wakeup`. The reader thread ALSO calls
///   `notify_sender()` directly after each `advance`, so this is a
///   harmless secondary path that coalesces naturally.
/// - `title`: forwards OSC 0/2 title strings to iced as `(tab_id, title)`
///   pairs so the window title bar tracks the active tab's shell title.
pub struct Listener {
    tab_id: String,
    pty_write: mpsc::Sender<(String, Vec<u8>)>,
    notify: mpsc::Sender<String>,
    title: mpsc::Sender<(String, String)>,
}

impl Listener {
    pub fn new(
        tab_id: String,
        pty_write: mpsc::Sender<(String, Vec<u8>)>,
        notify: mpsc::Sender<String>,
        title: mpsc::Sender<(String, String)>,
    ) -> Self {
        Self { tab_id, pty_write, notify, title }
    }
}

impl EventListener for Listener {
    fn send_event(&self, event: Event) {
        match event {
            Event::PtyWrite(text) => {
                let _ = self.pty_write.send((self.tab_id.clone(), text.into_bytes()));
            }
            Event::Wakeup => {
                let _ = self.notify.send(self.tab_id.clone());
            }
            // Forward OSC 0/2 title to App via the title channel.
            Event::Title(title) => {
                let _ = self.title.send((self.tab_id.clone(), title));
            }
            // ResetTitle: send an empty string so the window title falls back
            // to "Terminal".
            Event::ResetTitle => {
                let _ = self.title.send((self.tab_id.clone(), String::new()));
            }
            // Phase 4: bell → bus event / visual flash.
            Event::Bell => { /* Phase 4 */ }
            // Phase 4: propagate child exit so App can close the tab.
            Event::ChildExit(_status) => { /* Phase 4 */ }
            // All other variants (MouseCursorDirty,
            // ClipboardStore, ClipboardLoad, ColorRequest,
            // TextAreaSizeRequest, CursorBlinkingChange, Exit, …) are
            // intentionally ignored until their phases land.
            _ => {}
        }
    }
}

// ── Dimensions helper ──────────────────────────────────────────────────────────

/// Minimal `Dimensions` impl used to construct and resize a `Term`.
///
/// `alacritty_terminal::term::test::TermSize` exists as a public test
/// helper but we define our own to avoid relying on that name (pattern
/// from the spike).
#[derive(Copy, Clone)]
struct TermDims {
    cols: usize,
    rows: usize,
}

impl Dimensions for TermDims {
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

// ── Emulator ───────────────────────────────────────────────────────────────────

/// Per-tab terminal emulator.
///
/// Owns an `alacritty_terminal::Term` behind an `Arc<FairMutex>` so the
/// renderer (Task 2.5) and the reader thread (Task 2.4) can each hold an
/// independent handle, plus a `vte::ansi::Processor` that drives the ANSI
/// state machine.
///
/// The renderer calls `term()` to get its `Arc` handle and then
/// `term.lock().renderable_content()` each frame.  The reader thread
/// advances bytes by calling `emulator.advance(bytes)` or by holding its
/// own `Arc` handle and calling `processor.advance` directly — either
/// pattern is fine; only one thread should drive the `Processor`.
///
/// LIVE PATH NOTE: in the running app the reader thread (Task 2.4) holds its
/// own thread-local `Processor` and drives `term()` directly, so this struct's
/// `parser` field is exercised only by the headless `advance` unit test.
pub struct Emulator {
    term: Arc<FairMutex<Term<Listener>>>,
    // Used by Emulator::advance (called from unit tests and future PTY path).
    #[allow(dead_code)]
    parser: Processor,
}

impl Emulator {
    /// Construct a new emulator with a `cols × rows` grid.
    ///
    /// The tab id lives on the `Listener` (its rightful owner); the emulator
    /// is identified by its shared `term` handle, so `new` takes no id.
    pub fn new(cols: u16, rows: u16, listener: Listener) -> Self {
        let dims = TermDims {
            cols: cols as usize,
            rows: rows as usize,
        };
        // Enable the kitty keyboard protocol so apps can negotiate it
        // (CSI > flags u). The engine then tracks the kitty TermMode bits and
        // replies to the report query (CSI ? u) via the `pty_write` channel;
        // `input::resolve_bytes` honours those bits to disambiguate keys such
        // as Shift+Enter (CSI 13;2u) from plain Enter (CR). Without this flag
        // the engine ignores the negotiation entirely.
        let mut config = Config::default();
        config.kitty_keyboard = true;
        let term = Term::new(config, &dims, listener);
        Self {
            term: Arc::new(FairMutex::new(term)),
            parser: Processor::new(),
        }
    }

    /// Feed raw PTY bytes through the VTE parser into the terminal grid.
    ///
    /// `FairMutex::lock()` (parking_lot) returns the guard directly with
    /// no `Result` — do not call `.unwrap()`.
    // Called by unit tests; the live PTY path uses the Arc<Term> handle
    // directly via Processor::advance. Reserved for future integration.
    #[allow(dead_code)]
    pub fn advance(&mut self, bytes: &[u8]) {
        let mut term = self.term.lock();
        self.parser.advance(&mut *term, bytes);
    }

    /// Resize the terminal grid to the new dimensions.
    ///
    /// `Term::resize` takes the size **by value** (0.26 API). Call this
    /// whenever the pane size changes (Task 2.6), and also drive the
    /// resize into tmux so both agree on the grid size.
    pub fn resize(&self, cols: u16, rows: u16) {
        let dims = TermDims {
            cols: cols as usize,
            rows: rows as usize,
        };
        let mut term = self.term.lock();
        term.resize(dims);
    }

    /// Returns a cheap `Arc` clone of the shared term handle.
    ///
    /// The renderer (Task 2.5) and reader thread (Task 2.4) each hold one.
    /// Locking order: hold only one FairMutex lock at a time to avoid
    /// deadlock.
    pub fn term(&self) -> Arc<FairMutex<Term<Listener>>> {
        self.term.clone()
    }

    /// `(history_size, display_offset)` — scrollback diagnostics for the parked
    /// divider-resize issue. Read by the debug-gated `SCROLLBACK` logs in
    /// `main.rs::{resize_all_panes, update}`.
    pub fn scrollback_stats(&self) -> (usize, usize) {
        use alacritty_terminal::grid::Dimensions;
        let t = self.term.lock();
        let g = t.grid();
        (g.total_lines() - g.screen_lines(), g.display_offset())
    }
}

// ── Tests ──────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    /// Feed two ASCII bytes into a fresh emulator and verify they appear as
    /// cells at grid position (line 0, col 0) == 'h' and (line 0, col 1) == 'i'.
    ///
    /// Headless — no iced runtime, no real PTY. Exercises the real 0.26
    /// `Processor::advance` → `Term` → `renderable_content` →
    /// `GridIterator` → `Indexed<&Cell>` chain so any API change in
    /// alacritty_terminal breaks this test immediately.
    #[test]
    fn advance_writes_cells_into_grid() {
        use alacritty_terminal::index::{Column, Line, Point as GridPoint};

        let (ptx, _prx) = mpsc::channel::<(String, Vec<u8>)>();
        let (ntx, _nrx) = mpsc::channel::<String>();
        let (ttx, _trx) = mpsc::channel::<(String, String)>();
        let mut e = Emulator::new(80, 24, Listener::new("t".into(), ptx, ntx, ttx));

        e.advance(b"hi");

        let term = e.term();
        let term = term.lock();
        let content = term.renderable_content();

        // Collect all visible cells so we can query by grid position.
        let cells: Vec<_> = content.display_iter.collect();

        // --- (line 0, col 0) should be 'h' ---
        let cell_h = cells.iter().find(|indexed| {
            indexed.point == GridPoint::new(Line(0), Column(0))
        });
        assert!(cell_h.is_some(), "No cell found at line 0 column 0");
        assert_eq!(
            cell_h.unwrap().cell.c,
            'h',
            "Expected 'h' at (0,0), got {:?}",
            cell_h.unwrap().cell.c
        );

        // --- (line 0, col 1) should be 'i' ---
        let cell_i = cells.iter().find(|indexed| {
            indexed.point == GridPoint::new(Line(0), Column(1))
        });
        assert!(cell_i.is_some(), "No cell found at line 0 column 1");
        assert_eq!(
            cell_i.unwrap().cell.c,
            'i',
            "Expected 'i' at (0,1), got {:?}",
            cell_i.unwrap().cell.c
        );
    }
}
