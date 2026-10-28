//! The AI behind a bot, chosen when the bot is made and fixed after.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Ai {
    /// `grok agent stdio` (ACP) on the Grok CLI's own login.
    Grok,
    /// Claude Code (`claude -p`) on its own login: the subscription.
    Claude,
    /// Gab AI's chat API (OpenAI-shaped), with a key. Chat only, no tools.
    Gab,
}

impl Ai {
    pub const ALL: [Ai; 3] = [Ai::Grok, Ai::Claude, Ai::Gab];

    /// Wire and `bot.json` name.
    pub fn id(self) -> &'static str {
        match self {
            Ai::Grok => "grok",
            Ai::Claude => "claude",
            Ai::Gab => "gab",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Ai::Grok => "Grok",
            Ai::Claude => "Claude",
            Ai::Gab => "Gab AI",
        }
    }

    pub fn parse(s: &str) -> Option<Ai> {
        match s.trim().to_ascii_lowercase().as_str() {
            "grok" => Some(Ai::Grok),
            "claude" => Some(Ai::Claude),
            "gab" | "gab.ai" | "gab ai" | "gabai" => Some(Ai::Gab),
            _ => None,
        }
    }

    /// Whether the bot runs as an agent in its home (shell, files, the
    /// browser through `solactl`), or only chats.
    pub fn has_tools(self) -> bool {
        !matches!(self, Ai::Gab)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_ids_and_labels() {
        for ai in Ai::ALL {
            assert_eq!(Ai::parse(ai.id()), Some(ai));
            assert_eq!(Ai::parse(ai.label()), Some(ai));
        }
        assert_eq!(Ai::parse("gab.ai"), Some(Ai::Gab));
        assert_eq!(Ai::parse("gpt"), None);
    }
}
