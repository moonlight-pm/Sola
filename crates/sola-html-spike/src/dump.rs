//! Headless PNG dumps at 1× and 2× CSS scale.

use std::path::Path;

use crate::app::App;

pub fn run(dir: &Path) {
    std::fs::create_dir_all(dir).ok();
    for scale in [1.0_f32, 2.0] {
        let mut app = App::new(720.0, 640.0, scale);
        let pix = app.frame();
        let (w, h) = app.buffer_size();
        let path = dir.join(format!("scale-{scale:.0}x.png"));
        write_png(&path, w, h, &pix);
        let font_px = 12.0 * scale;
        if let Some((lw, lh, fs)) = app.label_metrics() {
            tracing::info!(
                scale,
                css_label_w = lw,
                css_label_h = lh,
                css_font = fs,
                device_font = fs * scale,
                "label metrics"
            );
        }
        tracing::info!(
            scale,
            w,
            h,
            font_px,
            path = %path.display(),
            "dumped frame"
        );
        assert!(
            font_px >= 12.0,
            "12px CSS type at scale {scale} must be ≥12 device px"
        );
    }

    let mut scrolled = App::new(720.0, 640.0, 1.0);
    scrolled.scroll_y = 400.0;
    let pix = scrolled.frame();
    let (w, h) = scrolled.buffer_size();
    write_png(&dir.join("scrolled.png"), w, h, &pix);
    tracing::info!(scroll_y = scrolled.scroll_y, "dumped scrolled frame");

    let mut typed = App::new(720.0, 640.0, 1.0);
    typed.input = "filter query".into();
    typed.input_focused = true;
    let pix = typed.frame();
    let (w, h) = typed.buffer_size();
    write_png(&dir.join("input.png"), w, h, &pix);
    tracing::info!("dumped focused input frame");

    let mut composing = App::new(720.0, 640.0, 1.0);
    composing.input = "filter ".into();
    composing.input_focused = true;
    composing.set_preedit("へんかん".into(), Some((0, "へんかん".len())));
    let pix = composing.frame();
    let (w, h) = composing.buffer_size();
    write_png(&dir.join("ime-preedit.png"), w, h, &pix);
    tracing::info!("dumped IME preedit frame");

    let mut themed = App::new(720.0, 640.0, 1.0);
    themed.cycle_theme();
    let pix = themed.frame();
    let (w, h) = themed.buffer_size();
    write_png(&dir.join("theme-warm.png"), w, h, &pix);
    themed.cycle_theme();
    let pix = themed.frame();
    write_png(&dir.join("theme-green.png"), w, h, &pix);
    tracing::info!("dumped live CSS var palettes");

    let mut hole = App::new(720.0, 640.0, 1.0);
    hole.tick(0.8);
    let pix = hole.frame();
    let (w, h) = hole.buffer_size();
    write_png(&dir.join("gpu-hole.png"), w, h, &pix);
    let Some((sx, sy, sw, sh)) = hole.surface_device_rect() else {
        panic!("no data-surface box in layout");
    };
    let sample = |x: u32, y: u32| pix[(y * w + x) as usize];
    let tl = sample(sx + 2, sy + 2);
    let tr = sample(sx + sw.saturating_sub(3), sy + 2);
    let br = sample(sx + sw.saturating_sub(3), sy + sh.saturating_sub(3));
    let red = |p: u32| (p >> 16) & 0xff;
    let blu = |p: u32| p & 0xff;
    // Shader is vec4(uv.x, sine, uv.y). wgpu NDC is Y-up, so uv.y=1 is the
    // top of the framebuffer: red grows left→right, blue top→bottom inverted.
    // CPU checker stays near 0x10141e / 0x151822 — tiny channel deltas.
    let gpu_gradient = red(tr) > red(tl) + 40 && blu(tl) > blu(br) + 40;
    tracing::info!(
        gpu_init = hole.gpu_live(),
        gpu_gradient,
        hole = format!("{sx},{sy} {sw}×{sh}"),
        tl = format!("#{:06x}", tl & 0xffffff),
        tr = format!("#{:06x}", tr & 0xffffff),
        br = format!("#{:06x}", br & 0xffffff),
        "wgpu hole samples"
    );
    assert!(
        hole.gpu_live() && gpu_gradient,
        "CSS hole must contain wgpu readback, not CPU checker"
    );
}

pub fn write_png(path: &Path, w: u32, h: u32, pix: &[u32]) {
    let mut rgba = vec![0u8; (w as usize) * (h as usize) * 4];
    for (i, p) in pix.iter().take((w * h) as usize).enumerate() {
        rgba[i * 4] = ((p >> 16) & 0xff) as u8;
        rgba[i * 4 + 1] = ((p >> 8) & 0xff) as u8;
        rgba[i * 4 + 2] = (p & 0xff) as u8;
        rgba[i * 4 + 3] = 255;
    }
    let mut out = Vec::new();
    {
        let mut enc = png::Encoder::new(&mut out, w, h);
        enc.set_color(png::ColorType::Rgba);
        enc.set_depth(png::BitDepth::Eight);
        let mut wr = enc.write_header().expect("png header");
        wr.write_image_data(&rgba).expect("png data");
    }
    std::fs::write(path, out).expect("write png");
}