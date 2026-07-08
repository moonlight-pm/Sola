use iced::widget::{container, row, text};
use iced::{Element, Length, Padding};

use crate::session::Usage;
use crate::{App, Msg};

pub(crate) fn token_summary(usage: &Usage) -> String {
    format!(
        "tokens: {} in / {} out",
        usage.input_tokens, usage.output_tokens
    )
}

pub(crate) fn view(app: &App) -> Element<'_, Msg> {
    let content = row![
        text(format!("model: {}", app.model))
            .size(12)
            .style(sola_kit::components::text::muted),
        text(format!("effort: {}", app.effort))
            .size(12)
            .style(sola_kit::components::text::muted),
        text(token_summary(&app.usage))
            .size(12)
            .style(sola_kit::components::text::muted),
    ]
    .spacing(16)
    .padding(Padding::new(10.0));
    container(content).width(Length::Fill).into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn token_summary_formats_counts() {
        let u = Usage {
            input_tokens: 12,
            output_tokens: 34,
        };
        assert_eq!(token_summary(&u), "tokens: 12 in / 34 out");
    }
}
