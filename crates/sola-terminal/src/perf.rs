//! Lightweight performance instrumentation for scroll/redraw diagnosis.
//!
//! Always-on, low overhead:
//! - Every sample is aggregated into per-second counters.
//! - A one-line summary is flushed to `/opt/sola/log/sola-terminal-perf.log`
//!   about once per second while anything is happening.
//! - Individual **slow** events (≥ [`SLOW_US`] µs) are logged immediately so
//!   hitch spikes are visible without grepping aggregates.
//!
//! Disable entirely with `SOLA_TERMINAL_PERF=0`. Force verbose (log every
//! draw/wheel sample) with `SOLA_TERMINAL_PERF=verbose`.

use std::fs::OpenOptions;
use std::io::Write;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

/// Log path (same dir as other sola app logs).
const LOG_PATH: &str = "/opt/sola/log/sola-terminal-perf.log";

/// Events at or above this duration get an immediate line.
const SLOW_US: u64 = 8_000; // 8 ms

/// How often to flush the aggregate summary.
const SUMMARY_EVERY: Duration = Duration::from_secs(1);

#[derive(Default)]
struct Counters {
    // draws
    draw_n: u64,
    draw_rebuild_n: u64,
    draw_cache_hit_n: u64,
    draw_total_us: u64,
    draw_lock_us: u64,
    draw_paint_us: u64,
    draw_cells: u64,
    draw_glyphs: u64,
    draw_max_us: u64,
    // pty output wakeups delivered to iced
    ptyout_n: u64,
    // mouse-mode wheel
    wheel_events_n: u64,
    wheel_flushed_reports_n: u64,
    wheel_dropped_n: u64, // accumulated notches not yet flushed at end of window (informational)
    // writer thread
    write_enqueue_n: u64,
    write_bytes_n: u64,
    write_block_us: u64, // time spent inside blocking write(2)
    write_block_n: u64,
    write_max_block_us: u64,
    // reader advance
    reader_advance_n: u64,
    reader_bytes_n: u64,
    reader_lock_us: u64,
    reader_max_lock_us: u64,
}

struct State {
    enabled: bool,
    verbose: bool,
    counters: Counters,
    window_start: Instant,
}

static INIT: AtomicBool = AtomicBool::new(false);
static STATE: Mutex<Option<State>> = Mutex::new(None);

fn state() -> std::sync::MutexGuard<'static, Option<State>> {
    let mut g = STATE.lock().unwrap_or_else(|e| e.into_inner());
    if !INIT.swap(true, Ordering::SeqCst) {
        let mode = std::env::var("SOLA_TERMINAL_PERF").unwrap_or_default();
        let enabled = mode != "0" && mode != "off" && mode != "false";
        let verbose = mode == "verbose" || mode == "2";
        *g = Some(State {
            enabled,
            verbose,
            counters: Counters::default(),
            window_start: Instant::now(),
        });
        if enabled {
            let _ = writeln_raw(&format!(
                "perf: start enabled=1 verbose={} slow_us={SLOW_US} log={LOG_PATH}",
                verbose as u8
            ));
        }
    }
    g
}

fn writeln_raw(line: &str) -> std::io::Result<()> {
    let mut f = OpenOptions::new().create(true).append(true).open(LOG_PATH)?;
    let ts = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs_f64())
        .unwrap_or(0.0);
    writeln!(f, "{ts:.3} {line}")
}

fn maybe_flush_summary(st: &mut State) {
    if st.window_start.elapsed() < SUMMARY_EVERY {
        return;
    }
    let c = std::mem::take(&mut st.counters);
    st.window_start = Instant::now();

    // Quiet second: skip the line so the log stays readable when idle.
    let activity = c.draw_n
        + c.ptyout_n
        + c.wheel_events_n
        + c.write_enqueue_n
        + c.reader_advance_n;
    if activity == 0 {
        return;
    }

    let avg = |sum: u64, n: u64| if n == 0 { 0 } else { sum / n };
    let line = format!(
        "SUM draw={} rebuild={} cache_hit={} draw_avg_us={} draw_max_us={} lock_avg_us={} paint_avg_us={} cells/draw={} glyphs/draw={} \
         ptyout={} wheel_ev={} wheel_flush={} \
         wr_enq={} wr_bytes={} wr_block_n={} wr_block_avg_us={} wr_block_max_us={} \
         rd_adv={} rd_bytes={} rd_lock_avg_us={} rd_lock_max_us={}",
        c.draw_n,
        c.draw_rebuild_n,
        c.draw_cache_hit_n,
        avg(c.draw_total_us, c.draw_n),
        c.draw_max_us,
        avg(c.draw_lock_us, c.draw_rebuild_n.max(1)),
        avg(c.draw_paint_us, c.draw_rebuild_n.max(1)),
        avg(c.draw_cells, c.draw_rebuild_n.max(1)),
        avg(c.draw_glyphs, c.draw_rebuild_n.max(1)),
        c.ptyout_n,
        c.wheel_events_n,
        c.wheel_flushed_reports_n,
        c.write_enqueue_n,
        c.write_bytes_n,
        c.write_block_n,
        avg(c.write_block_us, c.write_block_n.max(1)),
        c.write_max_block_us,
        c.reader_advance_n,
        c.reader_bytes_n,
        avg(c.reader_lock_us, c.reader_advance_n.max(1)),
        c.reader_max_lock_us,
    );
    let _ = writeln_raw(&line);
}

fn with_state(f: impl FnOnce(&mut State)) {
    let mut g = state();
    if let Some(st) = g.as_mut() {
        if !st.enabled {
            return;
        }
        f(st);
        maybe_flush_summary(st);
    }
}

fn us(d: Duration) -> u64 {
    d.as_micros().min(u64::MAX as u128) as u64
}

/// Record one `TermView::draw` call.
pub fn draw(
    total: Duration,
    lock: Duration,
    paint: Duration,
    rebuilt: bool,
    cells: usize,
    glyphs: usize,
) {
    let total_us = us(total);
    let lock_us = us(lock);
    let paint_us = us(paint);
    with_state(|st| {
        st.counters.draw_n += 1;
        st.counters.draw_total_us += total_us;
        if total_us > st.counters.draw_max_us {
            st.counters.draw_max_us = total_us;
        }
        if rebuilt {
            st.counters.draw_rebuild_n += 1;
            st.counters.draw_lock_us += lock_us;
            st.counters.draw_paint_us += paint_us;
            st.counters.draw_cells += cells as u64;
            st.counters.draw_glyphs += glyphs as u64;
        } else {
            st.counters.draw_cache_hit_n += 1;
        }
        if st.verbose || total_us >= SLOW_US || lock_us >= SLOW_US || paint_us >= SLOW_US {
            let _ = writeln_raw(&format!(
                "DRAW rebuilt={rebuilt} total_us={total_us} lock_us={lock_us} paint_us={paint_us} cells={cells} glyphs={glyphs}"
            ));
        }
    });
}

/// Iced received a coalesced PtyOutput for one pane.
pub fn pty_output() {
    with_state(|st| {
        st.counters.ptyout_n += 1;
        if st.verbose {
            let _ = writeln_raw("PTYOUT");
        }
    });
}

/// A mouse-mode wheel event arrived (pre-throttle).
pub fn wheel_event() {
    with_state(|st| {
        st.counters.wheel_events_n += 1;
    });
}

/// Wheel flush wrote `reports` SGR/X10 reports; `pending_left` remain.
pub fn wheel_flush(reports: u32, pending_left: i32) {
    with_state(|st| {
        st.counters.wheel_flushed_reports_n += reports as u64;
        if pending_left != 0 {
            st.counters.wheel_dropped_n += pending_left.unsigned_abs() as u64;
        }
        if st.verbose {
            let _ = writeln_raw(&format!(
                "WHEEL flush_reports={reports} pending_left={pending_left}"
            ));
        }
    });
}

/// Bytes enqueued on a pane write queue (UI → writer thread).
pub fn write_enqueue(bytes: usize) {
    with_state(|st| {
        st.counters.write_enqueue_n += 1;
        st.counters.write_bytes_n += bytes as u64;
    });
}

/// Time spent in a blocking `write(2)` on the writer thread.
pub fn write_block(blocked: Duration, bytes: usize) {
    let blocked_us = us(blocked);
    with_state(|st| {
        st.counters.write_block_n += 1;
        st.counters.write_block_us += blocked_us;
        if blocked_us > st.counters.write_max_block_us {
            st.counters.write_max_block_us = blocked_us;
        }
        if st.verbose || blocked_us >= SLOW_US {
            let _ = writeln_raw(&format!(
                "WRITE_BLOCK us={blocked_us} bytes={bytes}"
            ));
        }
    });
}

/// Reader thread held the term lock for `held` while advancing `bytes`.
pub fn reader_advance(held: Duration, bytes: usize) {
    let held_us = us(held);
    with_state(|st| {
        st.counters.reader_advance_n += 1;
        st.counters.reader_bytes_n += bytes as u64;
        st.counters.reader_lock_us += held_us;
        if held_us > st.counters.reader_max_lock_us {
            st.counters.reader_max_lock_us = held_us;
        }
        if st.verbose || held_us >= SLOW_US {
            let _ = writeln_raw(&format!(
                "READER_LOCK us={held_us} bytes={bytes}"
            ));
        }
    });
}

// Silence unused-warning if some call sites are feature-gated later.
#[allow(dead_code)]
static _TOUCH: AtomicU64 = AtomicU64::new(0);
