//! Bluetooth popover — hosted in the Menu overlay.

use iced::widget::{column, container, mouse_area, row, stack, text, toggler};
use iced::{Alignment, Element, Length, Padding};

use sola_kit::components::button as kit_btn;
use sola_kit::components::form::{form_row, toggle_style};
use sola_kit::components::popover;
use sola_kit::components::style::{SPACE_MD, SPACE_SM, SPACE_XS};
use sola_kit::components::text as kit_text;
use sola_kit::components::text_input as kit_text_input;
use sola_kit::fonts;

use crate::app::{Msg, Shell};
use crate::bluetooth::{AgentKind, Device, Ui};

pub const CARD_WIDTH: f32 = 320.0;

/// Dim text derived from `palette().text` (same trap calendar/stats avoid:
/// kit `muted` on the popover face can vanish).
fn dim(theme: &iced::Theme) -> iced::widget::text::Style {
    iced::widget::text::Style {
        color: Some(iced::Color {
            a: 0.55,
            ..theme.palette().text
        }),
    }
}

pub fn panel(shell: &Shell) -> Element<'_, Msg> {
    let output_w = shell.output_size.map(|(w, _)| w as f32).unwrap_or(1920.0);
    let left = shell
        .estimate_bluetooth_x()
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
    let ui = &shell.bluetooth;
    let powered = ui
        .snapshot
        .adapter
        .as_ref()
        .map(|a| a.powered)
        .unwrap_or(false);

    let power: Element<'_, Msg> = form_row(
        "Bluetooth",
        toggler(powered)
            .on_toggle(|on| Msg::BluetoothUi(crate::bluetooth::UiMsg::Power(on)))
            .style(toggle_style),
    )
    .into();

    let mut col = column![power].spacing(SPACE_MD).width(Length::Fill);

    if let Some(n) = ui.notice.as_deref() {
        col = col.push(
            text(n.to_string())
                .font(fonts::ui())
                .size(12.0)
                .style(kit_text::danger),
        );
    }

    if let Some(prompt) = ui.prompt.as_ref() {
        col = col.push(agent_block(ui, prompt));
        return col.into();
    }

    if !powered {
        col = col.push(
            text("Bluetooth is off")
                .font(fonts::ui())
                .size(12.0)
                .style(dim),
        );
        return col.into();
    }

    let connected = ui.snapshot.connected();
    if connected.is_empty() && ui.snapshot.paired_idle().is_empty() && !ui.adding {
        col = col.push(
            text("No connected devices")
                .font(fonts::ui())
                .size(12.0)
                .style(dim),
        );
    } else {
        for d in connected {
            col = col.push(device_row(ui, d, RowKind::Disconnect));
        }
        let idle = ui.snapshot.paired_idle();
        if !idle.is_empty() {
            col = col.push(section_label("Not connected"));
            for d in idle {
                col = col.push(device_row(ui, d, RowKind::Connect));
            }
        }
    }

    if ui.adding {
        let nearby = ui.snapshot.nearby();
        col = col.push(section_label("Nearby"));
        if nearby.is_empty() {
            let searching = ui
                .snapshot
                .adapter
                .as_ref()
                .map(|a| a.discovering)
                .unwrap_or(false);
            let copy = if searching {
                "Searching…"
            } else {
                "No devices found"
            };
            col = col.push(text(copy).font(fonts::ui()).size(12.0).style(dim));
        } else {
            for d in nearby {
                col = col.push(device_row(ui, d, RowKind::Pair));
            }
        }
        col = col.push(
            kit_btn::labeled_sm("Done", kit_btn::ghost)
                .on_press(Msg::BluetoothUi(crate::bluetooth::UiMsg::DoneAdding)),
        );
    } else {
        col = col.push(
            kit_btn::labeled_sm("Add device", kit_btn::secondary)
                .on_press(Msg::BluetoothUi(crate::bluetooth::UiMsg::Add)),
        );
    }

    col.into()
}

fn section_label<'a>(label: &'a str) -> Element<'a, Msg> {
    text(label)
        .font(fonts::chrome())
        .size(11.0)
        .style(dim)
        .into()
}

enum RowKind {
    Disconnect,
    Connect,
    Pair,
}

fn device_row<'a>(ui: &'a Ui, d: &'a Device, kind: RowKind) -> Element<'a, Msg> {
    let busy = ui.busy_path.as_deref() == Some(d.path.as_str());
    let name: Element<'a, Msg> = text(d.alias.clone())
        .font(fonts::ui())
        .size(13.0)
        .width(Length::Fill)
        .into();

    let mut info = row![name].spacing(SPACE_SM).align_y(Alignment::Center);

    if let Some(bat) = d.battery_label() {
        info = info.push(text(bat).font(fonts::chrome()).size(12.0).style(dim));
    }

    let action: Element<'a, Msg> = if busy {
        text("…").font(fonts::ui()).size(12.0).style(dim).into()
    } else {
        match kind {
            RowKind::Disconnect => kit_btn::labeled_sm("Disconnect", kit_btn::ghost)
                .on_press(Msg::BluetoothUi(crate::bluetooth::UiMsg::Disconnect(
                    d.path.clone(),
                )))
                .into(),
            RowKind::Connect => kit_btn::labeled_sm("Connect", kit_btn::ghost)
                .on_press(Msg::BluetoothUi(crate::bluetooth::UiMsg::Connect(
                    d.path.clone(),
                )))
                .into(),
            RowKind::Pair => kit_btn::labeled_sm("Pair", kit_btn::secondary)
                .on_press(Msg::BluetoothUi(crate::bluetooth::UiMsg::Pair(
                    d.path.clone(),
                )))
                .into(),
        }
    };

    row![info.width(Length::Fill), action]
        .spacing(SPACE_SM)
        .align_y(Alignment::Center)
        .width(Length::Fill)
        .into()
}

fn agent_block<'a>(ui: &'a Ui, prompt: &'a crate::bluetooth::AgentPrompt) -> Element<'a, Msg> {
    let name = prompt.device_name.as_str();
    let (title, detail, needs_input): (String, Option<String>, bool) = match &prompt.kind {
        AgentKind::ConfirmPasskey(n) => (
            format!("Confirm passkey for {name}"),
            Some(format!("{n:06}")),
            false,
        ),
        AgentKind::RequestPin => (format!("Enter PIN for {name}"), None, true),
        AgentKind::RequestPasskey => (format!("Enter passkey for {name}"), None, true),
        AgentKind::DisplayPin(pin) => (
            format!("Enter this code on {name}"),
            Some(pin.clone()),
            false,
        ),
        AgentKind::DisplayPasskey { passkey, entered } => (
            format!("Passkey for {name}"),
            Some(format!("{passkey:06}  ({entered} entered)")),
            false,
        ),
        AgentKind::Authorize => (format!("Allow {name} to pair?"), None, false),
    };

    let mut col = column![text(title).font(fonts::ui_medium()).size(13.0),]
        .spacing(SPACE_SM)
        .width(Length::Fill);

    if let Some(detail) = detail {
        col = col.push(
            text(detail)
                .font(fonts::mono())
                .size(18.0)
                .width(Length::Fill),
        );
    }

    if needs_input {
        col = col.push(
            kit_text_input::text_input("PIN", &ui.pin_input)
                .on_input(|s| Msg::BluetoothUi(crate::bluetooth::UiMsg::PinInput(s)))
                .on_submit(Msg::BluetoothUi(crate::bluetooth::UiMsg::AgentAccept))
                .style(kit_text_input::style)
                .padding([6, 8]),
        );
    }

    let show_actions = !matches!(
        prompt.kind,
        AgentKind::DisplayPin(_) | AgentKind::DisplayPasskey { .. }
    );
    if show_actions {
        let confirm_label = match prompt.kind {
            AgentKind::ConfirmPasskey(_) | AgentKind::Authorize => "Confirm",
            AgentKind::RequestPin | AgentKind::RequestPasskey => "Pair",
            _ => "OK",
        };
        col = col.push(
            row![
                kit_btn::labeled_sm(confirm_label, kit_btn::primary)
                    .on_press(Msg::BluetoothUi(crate::bluetooth::UiMsg::AgentAccept)),
                kit_btn::labeled_sm("Cancel", kit_btn::ghost)
                    .on_press(Msg::BluetoothUi(crate::bluetooth::UiMsg::AgentReject)),
            ]
            .spacing(SPACE_XS),
        );
    }

    col.into()
}
