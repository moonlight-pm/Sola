//! sola-browser-wpe — custom browser with iced chrome and embedded
//! WPEWebKit webviews. Phase-0 skeleton: opens an empty iced window
//! and exits cleanly. Subsequent phases add the wgpu DMA-BUF import
//! path (0a), a WPE worker subprocess that exports frames (0b), the
//! main-process glue that imports those frames into wgpu (0c), and
//! input forwarding (0d). See `docs/specs/2026-05-21-sola-browser-wpe.md`
//! (TODO) for the full plan.

use iced::widget::container;
use iced::{Element, Length, Task};

const APP_ID: &str = "sola-browser-wpe";

fn main() -> iced::Result {
    sola_core::log::init(APP_ID);
    tracing::info!("{APP_ID} starting (skeleton)");

    let _ = sola_core::env::activate_wayland_session(10_000);

    iced::application(App::default, App::update, App::view)
        .title(|_: &App| String::from(APP_ID))
        .window(iced::window::Settings {
            decorations: false,
            platform_specific: iced::window::settings::PlatformSpecific {
                application_id: APP_ID.into(),
                ..Default::default()
            },
            ..iced::window::Settings::default()
        })
        .run()
}

#[derive(Default)]
struct App;

#[derive(Debug, Clone)]
enum Msg {}

impl App {
    fn update(&mut self, _: Msg) -> Task<Msg> {
        Task::none()
    }

    fn view(&self) -> Element<'_, Msg> {
        container(iced::widget::Space::new().width(Length::Fill).height(Length::Fill))
            .width(Length::Fill)
            .height(Length::Fill)
            .into()
    }
}
