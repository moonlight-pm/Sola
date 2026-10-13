//! Advertised sola-call methods for owner `design` (`solactl design …`).
//!
//! First-class surface: keep this list, the handlers in `app/cli.rs`,
//! tests, and `docs/manual/solactl.md` in the same change.
//! Freeze: `docs/specs/2026-10-12-sola-design-design.md`.

use sola_call::{ArgSpec, ArgType, MethodSpec};

pub const OWNER: &str = "design";

/// CDP round trips on one board.
pub const PAGE_TIMEOUT_MS: u64 = 15_000;
/// Upper bound for `wait` / `reload` (`--timeout` raises it per call).
pub const WAIT_TIMEOUT_MS: u64 = 32_000;
pub const WAIT_DEFAULT_SECS: u64 = 15;

pub fn methods() -> Vec<MethodSpec> {
    let artboard = || opt_s("artboard", Some('a'), "Artboard id (default: selection's, or the only one)");
    let timeout = || opt("timeout", None, ArgType::Int, "Seconds (default 15)");
    vec![
        method("project.list", "Projects and their canvases", &[]),
        method(
            "project.add",
            "Register a project folder (canvases live in its design/)",
            &[req("path", Some('p'), ArgType::Path, "Project folder")],
        ),
        method(
            "open",
            "Show a canvas (registers its project if needed)",
            &[
                req(
                    "path",
                    Some('p'),
                    ArgType::Path,
                    "Canvas dir, its canvas.toml, or any file in it",
                ),
                flag("raise", 'r', "Raise the Design window"),
            ],
        ),
        method(
            "status",
            "Open canvas, artboards (size, loading), zoom, selection",
            &[],
        ),
        method(
            "selection",
            "What the human Super+clicked (null when nothing)",
            &[],
        ),
        method(
            "view",
            "Steer the camera: --fit, --artboard (fit it), --zoom",
            &[
                flag("fit", 'f', "Fit every artboard"),
                artboard(),
                opt("zoom", Some('z'), ArgType::Float, "Zoom (1 = 100%)"),
            ],
        ),
        method_ms(
            "wait",
            "Until the artboards have loaded and painted",
            &[artboard(), timeout()],
            WAIT_TIMEOUT_MS,
        ),
        method_ms(
            "reload",
            "Reload artboards (all, or --artboard) and wait for them",
            &[artboard(), timeout()],
            WAIT_TIMEOUT_MS,
        ),
        method_ms(
            "screenshot",
            "PNG of an artboard as rendered (2× raster)",
            &[
                artboard(),
                opt("out", Some('o'), ArgType::Path, "Write here (default: runtime dir)"),
            ],
            PAGE_TIMEOUT_MS,
        ),
        method_ms(
            "pdf",
            "One-page PDF of an artboard as it looks on screen",
            &[
                artboard(),
                opt("out", Some('o'), ArgType::Path, "Write here (default: runtime dir)"),
            ],
            PAGE_TIMEOUT_MS,
        ),
        method_ms(
            "inspect",
            "Viewport, document size, element box + styles, overflow issues",
            &[
                artboard(),
                opt_s("selector", Some('s'), "CSS selector (default: the selection's)"),
            ],
            PAGE_TIMEOUT_MS,
        ),
    ]
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
        timeout_ms: Some(timeout_ms),
        ..method(name, summary, args)
    }
}

fn req(name: &str, short: Option<char>, ty: ArgType, help: &str) -> ArgSpec {
    arg(name, true, ty, short, help)
}

fn opt(name: &str, short: Option<char>, ty: ArgType, help: &str) -> ArgSpec {
    arg(name, false, ty, short, help)
}

fn opt_s(name: &str, short: Option<char>, help: &str) -> ArgSpec {
    arg(name, false, ArgType::String, short, help)
}

fn flag(name: &str, short: char, help: &str) -> ArgSpec {
    arg(name, false, ArgType::Bool, Some(short), help)
}

fn arg(name: &str, required: bool, ty: ArgType, short: Option<char>, help: &str) -> ArgSpec {
    ArgSpec {
        name: name.into(),
        long: Some(name.into()),
        short,
        ty,
        required,
        help: help.into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn surface_is_complete_and_paths_are_paths() {
        let methods = methods();
        let names: Vec<&str> = methods.iter().map(|m| m.name.as_str()).collect();
        for need in [
            "project.list",
            "project.add",
            "open",
            "status",
            "selection",
            "view",
            "wait",
            "reload",
            "screenshot",
            "pdf",
            "inspect",
        ] {
            assert!(names.contains(&need), "missing {need}");
        }
        let open = methods.iter().find(|m| m.name == "open").unwrap();
        let path = open.args.iter().find(|a| a.name == "path").unwrap();
        assert!(path.required && matches!(path.ty, ArgType::Path));
        let shot = methods.iter().find(|m| m.name == "screenshot").unwrap();
        assert!(matches!(
            shot.args.iter().find(|a| a.name == "out").unwrap().ty,
            ArgType::Path
        ));
        let wait = methods.iter().find(|m| m.name == "wait").unwrap();
        assert_eq!(wait.timeout_ms, Some(WAIT_TIMEOUT_MS));
        // No raw eval on this plane (freeze: not an agent API).
        assert!(!names.iter().any(|n| n.contains("eval")));
    }
}
