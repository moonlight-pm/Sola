//! HSV, enamel plates, and header palettes — iced kit recipes, no bus.

use crate::css::Rgba;

pub fn hsv_to_rgb(h: f32, s: f32, v: f32, a: f32) -> Rgba {
    let c = v * s;
    let m = v - c;
    let h6 = (h * 6.0).rem_euclid(6.0);
    let x = c * (1.0 - (h6 % 2.0 - 1.0).abs());
    let (r, g, b) = match h6 as u32 {
        0 => (c, x, 0.0),
        1 => (x, c, 0.0),
        2 => (0.0, c, x),
        3 => (0.0, x, c),
        4 => (x, 0.0, c),
        _ => (c, 0.0, x),
    };
    Rgba {
        r: ((r + m) * 255.0).round().clamp(0.0, 255.0) as u8,
        g: ((g + m) * 255.0).round().clamp(0.0, 255.0) as u8,
        b: ((b + m) * 255.0).round().clamp(0.0, 255.0) as u8,
        a: (a * 255.0).round().clamp(0.0, 255.0) as u8,
    }
}

pub fn rgb_to_hsv(c: Rgba) -> (f32, f32, f32) {
    let r = c.r as f32 / 255.0;
    let g = c.g as f32 / 255.0;
    let b = c.b as f32 / 255.0;
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    let d = max - min;
    let v = max;
    let s = if max <= 1e-6 { 0.0 } else { d / max };
    let h = if d <= 1e-6 {
        0.0
    } else if (max - r).abs() < 1e-6 {
        ((g - b) / d).rem_euclid(6.0) / 6.0
    } else if (max - g).abs() < 1e-6 {
        ((b - r) / d + 2.0) / 6.0
    } else {
        ((r - g) / d + 4.0) / 6.0
    };
    (h, s, v)
}

pub fn rgb_to_hsl(c: Rgba) -> (f32, f32, f32) {
    let r = c.r as f32 / 255.0;
    let g = c.g as f32 / 255.0;
    let b = c.b as f32 / 255.0;
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    let d = max - min;
    let l = (max + min) / 2.0;
    let s = if d <= 1e-6 {
        0.0
    } else {
        d / (1.0 - (2.0 * l - 1.0).abs())
    };
    let (h, _, _) = rgb_to_hsv(c);
    (h, s, l)
}

pub fn hsl_to_rgb(h: f32, s: f32, l: f32, a: f32) -> Rgba {
    let c = (1.0 - (2.0 * l - 1.0).abs()) * s;
    let m = l - c / 2.0;
    let h6 = (h * 6.0).rem_euclid(6.0);
    let x = c * (1.0 - (h6 % 2.0 - 1.0).abs());
    let (r, g, b) = match h6 as u32 {
        0 => (c, x, 0.0),
        1 => (x, c, 0.0),
        2 => (0.0, c, x),
        3 => (0.0, x, c),
        4 => (x, 0.0, c),
        _ => (c, 0.0, x),
    };
    Rgba {
        r: ((r + m) * 255.0).round().clamp(0.0, 255.0) as u8,
        g: ((g + m) * 255.0).round().clamp(0.0, 255.0) as u8,
        b: ((b + m) * 255.0).round().clamp(0.0, 255.0) as u8,
        a: (a * 255.0).round().clamp(0.0, 255.0) as u8,
    }
}

pub fn format_hex(c: Rgba) -> String {
    if c.a < 255 {
        format!("#{:02x}{:02x}{:02x}{:02x}", c.r, c.g, c.b, c.a)
    } else {
        format!("#{:02x}{:02x}{:02x}", c.r, c.g, c.b)
    }
}

pub fn parse_hex(s: &str) -> Option<Rgba> {
    let t = s.trim();
    let t = t.strip_prefix('#').unwrap_or(t);
    crate::css::parse_color(&format!("#{t}"))
}

/// Iced `select::enamel` — kiln hues mixed toward graphite.
pub fn enamel(seed: &str) -> (Rgba, Rgba) {
    let mut h = 2_166_136_261u32;
    for b in seed.as_bytes() {
        h ^= *b as u32;
        h = h.wrapping_mul(16_777_619);
    }
    const KILN: [[f32; 3]; 8] = [
        [0.38, 0.68, 0.72],
        [0.48, 0.54, 0.78],
        [0.72, 0.52, 0.42],
        [0.46, 0.64, 0.50],
        [0.68, 0.48, 0.58],
        [0.58, 0.62, 0.40],
        [0.40, 0.52, 0.70],
        [0.74, 0.62, 0.40],
    ];
    let [r, g, b] = KILN[(h as usize) % KILN.len()];
    let fill = Rgba::rgb(
        (r * 255.0).round() as u8,
        (g * 255.0).round() as u8,
        (b * 255.0).round() as u8,
    );
    let rim = Rgba::rgb(
        ((r * 0.55 + 0.45) * 255.0).min(255.0).round() as u8,
        ((g * 0.55 + 0.45) * 255.0).min(255.0).round() as u8,
        ((b * 0.55 + 0.45) * 255.0).min(255.0).round() as u8,
    );
    (fill, rim)
}

pub fn enamel_style(seed: &str) -> String {
    let (fill, rim) = enamel(seed);
    format!(
        "background:{};border:1px solid {}",
        format_hex(fill),
        format_hex(rim)
    )
}

pub struct ThemeVars {
    pub name: &'static str,
    pub vars: &'static [(&'static str, &'static str)],
}

pub const THEMES: &[ThemeVars] = &[
    ThemeVars {
        name: "Default",
        vars: &[
            ("--bg", "#0c0e12"),
            ("--chrome", "#121722"),
            ("--raised", "#151922"),
            ("--well", "#10141e"),
            ("--hairline", "#3a414c"),
            ("--field", "#1c2029"),
            ("--edge", "#3a414c"),
            ("--hover", "#1e2533"),
            ("--fg", "#e9ecf2"),
            ("--idle", "#a1adc7"),
            ("--header", "#9aa3b8"),
            ("--muted", "#8b94a8"),
            ("--accent", "#3dd6f5"),
            ("--danger", "#f07178"),
            ("--success", "#3ecf8e"),
            ("--warning", "#e8b84a"),
            ("--close", "#ff5f57"),
        ],
    },
    ThemeVars {
        name: "Graphite",
        vars: &[
            ("--bg", "#0a0c10"),
            ("--chrome", "#10141c"),
            ("--raised", "#141820"),
            ("--well", "#0e121a"),
            ("--hairline", "#3a414c"),
            ("--field", "#1a1e26"),
            ("--edge", "#3a414c"),
            ("--hover", "#1a2230"),
            ("--fg", "#e6eaf2"),
            ("--idle", "#98a4be"),
            ("--header", "#8e98ae"),
            ("--muted", "#8490a6"),
            ("--accent", "#3dd6f5"),
            ("--danger", "#f07178"),
            ("--success", "#3ecf8e"),
            ("--warning", "#e8b84a"),
            ("--close", "#ff5f57"),
        ],
    },
    ThemeVars {
        name: "Night",
        vars: &[
            ("--bg", "#06070a"),
            ("--chrome", "#0b0e14"),
            ("--raised", "#10141c"),
            ("--well", "#0a0d14"),
            ("--hairline", "#3a414c"),
            ("--field", "#161a22"),
            ("--edge", "#3a414c"),
            ("--hover", "#182030"),
            ("--fg", "#f2f4f8"),
            ("--idle", "#a8b4cc"),
            ("--header", "#9aa3b8"),
            ("--muted", "#8b94a8"),
            ("--accent", "#5ae0f8"),
            ("--danger", "#ff7a80"),
            ("--success", "#4adb9a"),
            ("--warning", "#f0c45c"),
            ("--close", "#ff5f57"),
        ],
    },
];

pub fn theme_vars(name: &str) -> &'static [(&'static str, &'static str)] {
    THEMES
        .iter()
        .find(|t| t.name == name)
        .unwrap_or(&THEMES[0])
        .vars
}

pub const ATOMS: &[(&str, &str, &str)] = &[
    ("bg", "BG", "--bg"),
    ("raised", "BG_RAISED", "--raised"),
    ("hover", "BG_HOVER", "--hover"),
    ("hairline", "BORDER", "--hairline"),
    ("fg", "FG", "--fg"),
    ("muted", "FG_MUTED", "--muted"),
    ("accent", "ACCENT", "--accent"),
    ("success", "SUCCESS", "--success"),
    ("warning", "WARNING", "--warning"),
    ("danger", "DANGER", "--danger"),
];

pub const SELECT_NAMES: [&str; 3] = ["Primary", "Alternate", "Work"];
pub const SELECT_SEEDS: [&str; 3] = ["seed-primary", "seed-alternate", "seed-work"];

/// Iced storybook `Page::atoms` — contextual swatches under each demo.
pub fn page_atoms(page: &str) -> &'static [&'static str] {
    match page {
        "overview" | "theme" | "shell" => &[],
        "divider" => &["hairline", "bg"],
        "split" => &["bg", "raised", "hairline"],
        "titlebar" => &["bg", "raised", "hairline", "fg"],
        "toolbar" => &["bg", "raised", "hover", "hairline", "fg", "muted"],
        "text" => &["fg", "muted", "accent", "success", "warning", "danger"],
        "json" => &["fg", "muted", "accent", "success", "warning"],
        "button" => &["accent", "danger", "bg", "hover", "hairline", "fg"],
        "badge" => &["accent", "success", "warning", "danger", "hairline", "muted"],
        "card" => &["bg", "raised", "hairline", "fg", "accent"],
        "field" => &["raised", "hairline", "fg", "muted", "danger", "accent"],
        "form" => &["accent", "raised", "hover", "hairline", "fg", "muted"],
        "icon" => &["fg", "muted", "accent"],
        "number_input" => &["bg", "hairline", "fg", "muted", "accent"],
        "readable" => &["bg", "raised", "fg", "muted"],
        "prose" => &["bg", "raised", "fg", "muted", "accent"],
        "color_picker" => &["bg", "raised", "hairline", "fg", "accent"],
        "file_picker" => &["bg", "raised", "hover", "hairline", "fg", "muted", "accent"],
        "popover" => &["raised", "hairline", "fg", "muted"],
        "context_menu" => &["raised", "hairline", "fg", "muted"],
        "select" => &["raised", "hover", "hairline", "fg", "muted"],
        "sidebar" => &["bg", "hover", "fg", "muted", "accent"],
        _ => &[],
    }
}
