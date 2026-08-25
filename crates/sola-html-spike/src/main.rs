//! HTML/CSS + Taffy + cosmic-text. Isolated; do not install; do not merge.

mod app;
mod css;
mod dom;
mod dump;
mod foreign;
mod gpu;
mod layout;
mod markup;
mod paint;
mod strip;
mod subsurface;
mod tabs;

use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use winit::application::ApplicationHandler;
use winit::event::{ElementState, Ime, MouseButton, MouseScrollDelta, WindowEvent};
use winit::event_loop::{ActiveEventLoop, EventLoop};
use winit::keyboard::{Key, NamedKey};
use winit::window::{Window, WindowId};

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if let Some(i) = args.iter().position(|a| a == "--dump") {
        tracing_subscriber::fmt()
            .with_env_filter("sola_html_spike=info")
            .with_writer(std::io::stderr)
            .init();
        let dir = args
            .get(i + 1)
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("/tmp/sola-html-spike"));
        dump::run(&dir);
        return;
    }
    if args.iter().any(|a| a == "--foreign-client") {
        tracing_subscriber::fmt()
            .with_env_filter("sola_html_spike=info")
            .with_writer(std::io::stderr)
            .init();
        foreign::run_client();
        return;
    }
    boot("sola-html-spike");
    let event_loop = EventLoop::new().expect("event loop");
    let mut host = Host {
        window: None,
        present: None,
        hole: None,
        foreign: None,
        app: app::App::for_present(720.0, 640.0, 1.0),
        cursor: (0.0, 0.0),
    };
    event_loop.run_app(&mut host).expect("run");
}

struct Host {
    window: Option<Arc<Window>>,
    present: Option<gpu::Present>,
    hole: Option<subsurface::Hole>,
    foreign: Option<foreign::ForeignHole>,
    app: app::App,
    cursor: (f32, f32),
}

impl ApplicationHandler for Host {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_some() {
            return;
        }
        let attrs = Window::default_attributes()
            .with_title("HTML spike · wgpu present")
            .with_inner_size(winit::dpi::LogicalSize::new(720.0, 640.0));
        let window = Arc::new(event_loop.create_window(attrs).unwrap());
        match gpu::Present::new(window.clone()) {
            Some(present) => self.present = Some(present),
            None => tracing::error!("wgpu present failed; window will stay black"),
        }
        self.hole = subsurface::Hole::new(&window);
        if self.hole.is_none() {
            tracing::warn!("no wl_subsurface; CSS hole stays on the parent swapchain");
        }
        self.foreign = foreign::ForeignHole::start(436, 496);
        if self.foreign.is_none() {
            tracing::warn!("nested compositor / foreign client failed");
        }
        self.window = Some(window.clone());
        self.app.scale = window.scale_factor() as f32;
        let size = window.inner_size();
        self.app.css_w = size.width as f32 / self.app.scale;
        self.app.css_h = size.height as f32 / self.app.scale;
        tracing::info!(
            scale = self.app.scale,
            css_w = self.app.css_w,
            css_h = self.app.css_h,
            physical = ?size,
            "window mapped"
        );
        window.request_redraw();
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::KeyboardInput { event, .. }
                if event.state == ElementState::Pressed
                    && event.logical_key == Key::Named(NamedKey::Escape) =>
            {
                event_loop.exit();
            }
            WindowEvent::Resized(size) => {
                let scale = self.app.scale.max(0.01);
                self.app.css_w = size.width as f32 / scale;
                self.app.css_h = size.height as f32 / scale;
                if let Some(present) = &mut self.present {
                    present.resize(size.width.max(1), size.height.max(1));
                }
                if let Some(window) = &self.window {
                    window.request_redraw();
                }
            }
            WindowEvent::ScaleFactorChanged { scale_factor, .. } => {
                self.app.scale = scale_factor as f32;
                if let Some(window) = &self.window {
                    let size = window.inner_size();
                    self.app.css_w = size.width as f32 / self.app.scale;
                    self.app.css_h = size.height as f32 / self.app.scale;
                    window.request_redraw();
                }
            }
            WindowEvent::RedrawRequested => self.redraw(),
            WindowEvent::CursorMoved { position, .. } => {
                let s = self.app.scale.max(0.01);
                self.cursor = (position.x as f32 / s, position.y as f32 / s);
                if self.app.mouse_move(self.cursor.0, self.cursor.1) {
                    if let Some(window) = &self.window {
                        window.request_redraw();
                    }
                }
            }
            WindowEvent::MouseInput {
                state: ElementState::Pressed,
                button: MouseButton::Left,
                ..
            } => {
                self.app.mouse_down(self.cursor.0, self.cursor.1);
                if let Some(window) = &self.window {
                    window.request_redraw();
                }
            }
            WindowEvent::MouseInput {
                state: ElementState::Released,
                button: MouseButton::Left,
                ..
            } => {
                self.app.mouse_up(self.cursor.0, self.cursor.1);
                if let Some(window) = &self.window {
                    window.request_redraw();
                }
            }
            WindowEvent::MouseWheel { delta, .. } => {
                let dy = match delta {
                    MouseScrollDelta::LineDelta(_, y) => y * 32.0,
                    MouseScrollDelta::PixelDelta(p) => p.y as f32,
                };
                if self.app.wheel(dy) {
                    if let Some(window) = &self.window {
                        window.request_redraw();
                    }
                }
            }
            WindowEvent::Ime(event) => {
                match event {
                    Ime::Enabled => {}
                    Ime::Preedit(text, cursor) => self.app.set_preedit(text, cursor),
                    Ime::Commit(text) => self.app.ime_commit(&text),
                    Ime::Disabled => self.app.ime_disable(),
                }
                if let Some(window) = &self.window {
                    window.request_redraw();
                }
            }
            WindowEvent::KeyboardInput { event, .. }
                if event.state == ElementState::Pressed =>
            {
                match &event.logical_key {
                    Key::Named(NamedKey::F2) => {
                        self.app.cycle_theme();
                        if let Some(window) = &self.window {
                            window.request_redraw();
                        }
                    }
                    Key::Named(NamedKey::Backspace) => {
                        self.app.backspace();
                        if let Some(window) = &self.window {
                            window.request_redraw();
                        }
                    }
                    Key::Named(NamedKey::Space) => {
                        self.app.type_text(" ");
                        if let Some(window) = &self.window {
                            window.request_redraw();
                        }
                    }
                    Key::Character(c) if event.text.as_ref().is_some() => {
                        if let Some(t) = event.text.as_ref() {
                            if !t.chars().any(|ch| ch.is_control()) {
                                self.app.type_text(t);
                                if let Some(window) = &self.window {
                                    window.request_redraw();
                                }
                            }
                        }
                        let _ = c;
                    }
                    _ => {}
                }
            }
            _ => {}
        }
    }

    fn about_to_wait(&mut self, _event_loop: &ActiveEventLoop) {
        self.app.tick(0.016);
        if self.app.reload_css_if_changed() || true {
            // Surface pattern animates; CSS mtime is cheap.
            if let Some(window) = &self.window {
                window.request_redraw();
            }
        }
    }
}

impl Host {
    fn redraw(&mut self) {
        let Some(window) = self.window.clone() else {
            return;
        };
        let Some(present) = self.present.as_mut() else {
            return;
        };
        let size = window.inner_size();
        let scale = window.scale_factor() as f32;
        self.app.scale = scale.max(0.01);
        self.app.css_w = size.width as f32 / self.app.scale;
        self.app.css_h = size.height as f32 / self.app.scale;
        let (quads, glyphs) = self.app.live_layers();
        let (w, h) = self.app.buffer_size();
        let time = self.app.time();
        let sub = self.hole.as_mut().is_some_and(|h| h.live());
        if let Some(hole) = self.hole.as_mut() {
            if let Some((x, y, cw, ch)) = self.app.surface_css_rect() {
                if let Some(f) = self.foreign.as_mut() {
                    f.set_size(cw.round() as i32, ch.round() as i32);
                }
                let copied = self
                    .foreign
                    .as_mut()
                    .and_then(|f| f.poll())
                    .map(|fr| (fr.argb.clone(), fr.width, fr.height));
                let foreign = copied
                    .as_ref()
                    .map(|(a, w, h)| (a.as_slice(), *w, *h));
                hole.update(x, y, cw, ch, self.app.scale, time, foreign);
            }
        }
        // Parent: GPU CSS boxes + glyphs. Child subsurface covers the well.
        present.frame(
            &quads,
            &glyphs,
            w,
            h,
            if sub {
                None
            } else {
                self.app.surface_device_rect()
            },
            time,
        );
    }
}

fn boot(app_id: &str) {
    let filter = tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| {
        format!("{}=info,sola_html_spike=info", app_id.replace('-', "_")).into()
    });
    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_writer(std::io::stderr)
        .init();
    tracing::info!("{app_id} starting");
    let socket = activate_wayland_session(20_000);
    tracing::info!(socket = %socket, "wayland socket resolved");
    if wait_for_wayland_socket(&socket, 10_000) {
        tracing::info!(socket = %socket, "wayland socket ready");
    } else {
        tracing::warn!(socket = %socket, "wayland socket not present after 10s");
    }
}

fn runtime_dir() -> PathBuf {
    std::env::var_os("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/tmp"))
}

fn activate_wayland_session(timeout_ms: u64) -> String {
    let display = resolve_wayland_display(timeout_ms);
    unsafe { std::env::set_var("WAYLAND_DISPLAY", &display) };
    display
}

fn resolve_wayland_display(timeout_ms: u64) -> String {
    let start = Instant::now();
    let interval = Duration::from_millis(500);
    loop {
        let path = runtime_dir().join("sola-wayland");
        if let Ok(raw) = std::fs::read_to_string(&path) {
            let name = raw.trim();
            if !name.is_empty() {
                return name.to_string();
            }
        }
        if start.elapsed().as_millis() as u64 >= timeout_ms {
            break;
        }
        std::thread::sleep(interval);
    }
    std::env::var("WAYLAND_DISPLAY")
        .ok()
        .filter(|v| !v.is_empty())
        .unwrap_or_else(|| "wayland-0".into())
}

fn wait_for_wayland_socket(display: &str, timeout_ms: u64) -> bool {
    let path = runtime_dir().join(display);
    let deadline = Instant::now() + Duration::from_millis(timeout_ms);
    loop {
        if path.exists() {
            return true;
        }
        if Instant::now() >= deadline {
            return false;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}
