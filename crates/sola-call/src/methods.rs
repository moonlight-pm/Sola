//! Advertised method specs for first-party owners.

use crate::protocol::{ArgSpec, ArgType, MethodSpec};

pub const OWNER_COMPOSITOR: &str = "compositor";
pub const OWNER_SESSION: &str = "session";

pub fn compositor_methods() -> Vec<MethodSpec> {
    vec![
        MethodSpec {
            name: "screenshot".into(),
            summary: "Capture a PNG of the output or a window".into(),
            args: vec![
                arg("output", Some("output"), Some('o'), ArgType::Path, false, "PNG path"),
                arg("app", Some("app"), Some('a'), ArgType::String, false, "app id"),
                arg("window", Some("window"), Some('w'), ArgType::String, false, "window title"),
                arg("x", Some("x"), None, ArgType::Int, false, "region x"),
                arg("y", Some("y"), None, ArgType::Int, false, "region y"),
                arg("width", Some("width"), None, ArgType::Int, false, "region width"),
                arg("height", Some("height"), None, ArgType::Int, false, "region height"),
            ],
        },
        MethodSpec {
            name: "windows".into(),
            summary: "List known windows grouped by app id".into(),
            args: vec![],
        },
        MethodSpec {
            name: "input.click".into(),
            summary: "Move and click at absolute output coordinates".into(),
            args: vec![
                arg("x", None, None, ArgType::Int, true, "x"),
                arg("y", None, None, ArgType::Int, true, "y"),
                arg("button", Some("button"), Some('b'), ArgType::String, false, "left|right|middle"),
            ],
        },
        MethodSpec {
            name: "input.move".into(),
            summary: "Move the pointer".into(),
            args: vec![
                arg("x", None, None, ArgType::Int, true, "x"),
                arg("y", None, None, ArgType::Int, true, "y"),
            ],
        },
        MethodSpec {
            name: "input.scroll".into(),
            summary: "Scroll the pointer wheel".into(),
            args: vec![
                arg("dx", Some("dx"), Some('x'), ArgType::Float, false, "horizontal"),
                arg("dy", Some("dy"), Some('y'), ArgType::Float, false, "vertical (down is +)"),
            ],
        },
        MethodSpec {
            name: "input.key".into(),
            summary: "Synthesize a key chord".into(),
            args: vec![arg(
                "chord",
                None,
                None,
                ArgType::String,
                true,
                "e.g. Meta+Tab",
            )],
        },
    ]
}

pub fn session_methods() -> Vec<MethodSpec> {
    vec![
        MethodSpec {
            name: "launch".into(),
            summary: "Spawn an app (session must be running)".into(),
            args: vec![
                arg("app_id", None, None, ArgType::String, true, "app id"),
                arg(
                    "command",
                    Some("command"),
                    Some('c'),
                    ArgType::String,
                    false,
                    "override command; default /opt/sola/bin/<app_id>",
                ),
            ],
        },
        MethodSpec {
            name: "close".into(),
            summary: "Close a session-tracked app".into(),
            args: vec![arg("app_id", None, None, ArgType::String, true, "app id")],
        },
    ]
}

fn arg(
    name: &str,
    long: Option<&str>,
    short: Option<char>,
    ty: ArgType,
    required: bool,
    help: &str,
) -> ArgSpec {
    ArgSpec {
        name: name.into(),
        long: long.map(str::to_string),
        short,
        ty,
        required,
        help: help.into(),
    }
}
