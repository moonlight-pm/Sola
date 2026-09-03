//! Default-sink spectrum analyzer for the menubar.
//!
//! A long-lived `pw-cat` tap (`stream.capture.sink`, `node.passive`) feeds
//! a 2048-point FFT, folded into 12 frequency bands with a treble-biased
//! log warp (more bars on the right) and pink (+3 dB/oct) weighting. The
//! canvas reads the bands on redraw
//! and self-wakes with `RedrawRequest::At` only while the meter is live.

use super::Event;
use iced::futures::channel::mpsc::UnboundedSender;
use std::collections::VecDeque;
use std::io::Read;
use std::os::fd::AsFd;
use std::process::{Child, ChildStdout, Command, Stdio};
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

pub const BANDS: usize = 12;
const RATE: u32 = 44100;
const FFT_N: usize = 2048;
const POLL_MS: u16 = 50;
/// dBFS floor for each band. Linear PCM sits too quiet to light a 5-row
/// matrix; this matches a compact digital meter.
const FLOOR_DB: f32 = -54.0;
const LIVE_EPS: f32 = 0.02;
/// Fast rise, slower fall — classic LED analyzer ballistics.
const RISE: f32 = 0.62;
const FALL: f32 = 0.80;
/// Linear gain before dB mapping so FFT band RMS clears the floor.
const GAIN: f32 = 3.0;
/// Peak-hold release per 50 ms tick (~2 s to drop by half). Instant attack.
const AGC_RELEASE: f32 = 0.985;
const AGC_FLOOR: f32 = 0.06;
/// Display range. Spotify / mastered pop has almost no energy above
/// ~8 kHz; the last bar is the presence band (~5–6.5 kHz) where hats
/// and vocals actually live.
const FMIN: f32 = 55.0;
const FMAX: f32 = 6_500.0;
/// Power warp on the log interpolant. 1.0 = equal octaves (too much
/// bass). <1 spends more of the 12 bars on the treble.
const WARP: f32 = 0.58;
/// Pink-noise flattening around `PINK_REF_HZ`. 0.7 ≈ +4 dB/octave so
/// the right-hand bars can compete with kick/vocal energy.
const PINK_REF_HZ: f32 = 500.0;
const PINK_EXP: f32 = 0.7;
/// Analog graphic-EQ bands overlap (constant-Q). Stretch each FFT
/// window so a hot neighbour still lights the next bar.
const OVERLAP: f32 = 1.25;

static TARGET: OnceLock<Mutex<Option<u32>>> = OnceLock::new();
static RING: OnceLock<Mutex<[f32; BANDS]>> = OnceLock::new();

fn target_slot() -> &'static Mutex<Option<u32>> {
    TARGET.get_or_init(|| Mutex::new(None))
}

fn ring_slot() -> &'static Mutex<[f32; BANDS]> {
    RING.get_or_init(|| Mutex::new([0.0; BANDS]))
}

pub fn set_target(id: Option<u32>) {
    if let Ok(mut g) = target_slot().lock() {
        *g = id;
    }
}

pub fn samples() -> [f32; BANDS] {
    ring_slot().lock().map(|g| *g).unwrap_or([0.0; BANDS])
}

pub fn is_live() -> bool {
    samples().iter().copied().any(|v| v > LIVE_EPS)
}

/// Map a linear peak in 0..1 onto the LED envelope.
pub fn loudness(peak: f32) -> f32 {
    if peak <= 1e-6 {
        return 0.0;
    }
    let db = 20.0 * peak.log10();
    ((db - FLOOR_DB) / -FLOOR_DB).clamp(0.0, 1.0)
}

/// Peak of packed little-endian f32 samples (trailing partials ignored).
#[cfg(test)]
pub fn peak_from_f32_le(bytes: &[u8]) -> f32 {
    bytes
        .chunks_exact(4)
        .map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]]).abs())
        .filter(|s| s.is_finite())
        .fold(0.0f32, f32::max)
}

pub fn spawn(kick: UnboundedSender<Event>) {
    std::thread::Builder::new()
        .name("sola-audio-meter".into())
        .spawn(move || run(kick))
        .ok();
}

fn run(kick: UnboundedSender<Event>) {
    reap_stale();
    let mut current: Option<u32> = None;
    let mut child: Option<Child> = None;
    let mut stdout: Option<ChildStdout> = None;
    let mut leftover = Vec::new();
    let mut pcm: VecDeque<f32> = VecDeque::with_capacity(FFT_N * 2);
    let mut agc_hold = 0.0f32;
    let bins = band_bin_ranges(FFT_N, RATE as f32);
    loop {
        let want = target_slot().lock().ok().and_then(|g| *g);
        if want != current {
            stop(&mut child, &mut stdout);
            leftover.clear();
            pcm.clear();
            agc_hold = 0.0;
            current = want;
            if let Some(id) = want {
                match start(id) {
                    Ok((c, out)) => {
                        child = Some(c);
                        stdout = Some(out);
                    }
                    Err(e) => {
                        tracing::debug!("audio meter pw-cat: {e}");
                        std::thread::sleep(Duration::from_secs(1));
                        current = None;
                    }
                }
            } else {
                clear_ring();
            }
        }
        let Some(out) = stdout.as_mut() else {
            std::thread::sleep(Duration::from_millis(POLL_MS as u64));
            continue;
        };
        let ready = poll_in(out, POLL_MS);
        let mut eof = false;
        let mut got = false;
        if ready {
            let mut buf = [0u8; 32768];
            match out.read(&mut buf) {
                Ok(0) => eof = true,
                Ok(n) => {
                    leftover.extend_from_slice(&buf[..n]);
                    let complete = leftover.len() / 4 * 4;
                    for c in leftover[..complete].chunks_exact(4) {
                        let s = f32::from_le_bytes([c[0], c[1], c[2], c[3]]);
                        if s.is_finite() {
                            pcm.push_back(s);
                        }
                    }
                    leftover.drain(..complete);
                    got = true;
                }
                Err(e) if e.kind() == std::io::ErrorKind::Interrupted => {}
                Err(_) => eof = true,
            }
        }
        if eof {
            stop(&mut child, &mut stdout);
            leftover.clear();
            pcm.clear();
            current = None;
            continue;
        }
        if pcm.len() > FFT_N * 3 {
            let extra = pcm.len() - FFT_N * 2;
            pcm.drain(..extra);
        }
        let was = is_live();
        if pcm.len() >= FFT_N {
            let start = pcm.len() - FFT_N;
            let frame: Vec<f32> = pcm.iter().skip(start).copied().collect();
            pcm.drain(..start + FFT_N / 2);
            let mut bands = analyze(&frame, &bins);
            autoscale(&mut bands, &mut agc_hold);
            blend(bands);
        } else if got || was {
            decay();
        } else {
            continue;
        }
        if !was && is_live() {
            let _ = kick.unbounded_send(Event::Kick);
        }
    }
}

/// Stretch the 12 bands so the recent peak uses the full LED stack.
/// Attack is instant (a louder moment raises the ceiling now); release is
/// slow so a quieter passage still has headroom, and relative band
/// heights stay intact. Silence is left alone so noise is not boosted.
pub fn autoscale(bands: &mut [f32; BANDS], hold: &mut f32) {
    let now = bands.iter().copied().fold(0.0f32, f32::max);
    if now <= LIVE_EPS {
        *hold = (*hold * AGC_RELEASE).max(AGC_FLOOR);
        return;
    }
    if now > *hold {
        *hold = now;
    } else {
        *hold = (*hold * AGC_RELEASE).max(now);
    }
    let scale = hold.max(AGC_FLOOR);
    for b in bands.iter_mut() {
        *b = (*b / scale).clamp(0.0, 1.0);
    }
}

fn blend(raw: [f32; BANDS]) {
    if let Ok(mut g) = ring_slot().lock() {
        for (shown, next) in g.iter_mut().zip(raw) {
            if next > *shown {
                *shown += (next - *shown) * RISE;
            } else {
                *shown *= FALL;
                if *shown < LIVE_EPS * 0.5 {
                    *shown = 0.0;
                }
            }
        }
    }
}

fn decay() {
    blend([0.0; BANDS]);
}

fn clear_ring() {
    if let Ok(mut g) = ring_slot().lock() {
        *g = [0.0; BANDS];
    }
}

/// `BANDS + 1` frequency edges, Hz. Equal-log octaves put half the
/// meters below ~700 Hz and the last two in empty air. A power warp
/// γ < 1 on the log interpolant (`f = fmin · r^(t^γ)`) is the usual
/// frequency-warped filter-bank trick: more bars on the treble.
pub fn band_edges(fmin: f32, fmax: f32) -> [f32; BANDS + 1] {
    let span = (fmax / fmin).ln();
    let mut e = [0.0f32; BANDS + 1];
    for i in 0..=BANDS {
        let t = i as f32 / BANDS as f32;
        e[i] = fmin * (span * t.powf(WARP)).exp();
    }
    e[0] = fmin;
    e[BANDS] = fmax;
    e
}

/// Pink-noise flattening. Music is closer to 1/f than white; without
/// this the right-hand bars stay dark. +3 dB/octave around `PINK_REF_HZ`.
pub fn pink_weight(hz: f32) -> f32 {
    (hz.max(1.0) / PINK_REF_HZ).powf(PINK_EXP)
}

pub fn band_bin_ranges(n: usize, rate: f32) -> [(usize, usize); BANDS] {
    let nyquist = rate / 2.0;
    let fmax = FMAX.min(nyquist - rate / n as f32);
    let edges = band_edges(FMIN, fmax);
    let bin_hz = rate / n as f32;
    let max_bin = n / 2;
    let mut out = [(0usize, 0usize); BANDS];
    for i in 0..BANDS {
        let lo_f = (edges[i] / OVERLAP).max(FMIN / 2.0);
        let hi_f = (edges[i + 1] * OVERLAP).min(nyquist);
        let lo = (lo_f / bin_hz).floor() as usize;
        let hi = (hi_f / bin_hz).ceil() as usize;
        let lo = lo.clamp(1, max_bin.saturating_sub(1));
        let hi = hi.clamp(lo + 1, max_bin);
        out[i] = (lo, hi);
    }
    out
}

fn analyze(frame: &[f32], bins: &[(usize, usize); BANDS]) -> [f32; BANDS] {
    let n = frame.len();
    let mut re = vec![0.0f32; n];
    let mut im = vec![0.0f32; n];
    let denom = (n - 1) as f32;
    for (i, s) in frame.iter().enumerate() {
        let w = 0.5 * (1.0 - (std::f32::consts::TAU * i as f32 / denom).cos());
        re[i] = s * w;
    }
    fft_inplace(&mut re, &mut im);
    let mut out = [0.0f32; BANDS];
    let inv_n = 1.0 / n as f32;
    let bin_hz = RATE as f32 / n as f32;
    for (i, &(lo, hi)) in bins.iter().enumerate() {
        let mut e = 0.0f32;
        let mut count = 0u32;
        for k in lo..hi {
            let mag = (re[k] * re[k] + im[k] * im[k]).sqrt() * inv_n;
            e += mag * mag;
            count += 1;
        }
        let rms = if count == 0 {
            0.0
        } else {
            (e / count as f32).sqrt()
        };
        let f_lo = lo as f32 * bin_hz;
        let f_hi = hi as f32 * bin_hz;
        let center = (f_lo * f_hi).sqrt();
        out[i] = loudness((rms * GAIN * pink_weight(center)).min(1.0));
    }
    out
}

/// In-place radix-2 Cooley–Tukey, real input already in `re` / `im`.
pub fn fft_inplace(re: &mut [f32], im: &mut [f32]) {
    let n = re.len();
    debug_assert_eq!(n, im.len());
    debug_assert!(n.is_power_of_two());
    let mut j = 0usize;
    for i in 1..n {
        let mut bit = n >> 1;
        while j >= bit {
            j -= bit;
            bit >>= 1;
        }
        j += bit;
        if i < j {
            re.swap(i, j);
            im.swap(i, j);
        }
    }
    let mut len = 2usize;
    while len <= n {
        let ang = -std::f32::consts::TAU / len as f32;
        let (wlen_re, wlen_im) = (ang.cos(), ang.sin());
        let mut i = 0usize;
        while i < n {
            let mut w_re = 1.0f32;
            let mut w_im = 0.0f32;
            let half = len / 2;
            for k in 0..half {
                let ur = re[i + k];
                let ui = im[i + k];
                let vr = re[i + k + half] * w_re - im[i + k + half] * w_im;
                let vi = re[i + k + half] * w_im + im[i + k + half] * w_re;
                re[i + k] = ur + vr;
                im[i + k] = ui + vi;
                re[i + k + half] = ur - vr;
                im[i + k + half] = ui - vi;
                let nr = w_re * wlen_re - w_im * wlen_im;
                w_im = w_re * wlen_im + w_im * wlen_re;
                w_re = nr;
            }
            i += len;
        }
        len <<= 1;
    }
}

/// A shell re-exec (`watch_own_binary`) keeps the same pid, so an old
/// `pw-cat` tap can outlive the previous image. Drop is skipped on exec.
fn reap_stale() {
    let pid = std::process::id();
    let _ = Command::new("pkill")
        .args(["-P", &pid.to_string(), "pw-cat"])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
}

fn start(id: u32) -> Result<(Child, ChildStdout), String> {
    let mut child = Command::new("pw-cat")
        .args([
            "--record",
            "--raw",
            "--format",
            "f32",
            "--rate",
            &RATE.to_string(),
            "--channels",
            "1",
            "--latency",
            "50ms",
            "--media-role",
            "Meter",
            "--target",
            &id.to_string(),
            "--properties",
            "stream.capture.sink=true node.passive=true application.name=sola-shell",
            "-",
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| e.to_string())?;
    let out = child.stdout.take().ok_or_else(|| "no stdout".to_string())?;
    Ok((child, out))
}

fn stop(child: &mut Option<Child>, stdout: &mut Option<ChildStdout>) {
    stdout.take();
    if let Some(mut c) = child.take() {
        let _ = c.kill();
        let _ = c.wait();
    }
}

fn poll_in(out: &ChildStdout, ms: u16) -> bool {
    use nix::poll::{PollFd, PollFlags, poll};
    let mut fds = [PollFd::new(out.as_fd(), PollFlags::POLLIN)];
    match poll(&mut fds, ms) {
        Ok(n) if n > 0 => fds[0]
            .revents()
            .map(|r| r.contains(PollFlags::POLLIN))
            .unwrap_or(false),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn silence_is_zero() {
        assert_eq!(loudness(0.0), 0.0);
        assert_eq!(loudness(1e-9), 0.0);
    }

    #[test]
    fn full_scale_fills() {
        assert!((loudness(1.0) - 1.0).abs() < 1e-5);
    }

    #[test]
    fn quiet_playback_still_lights() {
        let v = loudness(0.023);
        assert!(v > LIVE_EPS, "{v}");
        assert!(v < 0.6, "{v}");
    }

    #[test]
    fn floor_is_dark() {
        let lin = 10f32.powf(FLOOR_DB / 20.0);
        assert!(loudness(lin) < LIVE_EPS);
    }

    #[test]
    fn packed_f32_peak() {
        let mut bytes = Vec::new();
        for s in [0.1f32, -0.4, 0.2] {
            bytes.extend_from_slice(&s.to_le_bytes());
        }
        bytes.push(0xff);
        assert!((peak_from_f32_le(&bytes) - 0.4).abs() < 1e-6);
    }

    #[test]
    fn band_ranges_are_monotonic_and_in_nyquist() {
        let bins = band_bin_ranges(FFT_N, RATE as f32);
        let mut prev_lo = 0;
        let mut prev_hi = 0;
        for (i, &(lo, hi)) in bins.iter().enumerate() {
            assert!(lo < hi, "band {i}: {lo}..{hi}");
            assert!(hi <= FFT_N / 2, "band {i} past Nyquist");
            assert!(lo >= prev_lo, "band {i} lo went backwards");
            assert!(hi >= prev_hi, "band {i} hi went backwards");
            if i > 0 {
                // Constant-Q: this band starts before the previous one ends.
                assert!(lo < prev_hi, "band {i} should overlap previous");
            }
            prev_lo = lo;
            prev_hi = hi;
        }
        assert!(bins[0].0 < bins[BANDS - 1].0);
    }

    #[test]
    fn warp_spends_more_bars_on_the_treble() {
        let e = band_edges(FMIN, FMAX);
        assert!((e[0] - FMIN).abs() < 1e-3);
        assert!((e[BANDS] - FMAX).abs() < 1e-3);
        for i in 0..BANDS {
            assert!(e[i] < e[i + 1], "{} >= {}", e[i], e[i + 1]);
        }
        // Equal-log midpoint is sqrt(fmin·fmax); warp < 1 reaches higher
        // by t=0.5, so more of the remaining bars cover treble.
        let log_mid = (FMIN * FMAX).sqrt();
        let warped_mid = e[BANDS / 2];
        assert!(
            warped_mid > log_mid,
            "mid {warped_mid} should exceed equal-log {log_mid}"
        );
        // Last two bands are presence (~4–6.5 kHz), not empty air.
        assert!(e[BANDS - 2] > 3_000.0);
        assert!((e[BANDS] - FMAX).abs() < 1.0);
        assert!(e[BANDS - 1] < 6_000.0);
    }

    #[test]
    fn pink_weight_rises_with_frequency() {
        let lo = pink_weight(200.0);
        let mid = pink_weight(PINK_REF_HZ);
        let hi = pink_weight(5_000.0);
        assert!((mid - 1.0).abs() < 1e-5);
        assert!(lo < mid);
        assert!(hi > mid);
        assert!((hi / lo) > 4.0);
    }

    #[test]
    fn fft_sine_peaks_at_expected_bin() {
        let n = 64usize;
        let k0 = 8usize;
        let mut re = vec![0.0f32; n];
        let mut im = vec![0.0f32; n];
        for i in 0..n {
            re[i] = (std::f32::consts::TAU * k0 as f32 * i as f32 / n as f32).sin();
        }
        fft_inplace(&mut re, &mut im);
        let mag = |k: usize| (re[k] * re[k] + im[k] * im[k]).sqrt();
        let peak = (0..n / 2)
            .max_by(|a, b| mag(*a).partial_cmp(&mag(*b)).unwrap())
            .unwrap();
        assert_eq!(peak, k0);
    }

    #[test]
    fn autoscale_maps_peak_to_full_and_keeps_ratios() {
        let mut bands = [0.0f32; BANDS];
        bands[3] = 0.40;
        bands[4] = 0.20;
        let mut hold = 0.0;
        autoscale(&mut bands, &mut hold);
        assert!((hold - 0.40).abs() < 1e-5);
        assert!((bands[3] - 1.0).abs() < 1e-5);
        assert!((bands[4] - 0.50).abs() < 1e-5);
        assert!(bands[0] < LIVE_EPS);
    }

    #[test]
    fn autoscale_leaves_silence_dark() {
        let mut bands = [0.0f32; BANDS];
        let mut hold = 0.5;
        autoscale(&mut bands, &mut hold);
        assert!(bands.iter().all(|b| *b == 0.0));
        assert!(hold < 0.5);
    }

    #[test]
    fn autoscale_hold_falls_toward_a_quieter_peak() {
        let mut bands = [0.10f32; BANDS];
        let mut hold = 1.0;
        autoscale(&mut bands, &mut hold);
        assert!(hold < 1.0);
        assert!(hold >= 0.10);
        assert!(bands.iter().all(|b| (*b - 0.10 / hold).abs() < 1e-5));
    }

    #[test]
    fn analyzer_lights_the_band_of_a_mid_sine() {
        let n = FFT_N;
        let rate = RATE as f32;
        let bins = band_bin_ranges(n, rate);
        // ~1 kHz sits in the middle octaves.
        let freq = 1000.0;
        let mut frame = vec![0.0f32; n];
        for (i, s) in frame.iter_mut().enumerate() {
            *s = (std::f32::consts::TAU * freq * i as f32 / rate).sin();
        }
        let bands = analyze(&frame, &bins);
        let (best, _) = bands
            .iter()
            .enumerate()
            .max_by(|a, b| a.1.partial_cmp(b.1).unwrap())
            .unwrap();
        assert!(bands[best] > 0.3, "peak band {best} = {}", bands[best]);
        // Neighbours may light; the far bass/treble should stay quieter.
        assert!(bands[0] < bands[best]);
        assert!(bands[BANDS - 1] < bands[best]);
    }

    #[test]
    fn analyzer_lights_a_high_hat_sine_on_the_right() {
        let n = FFT_N;
        let rate = RATE as f32;
        let bins = band_bin_ranges(n, rate);
        let freq = 5_500.0;
        let mut frame = vec![0.0f32; n];
        for (i, s) in frame.iter_mut().enumerate() {
            *s = (std::f32::consts::TAU * freq * i as f32 / rate).sin();
        }
        let bands = analyze(&frame, &bins);
        let (best, _) = bands
            .iter()
            .enumerate()
            .max_by(|a, b| a.1.partial_cmp(b.1).unwrap())
            .unwrap();
        assert!(best >= BANDS / 2, "5.5 kHz peaked at band {best}");
        assert!(bands[best] > 0.3, "peak band {best} = {}", bands[best]);
        assert!(bands[0] < bands[best]);
    }
}
