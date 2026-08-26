//! THESIS: the lattice is the tool. No sidebar, no cards — a sheet of
//! magnified pixels with a locked mark on the hot pixel.
//! OWN-WORLD: sola-kit graphite chrome; mono for measurement; accent only
//! on the hot cell.
//! STORY: move the pointer anywhere; read hex, RGB, and coordinates.
//! FIRST VIEWPORT: zoom − / + over a square pixel lattice; readout strip
//! with swatch + hex + RGB + x,y.
//! FORM: Operate loupe inside the established Sola kit world.
//! FINISH: unreviewed and undocumented is unfinished; this build ends
//! with progress docs for the first-pass app.

use std::sync::Arc;
use std::time::Duration;

use iced::keyboard;
use iced::widget::canvas::Cache;
use iced::widget::{Space, column, container, mouse_area, row, text};
use iced::{
    Alignment, Background, Border, Color, Element, Event, Length, Padding, Subscription, Task,
    Theme, event,
};

use sola_bus::Message;
use sola_bus::topics::Topic;
use sola_kit::app::{apply_theme_update, bus_subscription, is_self_quit};
use sola_kit::components::icon::icon_handle;
use sola_kit::components::style::{
    CHROME_SURFACE, HAIRLINE_A, SPACE_LG, SPACE_MD, SPACE_SM, SPACE_XL, mix_white,
};
use sola_kit::components::text as kit_text;
use sola_kit::components::{swatch_sized, toolbar_icon_tip};
use sola_kit::fonts;
use sola_kit::theme::default_theme;

use crate::APP_ID;
use crate::grid::{self, Patch, ZOOM_DEFAULT, ZOOM_MAX, ZOOM_MIN};
use crate::sample;

const TOOLBAR_H: f32 = 48.0;
const READOUT_H: f32 = 56.0;
const SAMPLE_MS: u64 = 100;
const COPY_FEEDBACK_MS: u64 = 1500;

pub struct App {
    zoom: u32,
    patch: Option<Patch>,
    inflight: bool,
    /// Sample again as soon as the in-flight call returns (don't wait a tick).
    pending: bool,
    error: Option<String>,
    copied: bool,
    copied_gen: u64,
    cache: Cache,
    zoom_in: iced::widget::svg::Handle,
    zoom_out: iced::widget::svg::Handle,
    theme: Theme,
    float: sola_kit::FloatState,
    window_id: Option<iced::window::Id>,
}

impl Default for App {
    fn default() -> Self {
        Self {
            zoom: ZOOM_DEFAULT,
            patch: None,
            inflight: false,
            pending: false,
            error: None,
            copied: false,
            copied_gen: 0,
            cache: Cache::new(),
            zoom_in: icon_handle("lucide/zoom-in"),
            zoom_out: icon_handle("lucide/zoom-out"),
            theme: default_theme(),
            float: sola_kit::FloatState::new(APP_ID),
            window_id: None,
        }
    }
}

#[derive(Debug, Clone)]
pub enum Msg {
    Bus(Arc<Message>),
    Tick,
    SampleDone(Result<Patch, String>),
    ZoomIn,
    ZoomOut,
    CopyColor,
    ClearCopied(u64),
    KeyPressed(keyboard::Key, keyboard::Modifiers),
    WindowReady(Option<iced::window::Id>),
    TitleDrag,
    TitleResize(iced::window::Direction),
    TitleClose,
}

impl App {
    pub fn boot() -> (Self, Task<Msg>) {
        (
            Self::default(),
            sola_kit::window_ready_task(Msg::WindowReady),
        )
    }

    pub fn title(&self) -> String {
        "Scope".into()
    }

    pub fn theme(&self) -> Theme {
        sola_kit::theme_for(self.float.is_floating_any(), &self.theme)
    }

    pub fn subscription(&self) -> Subscription<Msg> {
        Subscription::batch([
            bus_subscription().map(Msg::Bus),
            iced::time::every(Duration::from_millis(SAMPLE_MS)).map(|_| Msg::Tick),
            event::listen_with(|event, _status, _id| match event {
                Event::Keyboard(keyboard::Event::KeyPressed { key, modifiers, .. }) => {
                    Some(Msg::KeyPressed(key, modifiers))
                }
                _ => None,
            }),
        ])
    }

    pub fn update(&mut self, msg: Msg) -> Task<Msg> {
        match msg {
            Msg::Bus(message) => {
                self.float.update(&message);
                apply_theme_update(&message, &mut self.theme);
                if is_self_quit(&message, APP_ID) {
                    return iced::exit();
                }
                match Topic::parse(&message) {
                    Some(Topic::MenuAction(p)) if p.app_id == APP_ID => {
                        return self.on_menu(&p.action_id);
                    }
                    _ => {}
                }
            }
            Msg::Tick => {
                if self.inflight {
                    self.pending = true;
                } else {
                    return self.request_sample();
                }
            }
            Msg::SampleDone(result) => {
                self.inflight = false;
                match result {
                    Ok(patch) => {
                        self.patch = Some(patch);
                        self.error = None;
                        self.cache.clear();
                    }
                    Err(e) => {
                        if e != "sample already in progress" {
                            self.error = Some(e);
                        }
                    }
                }
                if self.pending {
                    self.pending = false;
                    return self.request_sample();
                }
            }
            Msg::ZoomIn => self.set_zoom(self.zoom.saturating_add(1)),
            Msg::ZoomOut => self.set_zoom(self.zoom.saturating_sub(1)),
            Msg::CopyColor => return self.copy_color(),
            Msg::ClearCopied(token) => {
                if self.copied_gen == token {
                    self.copied = false;
                }
            }
            Msg::KeyPressed(key, mods) => return self.on_key(key, mods),
            Msg::WindowReady(id) => self.window_id = id,
            Msg::TitleDrag => return sola_kit::drag(self.window_id),
            Msg::TitleResize(dir) => return sola_kit::drag_resize(self.window_id, dir),
            Msg::TitleClose => sola_kit::close_app(APP_ID),
        }
        Task::none()
    }

    fn set_zoom(&mut self, zoom: u32) {
        let zoom = zoom.clamp(ZOOM_MIN, ZOOM_MAX);
        if zoom == self.zoom {
            return;
        }
        self.zoom = zoom;
        self.cache.clear();
    }

    fn request_sample(&mut self) -> Task<Msg> {
        self.inflight = true;
        let size = grid::sample_size(self.zoom);
        Task::perform(
            async move {
                match tokio::task::spawn_blocking(move || sample::fetch(size)).await {
                    Ok(r) => r,
                    Err(e) => Err(e.to_string()),
                }
            },
            Msg::SampleDone,
        )
    }

    fn on_menu(&mut self, action: &str) -> Task<Msg> {
        match action {
            "quit" => iced::exit(),
            "zoom_in" => {
                self.set_zoom(self.zoom.saturating_add(1));
                Task::none()
            }
            "zoom_out" => {
                self.set_zoom(self.zoom.saturating_sub(1));
                Task::none()
            }
            "copy" => self.copy_color(),
            _ => Task::none(),
        }
    }

    fn on_key(&mut self, key: keyboard::Key, mods: keyboard::Modifiers) -> Task<Msg> {
        match key.as_ref() {
            keyboard::Key::Character("=") | keyboard::Key::Character("+") => {
                self.set_zoom(self.zoom.saturating_add(1));
            }
            keyboard::Key::Character("-") => {
                self.set_zoom(self.zoom.saturating_sub(1));
            }
            keyboard::Key::Character("c") if mods.command() => return self.copy_color(),
            _ => {}
        }
        Task::none()
    }

    fn copy_color(&mut self) -> Task<Msg> {
        let Some([r, g, b, _]) = self.patch.as_ref().and_then(Patch::hot_rgba) else {
            return Task::none();
        };
        let hex = format!("#{r:02X}{g:02X}{b:02X}");
        self.copied_gen = self.copied_gen.wrapping_add(1);
        let token = self.copied_gen;
        self.copied = true;
        Task::batch([
            iced::clipboard::write(hex),
            Task::perform(
                async move {
                    tokio::time::sleep(Duration::from_millis(COPY_FEEDBACK_MS)).await;
                    token
                },
                Msg::ClearCopied,
            ),
        ])
    }

    pub fn view(&self) -> Element<'_, Msg> {
        let p = self.theme.extended_palette();
        let hairline = mix_white(p.background.weaker.color, HAIRLINE_A);
        let grid = grid::view(
            self.patch.as_ref(),
            &self.cache,
            p.background.weakest.color,
            hairline,
            p.primary.base.color,
        );

        let body = column![self.toolbar(), grid, self.readout()]
            .width(Length::Fill)
            .height(Length::Fill);

        sola_kit::wrap_if_floating(
            self.float.is_floating_any(),
            "Scope",
            Msg::TitleDrag,
            Msg::TitleClose,
            Msg::TitleResize,
            body.into(),
        )
    }

    fn toolbar(&self) -> Element<'_, Msg> {
        let zoom_out = toolbar_icon_tip(
            self.zoom_out.clone(),
            "Zoom out",
            (self.zoom > ZOOM_MIN).then_some(Msg::ZoomOut),
        );
        let zoom_in = toolbar_icon_tip(
            self.zoom_in.clone(),
            "Zoom in",
            (self.zoom < ZOOM_MAX).then_some(Msg::ZoomIn),
        );
        let size = grid::sample_size(self.zoom);
        let field = text(format!("{size}×{size}"))
            .font(fonts::mono())
            .size(12)
            .style(kit_text::muted);

        let coords: Element<'_, Msg> = match self.patch.as_ref() {
            Some(patch) => text(format!("{}, {}", patch.x, patch.y))
                .font(fonts::mono())
                .size(12)
                .into(),
            None => text("—")
                .font(fonts::mono())
                .size(12)
                .style(kit_text::muted)
                .into(),
        };

        container(
            row![
                zoom_out,
                field,
                zoom_in,
                Space::new().width(Length::Fill),
                coords,
            ]
            .spacing(SPACE_MD)
            .align_y(Alignment::Center)
            .width(Length::Fill)
            .padding(Padding {
                top: SPACE_SM,
                right: SPACE_LG,
                bottom: SPACE_SM,
                left: SPACE_LG,
            }),
        )
        .width(Length::Fill)
        .height(Length::Fixed(TOOLBAR_H))
        .center_y(Length::Fixed(TOOLBAR_H))
        .style(chrome_style)
        .into()
    }

    fn readout(&self) -> Element<'_, Msg> {
        let content: Element<'_, Msg> = if let Some(patch) = self.patch.as_ref() {
            if let Some([r, g, b, _]) = patch.hot_rgba() {
                let hex = format!("#{r:02X}{g:02X}{b:02X}");
                let rgb = format!("{r:>3}  {g:>3}  {b:>3}");
                let swatch = swatch_sized(Color::from_rgb8(r, g, b), 28.0);
                let hex_label = if self.copied {
                    text("Copied")
                        .font(fonts::ui_medium())
                        .size(13)
                        .style(kit_text::accent)
                } else {
                    text(hex).font(fonts::mono()).size(13)
                };
                let rgb_label = text(rgb)
                    .font(fonts::mono())
                    .size(12)
                    .style(kit_text::muted);
                mouse_area(
                    row![swatch, hex_label, rgb_label]
                        .spacing(SPACE_LG)
                        .align_y(Alignment::Center),
                )
                .on_press(Msg::CopyColor)
                .into()
            } else {
                text("No pixel").size(12).style(kit_text::muted).into()
            }
        } else {
            let msg = self
                .error
                .as_deref()
                .unwrap_or("Move the pointer over the desktop");
            text(msg).size(12).style(kit_text::muted).into()
        };

        container(container(content).padding(Padding {
            top: SPACE_MD,
            right: SPACE_XL,
            bottom: SPACE_MD,
            left: SPACE_XL,
        }))
        .width(Length::Fill)
        .height(Length::Fixed(READOUT_H))
        .center_y(Length::Fixed(READOUT_H))
        .style(chrome_style)
        .into()
    }
}

fn chrome_style(_theme: &Theme) -> container::Style {
    let surface = CHROME_SURFACE;
    container::Style {
        background: Some(Background::Color(surface)),
        border: Border {
            color: mix_white(surface, HAIRLINE_A),
            width: 1.0,
            radius: 0.0.into(),
        },
        ..container::Style::default()
    }
}
