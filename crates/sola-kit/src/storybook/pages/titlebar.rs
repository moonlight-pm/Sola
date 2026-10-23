//! Window frame — the titlebar with menus every kit window wears (Next,
//! boards 01 and 02): tiled focused, tiled unfocused, floating.

use iced::widget::{column, container, row, text};
use iced::{Background, Element, Length, Theme};

use sola_bus::topics::{AppMenuPayload, MenuDefinition, MenuItem};
use sola_kit::components::text::{body, muted};
use sola_kit::components::{Chrome, WindowFrame};

use crate::storybook::Msg;
use crate::storybook::pages::chrome::{lede, scene};

pub fn view() -> Element<'static, Msg> {
    column![
        lede(
            "Window frame",
            "Every window's frame: a 38 px titlebar with the close dot, the app's menus (its name in bold first, Window last) and the title right-aligned; 12 px corners; a 1 px rim lit while focused. A tile is opaque and paints its corners the ground; a float clears them and resizes from its edges. The body is full-bleed on purpose: the accent band must still meet the curve.",
        ),
        scene("Tiled · focused, then unfocused"),
        row![
            sample(Chrome::Tiled, true, "Inbox · 12 unread"),
            sample(Chrome::Tiled, false, "Inbox · 12 unread"),
        ]
        .spacing(20),
        scene("Floating · a long title gives way first"),
        sample(
            Chrome::Floating,
            true,
            "Re: the quarterly review of every window frame in the kit"
        ),
        text("Menus open here (their items do nothing); close and drag are inert.")
            .size(12)
            .style(muted),
    ]
    .spacing(16)
    .into()
}

fn sample(chrome: Chrome, focused: bool, title: &'static str) -> Element<'static, Msg> {
    let frame = WindowFrame::new(chrome, bleed())
        .title(title)
        .menus(demo_menus())
        .focused(focused)
        .on_drag(Msg::Noop)
        .on_close(Msg::Noop)
        .on_resize(|_| Msg::Noop);
    container(frame)
        .width(Length::Fixed(440.0))
        .height(Length::Fixed(240.0))
        .into()
}

/// Mail's menus. The demo's own Window menu stands in for the real one,
/// whose items would move the storybook's window.
fn demo_menus() -> AppMenuPayload {
    let item = |id: &str, label: &str, checked: bool| MenuItem::Action {
        id: format!("demo.{id}"),
        label: label.into(),
        shortcut: None,
        disabled: false,
        checked,
    };
    let menu = |label: &str, items: Vec<MenuItem>| MenuDefinition {
        label: label.into(),
        items,
    };
    AppMenuPayload {
        app_id: "sola-kit-demo".into(),
        menus: vec![
            menu("Mail", vec![item("about", "About Mail", false), MenuItem::Divider, item("quit", "Quit Mail", false)]),
            menu("Message", vec![item("new", "New Message", false), item("reply", "Reply", false), item("forward", "Forward", false)]),
            menu("View", vec![item("threads", "Threads", true), item("unread", "Unread Only", false)]),
            menu("Window", vec![item("hide", "Hide", false)]),
        ],
    }
}

fn bleed() -> Element<'static, Msg> {
    container(
        column![
            row_block("Inbox"),
            row_block("Drafts"),
            row_block("Sent"),
            container(text("").size(1))
                .width(Length::Fill)
                .height(Length::Fill),
            container(body("Last row meets the curve"))
                .padding([10, 14])
                .width(Length::Fill)
                .style(|theme: &Theme| {
                    let p = theme.extended_palette();
                    iced::widget::container::Style {
                        background: Some(Background::Color(p.primary.base.color)),
                        text_color: Some(p.primary.base.text),
                        ..Default::default()
                    }
                }),
        ]
        .width(Length::Fill)
        .height(Length::Fill),
    )
    .width(Length::Fill)
    .height(Length::Fill)
    .style(|theme: &Theme| iced::widget::container::Style {
        background: Some(Background::Color(theme.extended_palette().background.strong.color)),
        ..Default::default()
    })
    .into()
}

fn row_block(label: &'static str) -> Element<'static, Msg> {
    container(body(label))
        .padding([10, 14])
        .width(Length::Fill)
        .into()
}
