//! A computer's color: the hue Oath gives its prompt (`oath-core`
//! `seat::prompt_colors`), so a computer looks the same in its prompt and
//! wherever Sola names it (the host chip on a Projects window's tab).
//!
//! The hue is FNV-1a of the hostname, mod 360. Oath draws the prompt at
//! saturation 0.53, lightness 0.50 ([`prompt_rgb`]); a view picks its own
//! lightness for contrast on its ground ([`hsl_rgb`]).

/// Saturation of the prompt's `user@host` color.
pub const PROMPT_SATURATION: f32 = 0.53;
/// Lightness of the prompt's `user@host` color.
pub const PROMPT_LIGHTNESS: f32 = 0.50;

/// The hue, in degrees, for `hostname` (Oath's prompt hue).
pub fn host_hue(hostname: &str) -> f32 {
    let hash = hostname
        .bytes()
        .fold(0x811c_9dc5u32, |h, b| (h ^ u32::from(b)).wrapping_mul(0x0100_0193));
    (hash % 360) as f32
}

/// The prompt's `user@host` color for `hostname`, as Oath writes it.
pub fn prompt_rgb(hostname: &str) -> [u8; 3] {
    hsl_rgb(host_hue(hostname), PROMPT_SATURATION, PROMPT_LIGHTNESS)
}

/// HSL to 8-bit RGB (`h` in degrees; `s`, `l` in 0..=1), rounded as Oath
/// rounds.
pub fn hsl_rgb(h: f32, s: f32, l: f32) -> [u8; 3] {
    let (h, s, l) = (f64::from(h), f64::from(s), f64::from(l));
    let c = (1.0 - (2.0 * l - 1.0).abs()) * s;
    let x = c * (1.0 - ((h / 60.0) % 2.0 - 1.0).abs());
    let (r, g, b) = match (h / 60.0) as u32 {
        0 => (c, x, 0.0),
        1 => (x, c, 0.0),
        2 => (0.0, c, x),
        3 => (0.0, x, c),
        4 => (x, 0.0, c),
        _ => (c, 0.0, x),
    };
    let m = l - c / 2.0;
    let byte = |v: f64| ((v + m) * 255.0).round() as u8;
    [byte(r), byte(g), byte(b)]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hex([r, g, b]: [u8; 3]) -> String {
        format!("#{r:02X}{g:02X}{b:02X}")
    }

    /// The colors Oath wrote into each machine's `/etc/zshrc` (2026-10-30).
    #[test]
    fn matches_oaths_prompts() {
        assert_eq!(hex(prompt_rgb("novus")), "#C33C96");
        assert_eq!(hex(prompt_rgb("canto")), "#8D3CC3");
    }

    #[test]
    fn same_name_same_hue() {
        assert_eq!(host_hue("buccina"), host_hue("buccina"));
        assert_ne!(host_hue("novus"), host_hue("canto"));
    }
}
