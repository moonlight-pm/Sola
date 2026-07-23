use iced::widget::{container, row};
use iced::{Element, Length};
use sola_kit::components::style::{SPACE_MD, SPACE_SM, SPACE_XL};
use sola_kit::components::text as kit_text;

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
        kit_text::caption(format!("model: {}", app.model)).style(kit_text::muted),
        kit_text::caption(format!("effort: {}", app.effort)).style(kit_text::muted),
        kit_text::caption(token_summary(&app.usage)).style(kit_text::muted),
    ]
    .spacing(SPACE_XL)
    .padding(SPACE_MD + SPACE_SM);
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
