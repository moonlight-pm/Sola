//! Volume popover — hosted in the Menu overlay.

use iced::widget::{column, container, mouse_area, row, slider, stack, text};
use iced::{Alignment, Color, Element, Length, Padding};

use sola_kit::components::button as kit_btn;
use sola_kit::components::icon_colored;
use sola_kit::components::popover;
use sola_kit::components::style::{SPACE_MD, SPACE_SM};
use sola_kit::fonts;

use crate::app::{Msg, Shell};
use crate::audio::{Device, Kind, UiMsg};

pub const CARD_WIDTH: f32 = 320.0;

fn dim(theme: &iced::Theme) -> iced::widget::text::Style {
    iced::widget::text::Style {
        color: Some(Color {
            a: 0.55,
            ..theme.palette().text
        }),
    }
}

pub fn panel(shell: &Shell) -> Element<'_, Msg> {
    let output_w = shell.output_size.map(|(w, _)| w as f32).unwrap_or(1920.0);
    let left = shell
        .estimate_audio_x()
        .min((output_w - CARD_WIDTH - 8.0).max(0.0))
        .max(0.0);

    let card: Element<'_, Msg> = popover(card_body(shell))
        .padding(SPACE_MD)
        .width(Length::Fixed(CARD_WIDTH))
        .into();

    let positioned: Element<'_, Msg> = container(card)
        .padding(Padding {
            top: 0.0,
            left,
            right: 0.0,
            bottom: 0.0,
        })
        .width(Length::Fill)
        .height(Length::Fill)
        .align_y(iced::alignment::Vertical::Top)
        .into();

    let backdrop: Element<'_, Msg> =
        mouse_area(container(text("")).width(Length::Fill).height(Length::Fill))
            .on_press(Msg::CloseMenu)
            .into();

    stack![backdrop, positioned]
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
}

fn card_body(shell: &Shell) -> Element<'_, Msg> {
    let snap = &shell.audio.snapshot;
    let muted_c = Color {
        a: 0.55,
        ..shell.theme.palette().text
    };

    column![
        section(
            "Output",
            snap.sink_volume,
            snap.sink_mute,
            &snap.sinks,
            snap.default_sink,
            Kind::Output,
            muted_c,
        ),
        section(
            "Input",
            snap.source_volume,
            snap.source_mute,
            &snap.sources,
            snap.default_source,
            Kind::Input,
            muted_c,
        ),
    ]
    .spacing(SPACE_MD)
    .width(Length::Fill)
    .into()
}

fn section<'a>(
    title: &'a str,
    volume: f32,
    mute: bool,
    devices: &'a [Device],
    default: Option<u32>,
    kind: Kind,
    muted_c: Color,
) -> Element<'a, Msg> {
    let pct = (volume.clamp(0.0, 1.0) * 100.0).round();
    let mute_icon = if mute {
        "lucide/volume-x"
    } else {
        "lucide/volume-2"
    };
    let (vol_msg, mute_msg): (fn(f32) -> Msg, Msg) = match kind {
        Kind::Output => (
            |v| Msg::AudioUi(UiMsg::OutputVolume(v)),
            Msg::AudioUi(UiMsg::ToggleOutputMute),
        ),
        Kind::Input => (
            |v| Msg::AudioUi(UiMsg::InputVolume(v)),
            Msg::AudioUi(UiMsg::ToggleInputMute),
        ),
    };

    let header: Element<'a, Msg> = row![
        text(title)
            .font(fonts::chrome())
            .size(11.0)
            .style(dim)
            .width(Length::Fill),
        text(format!("{pct:.0}%"))
            .font(fonts::chrome())
            .size(12.0)
            .style(dim),
    ]
    .align_y(Alignment::Center)
    .into();

    let sl: Element<'a, Msg> = slider(0.0..=100.0, pct, vol_msg)
        .step(1.0)
        .width(Length::Fill)
        .into();
    let mute_btn: Element<'a, Msg> = iced::widget::button(icon_colored(mute_icon, 14, muted_c))
        .padding([4, 6])
        .style(kit_btn::ghost)
        .on_press(mute_msg)
        .into();
    let controls: Element<'a, Msg> = row![sl, mute_btn]
        .spacing(SPACE_SM)
        .align_y(Alignment::Center)
        .into();

    let mut col = column![header, controls]
        .spacing(SPACE_SM)
        .width(Length::Fill);

    if devices.is_empty() {
        let empty = match kind {
            Kind::Output => "No output devices",
            Kind::Input => "No input devices",
        };
        col = col.push(text(empty).font(fonts::ui()).size(12.0).style(dim));
    } else {
        for d in devices {
            col = col.push(device_row(d, default == Some(d.id), kind));
        }
    }
    col.into()
}

fn device_row(d: &Device, selected: bool, kind: Kind) -> Element<'_, Msg> {
    let id = d.id;
    let press = match kind {
        Kind::Output => Msg::AudioUi(UiMsg::SetDefaultSink(id)),
        Kind::Input => Msg::AudioUi(UiMsg::SetDefaultSource(id)),
    };
    iced::widget::button(
        text(d.name.clone())
            .font(fonts::ui())
            .size(13.0)
            .width(Length::Fill),
    )
    .padding([4, 6])
    .style(kit_btn::list_item(selected))
    .on_press(press)
    .width(Length::Fill)
    .into()
}
