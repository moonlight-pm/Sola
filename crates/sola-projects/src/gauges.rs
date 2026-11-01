//! What Claude's status line says about a session (Joshua, 2026-11-01:
//! "credits quota/remaining, model effort, fast (on/off), context should
//! show percent used"): Claude Code hands its status line command the
//! context window's use, the account's usage limits, the effort level and
//! fast mode — none of which its hooks or transcript carry. Projects
//! installs that command ([`crate::hooks`]); it writes the payload to the
//! hook log as a `gauge` line when one of these changes, and every
//! computer watching the session reads it there.

use serde_json::Value;

/// A session's gauges, as its last status line payload said.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Gauges {
    /// Context used, % of the window (`context_window.used_percentage`).
    pub context_pct: Option<u8>,
    /// The window, in tokens (`context_window.context_window_size`).
    pub window: Option<u64>,
    /// `effort.level`: low, medium, high, xhigh, max.
    pub effort: Option<String>,
    /// `fast_mode`.
    pub fast: Option<bool>,
    /// The subscription's limits, % used: the 5-hour and the 7-day.
    pub five_hour: Option<u8>,
    pub seven_day: Option<u8>,
    /// A gateway's spend limit: dollars used and the limit.
    pub spend: Option<(u64, u64)>,
}

fn pct(v: &Value) -> Option<u8> {
    v.as_f64().map(|p| p.clamp(0.0, 100.0).round() as u8)
}

impl Gauges {
    /// Read a status line payload (Claude Code's documented schema; a field
    /// it leaves out stays `None`).
    pub fn from_statusline(v: &Value) -> Self {
        let cw = &v["context_window"];
        let rl = &v["rate_limits"];
        let spend = &rl["spend_limit"];
        Self {
            context_pct: pct(&cw["used_percentage"]),
            window: cw["context_window_size"].as_u64(),
            effort: v["effort"]["level"].as_str().map(str::to_string),
            fast: v["fast_mode"].as_bool(),
            five_hour: pct(&rl["five_hour"]["used_percentage"]),
            seven_day: pct(&rl["seven_day"]["used_percentage"]),
            spend: spend["used_usd"]
                .as_f64()
                .zip(spend["limit_usd"].as_f64())
                .map(|(u, l)| (u.round() as u64, l.round() as u64)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_status_line_payload_gives_the_gauges() {
        let v: Value = serde_json::from_str(
            r#"{"session_id":"s","model":{"id":"claude-opus-5-5"},
                "context_window":{"context_window_size":1000000,"used_percentage":44.6},
                "rate_limits":{"five_hour":{"used_percentage":32.2,"resets_at":1},
                               "seven_day":{"used_percentage":18,"resets_at":2}},
                "effort":{"level":"xhigh"},"fast_mode":false}"#,
        )
        .unwrap();
        assert_eq!(
            Gauges::from_statusline(&v),
            Gauges {
                context_pct: Some(45),
                window: Some(1_000_000),
                effort: Some("xhigh".into()),
                fast: Some(false),
                five_hour: Some(32),
                seven_day: Some(18),
                spend: None,
            }
        );
    }

    #[test]
    fn what_the_payload_leaves_out_stays_unknown() {
        let v: Value = serde_json::from_str(r#"{"session_id":"s"}"#).unwrap();
        assert_eq!(Gauges::from_statusline(&v), Gauges::default());
    }
}
