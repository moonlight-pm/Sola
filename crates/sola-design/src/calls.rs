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
    let canvas = || {
        opt(
            "canvas",
            Some('c'),
            ArgType::Path,
            "Canvas dir, its canvas.toml, or a file in it (default: the one on the human's screen; over MCP, your last opened / created canvas)",
        )
    };
    vec![
        method("project.list", "Projects and their canvases", &[]),
        method(
            "project.add",
            "Register a project folder (canvases live in its design/)",
            &[req("path", Some('p'), ArgType::Path, "Project folder")],
        ),
        method(
            "project.remove",
            "Forget a project: its canvases leave the rail and close (files stay on disk)",
            &[req(
                "path",
                Some('p'),
                ArgType::Path,
                "Project folder (its `root` in project.list)",
            )],
        ),
        method(
            "open",
            "Make a canvas live for you, off the human's screen (registers its project if needed); --raise shows it to them",
            &[
                req(
                    "path",
                    Some('p'),
                    ArgType::Path,
                    "Canvas dir, its canvas.toml, or any file in it",
                ),
                flag("raise", 'r', "Show it on the human's screen and raise the Design window"),
            ],
        ),
        method(
            "canvas.new",
            "Make a canvas (design/<name>/) in a project, live for you off the human's screen; opens it if it exists",
            &[
                req_s("name", Some('n'), "Canvas name (the folder is its slug)"),
                opt(
                    "path",
                    Some('p'),
                    ArgType::Path,
                    "Project folder (default: your working directory)",
                ),
                flag("raise", 'r', "Show it on the human's screen and raise the Design window"),
            ],
        ),
        method(
            "status",
            "A canvas: artboards (size, scale, loading), watched outside files, zoom, shown, selection",
            &[canvas()],
        ),
        method(
            "share",
            "A canvas's live share link (the web sees its files as they are now, while Design runs)",
            &[canvas()],
        ),
        method(
            "share.rotate",
            "A new share link for a canvas; the old one stops working",
            &[canvas()],
        ),
        method(
            "selection",
            "What the human Super+clicked (null when nothing)",
            &[],
        ),
        method(
            "view",
            "Steer a canvas's camera: --fit, --artboard (fit it), --zoom. Off screen it frames the canvas for when the human opens it; once the human has moved it, it stays put (`moved: false`)",
            &[
                canvas(),
                flag("fit", 'f', "Fit every artboard"),
                artboard(),
                opt("zoom", Some('z'), ArgType::Float, "Zoom (1 = 100%)"),
            ],
        ),
        method_ms(
            "wait",
            "Until the artboards show the files on disk (reloads them if files changed) and have settled",
            &[canvas(), artboard(), timeout()],
            WAIT_TIMEOUT_MS,
        ),
        method_ms(
            "reload",
            "Reload artboards (all, or --artboard) and wait for them",
            &[canvas(), artboard(), timeout()],
            WAIT_TIMEOUT_MS,
        ),
        method_ms(
            "screenshot",
            "PNG of the whole artboard at its raster scale (`scale`: 2×, else 1×, else less for a page over 8192 px). The human's view does not change it",
            &[
                canvas(),
                artboard(),
                opt("out", Some('o'), ArgType::Path, "Write here (default: runtime dir)"),
            ],
            PAGE_TIMEOUT_MS,
        ),
        method_ms(
            "pdf",
            "One-page PDF of an artboard as it looks on screen",
            &[
                canvas(),
                artboard(),
                opt("out", Some('o'), ArgType::Path, "Write here (default: runtime dir)"),
            ],
            PAGE_TIMEOUT_MS,
        ),
        method_ms(
            "inspect",
            "Viewport, document size, element box + styles, overflow issues",
            &[
                canvas(),
                artboard(),
                opt_s("selector", Some('s'), "CSS selector (default: the selection's)"),
                flag(
                    "all",
                    'A',
                    "Every match as {selector, tag, rect, text} — no styles (default selector: body > *)",
                ),
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

fn req_s(name: &str, short: Option<char>, help: &str) -> ArgSpec {
    arg(name, true, ArgType::String, short, help)
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
            "project.remove",
            "open",
            "canvas.new",
            "status",
            "selection",
            "share",
            "share.rotate",
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
        for page in ["status", "share", "share.rotate", "view", "wait", "reload", "screenshot", "pdf", "inspect"] {
            let m = methods.iter().find(|m| m.name == page).unwrap();
            let c = m.args.iter().find(|a| a.name == "canvas").expect(page);
            assert!(!c.required && matches!(c.ty, ArgType::Path), "{page}");
        }
        // No raw eval on this plane (freeze: not an agent API).
        assert!(!names.iter().any(|n| n.contains("eval")));
    }
}
