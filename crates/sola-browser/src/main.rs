//! sola-browser — selects an engine and execs sola-browser-{wpe,cef}.
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::OsString;

    fn s(v: &[&str]) -> Vec<OsString> { v.iter().map(OsString::from).collect() }

    #[test]
    fn default_engine_is_wpe() {
        assert_eq!(pick_engine(&s(&[]), None), "wpe");
    }
    #[test]
    fn flag_selects_cef() {
        assert_eq!(pick_engine(&s(&["--engine", "cef"]), None), "cef");
    }
    #[test]
    fn flag_eq_form_selects_cef() {
        assert_eq!(pick_engine(&s(&["--engine=cef"]), None), "cef");
    }
    #[test]
    fn env_selects_when_no_flag() {
        assert_eq!(pick_engine(&s(&[]), Some("cef".into())), "cef");
    }
    #[test]
    fn flag_overrides_env() {
        assert_eq!(pick_engine(&s(&["--engine", "wpe"]), Some("cef".into())), "wpe");
    }
    #[test]
    fn unknown_engine_falls_back_to_wpe() {
        assert_eq!(pick_engine(&s(&["--engine", "lynx"]), None), "wpe");
    }
    #[test]
    fn passthrough_strips_engine_flag_keeps_url() {
        assert_eq!(passthrough(&s(&["--engine", "cef", "https://x.test"])), s(&["https://x.test"]));
    }
    #[test]
    fn passthrough_strips_eq_form() {
        assert_eq!(passthrough(&s(&["--engine=cef", "--app", "https://x.test"])), s(&["--app", "https://x.test"]));
    }

    #[test]
    fn resolve_target_finds_and_falls_back() {
        use std::fs;
        use std::process;

        let test_id = process::id();
        let base_temp = std::env::temp_dir().join(format!("sola-browser-test-{}", test_id));
        let _ = fs::remove_dir_all(&base_temp);

        // Case A: resolve_target finds the requested engine
        let case_a_dir = base_temp.join("case_a");
        fs::create_dir_all(&case_a_dir).expect("create case_a");
        fs::write(case_a_dir.join("sola-browser-wpe"), "").expect("write wpe");

        let result = resolve_target(&case_a_dir, "wpe");
        assert_eq!(result, Some(case_a_dir.join("sola-browser-wpe")));

        // Case B: fallback to the other engine
        let case_b_dir = base_temp.join("case_b");
        fs::create_dir_all(&case_b_dir).expect("create case_b");
        fs::write(case_b_dir.join("sola-browser-wpe"), "").expect("write wpe");

        let result = resolve_target(&case_b_dir, "cef");
        assert_eq!(result, Some(case_b_dir.join("sola-browser-wpe")));

        // Case C: neither engine exists
        let case_c_dir = base_temp.join("case_c");
        fs::create_dir_all(&case_c_dir).expect("create case_c");

        let result = resolve_target(&case_c_dir, "wpe");
        assert_eq!(result, None);

        // Clean up
        let _ = fs::remove_dir_all(&base_temp);
    }
}

const ENGINES: [&str; 2] = ["wpe", "cef"];
const DEFAULT_ENGINE: &str = "wpe";

/// Resolve the engine name from `--engine <x>` / `--engine=x`, then
/// `$SOLA_BROWSER_ENGINE`, else the default. Unknown names fall back to default.
fn pick_engine(args: &[OsString], env: Option<String>) -> &'static str {
    let mut chosen: Option<String> = None;
    let mut it = args.iter();
    while let Some(a) = it.next() {
        let a = a.to_string_lossy();
        if let Some(v) = a.strip_prefix("--engine=") {
            chosen = Some(v.to_string());
        } else if a == "--engine" {
            if let Some(v) = it.next() {
                chosen = Some(v.to_string_lossy().to_string());
            }
        }
    }
    let want = chosen.or(env).unwrap_or_else(|| DEFAULT_ENGINE.to_string());
    ENGINES.into_iter().find(|e| *e == want).unwrap_or(DEFAULT_ENGINE)
}

/// Args to forward to the engine binary: everything except `--engine`/value.
fn passthrough(args: &[OsString]) -> Vec<OsString> {
    let mut out = Vec::new();
    let mut it = args.iter();
    while let Some(a) = it.next() {
        let s = a.to_string_lossy();
        if s == "--engine" {
            let _ = it.next(); // drop its value
        } else if s.starts_with("--engine=") {
            // drop
        } else {
            out.push(a.clone());
        }
    }
    out
}

/// Path to `sola-browser-<engine>` next to this dispatcher; falls back to
/// the other engine if the requested one is missing.
///
/// Fallback assumes exactly two engines (wpe/cef): if the requested engine's binary
/// is absent, tries the other; returns None if neither exists.
fn resolve_target(dir: &Path, engine: &str) -> Option<PathBuf> {
    let primary = dir.join(format!("sola-browser-{engine}"));
    if primary.exists() {
        return Some(primary);
    }
    let other = if engine == "wpe" { "cef" } else { "wpe" };
    let fallback = dir.join(format!("sola-browser-{other}"));
    if fallback.exists() {
        eprintln!(
            "sola-browser: engine '{engine}' not found; falling back to '{other}' ({})",
            fallback.display()
        );
        Some(fallback)
    } else {
        None
    }
}

fn main() -> ExitCode {
    use std::os::unix::process::CommandExt;
    let args: Vec<OsString> = std::env::args_os().skip(1).collect();
    let engine = pick_engine(&args, std::env::var("SOLA_BROWSER_ENGINE").ok());
    let dir = match std::env::current_exe().ok().and_then(|p| p.parent().map(Path::to_path_buf)) {
        Some(d) => d,
        None => {
            eprintln!("sola-browser: cannot resolve own directory");
            return ExitCode::FAILURE;
        }
    };
    let Some(target) = resolve_target(&dir, engine) else {
        eprintln!("sola-browser: no engine binary found in {}", dir.display());
        return ExitCode::FAILURE;
    };
    let err = std::process::Command::new(&target).args(passthrough(&args)).exec();
    eprintln!("sola-browser: exec {} failed: {err}", target.display());
    ExitCode::FAILURE
}
