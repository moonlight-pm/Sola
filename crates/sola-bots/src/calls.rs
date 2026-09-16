//! Call-plane owner `bots`.

use sola_call::{ArgSpec, ArgType, Incoming, MethodSpec};

use crate::host::Host;
use std::sync::Arc;

pub const OWNER: &str = "bots";
pub const SEND_TIMEOUT_MS: u64 = 8_000;

pub fn methods() -> Vec<MethodSpec> {
    vec![
        method("list", "List bots and status", &[]),
        method(
            "poll",
            "List bots and optional transcript in one call",
            &[opt_s("bot", Some('b'), "Bot id, slug, or name")],
        ),
        method(
            "new",
            "Create a named bot (`~/Bots/<slug>/`)",
            &[req_s("name", Some('n'), "Display name")],
        ),
        method_ms(
            "send",
            "Send a message (returns while the turn runs)",
            &[
                req_s("bot", Some('b'), "Bot id, slug, or name"),
                req_s("text", Some('t'), "Message"),
            ],
            SEND_TIMEOUT_MS,
        ),
        method(
            "transcript",
            "Dialog turns for a bot",
            &[req_s("bot", Some('b'), "Bot id, slug, or name")],
        ),
        method(
            "rm",
            "Delete a bot: catalog, home directory, Grok session",
            &[req_s("bot", Some('b'), "Bot id, slug, or name")],
        ),
        method(
            "cancel",
            "Cancel the current turn",
            &[req_s("bot", Some('b'), "Bot id, slug, or name")],
        ),
    ]
}

pub fn dispatch(host: &Arc<Host>, incoming: Incoming) {
    let Incoming {
        method,
        params,
        reply,
    } = incoming;
    let str_arg = |k: &str| {
        params
            .get(k)
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
            .or_else(|| {
                params.get(k).and_then(|v| v.as_i64()).map(|n| n.to_string())
            })
    };
    match method.as_str() {
        "list" => reply.ok(host.list_json()),
        "poll" => reply.ok(host.poll_json(str_arg("bot").as_deref())),
        "new" => match str_arg("name") {
            Some(name) => match host.create(&name) {
                Ok(v) => reply.ok(v),
                Err(e) => reply.err(e),
            },
            None => reply.err("name required"),
        },
        "send" => {
            let bot = str_arg("bot");
            let text = str_arg("text");
            match (bot, text) {
                (Some(b), Some(t)) => match host.send(&b, &t) {
                    Ok(v) => reply.ok(v),
                    Err(e) => reply.err(e),
                },
                _ => reply.err("bot and text required"),
            }
        }
        "transcript" => match str_arg("bot") {
            Some(b) => match host.transcript_json(&b) {
                Ok(v) => reply.ok(v),
                Err(e) => reply.err(e),
            },
            None => reply.err("bot required"),
        },
        "rm" => match str_arg("bot") {
            Some(b) => match host.rm(&b) {
                Ok(v) => reply.ok(v),
                Err(e) => reply.err(e),
            },
            None => reply.err("bot required"),
        },
        "cancel" => match str_arg("bot") {
            Some(b) => match host.cancel(&b) {
                Ok(v) => reply.ok(v),
                Err(e) => reply.err(e),
            },
            None => reply.err("bot required"),
        },
        other => reply.err(format!("unknown method {other}")),
    }
}

fn method(name: &str, summary: &str, args: &[ArgSpec]) -> MethodSpec {
    MethodSpec {
        name: name.into(),
        summary: summary.into(),
        args: args.to_vec(),
        timeout_ms: None,
    }
}

fn method_ms(name: &str, summary: &str, args: &[ArgSpec], timeout_ms: u64) -> MethodSpec {
    MethodSpec {
        name: name.into(),
        summary: summary.into(),
        args: args.to_vec(),
        timeout_ms: Some(timeout_ms),
    }
}

fn opt_s(name: &str, short: Option<char>, help: &str) -> ArgSpec {
    ArgSpec {
        name: name.into(),
        long: Some(name.into()),
        short,
        ty: ArgType::String,
        required: false,
        help: help.into(),
    }
}

fn req_s(name: &str, short: Option<char>, help: &str) -> ArgSpec {
    ArgSpec {
        name: name.into(),
        long: Some(name.into()),
        short,
        ty: ArgType::String,
        required: true,
        help: help.into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names() {
        let methods = methods();
        let n: Vec<_> = methods.iter().map(|m| m.name.as_str()).collect();
        assert_eq!(
            n,
            ["list", "poll", "new", "send", "transcript", "rm", "cancel"]
        );
    }
}
