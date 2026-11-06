//! Asking Oath: `oath --json <verb>` as `home` (every verb this app runs
//! reads only), and the catalog's own files under `/oath`.

use std::path::PathBuf;
use std::process::{Command, Stdio};

use serde::Deserialize;
use serde_json::Value;

/// `oath`, or `OATH_BIN` (a stand-in for tests and for trying the app off
/// Oath).
pub fn bin() -> PathBuf {
    std::env::var_os("OATH_BIN").map_or_else(|| PathBuf::from("/bin/oath"), PathBuf::from)
}

/// The catalog root: `/oath`, or `OATH_ROOT` as `oath` itself reads it.
pub fn root() -> PathBuf {
    std::env::var_os("OATH_ROOT").map_or_else(|| PathBuf::from("/oath"), PathBuf::from)
}

/// The release this machine runs (`/oath/output.json`).
pub fn release_file() -> PathBuf {
    root().join("output.json")
}

/// One line per apply; a new line means the packages moved.
pub fn apply_log() -> PathBuf {
    root().join("log/apply.jsonl")
}

/// `oath --json <args>`: the JSON it printed, or what went wrong in words.
/// A verb that refuses prints `{"error": …, "hint": …}`; that comes back
/// as the error.
pub fn json(args: &[&str]) -> Result<Value, String> {
    let out = Command::new(bin())
        .arg("--json")
        .args(args)
        .stdin(Stdio::null())
        .output()
        .map_err(|e| format!("oath {}: {e}", args.join(" ")))?;
    let stdout = String::from_utf8_lossy(&out.stdout);
    let value = extract(&stdout);
    if let Some(v) = &value
        && let Some(err) = v.get("error").and_then(Value::as_str)
    {
        let hint = v.get("hint").and_then(Value::as_str);
        return Err(match hint {
            Some(h) => format!("{err} ({h})"),
            None => err.to_string(),
        });
    }
    match value {
        Some(v) if out.status.success() => Ok(v),
        _ => {
            let stderr = String::from_utf8_lossy(&out.stderr);
            let why = stderr
                .lines()
                .rev()
                .find(|l| !l.trim().is_empty() && !is_telemetry(l))
                .unwrap_or("no JSON");
            Err(format!("oath {} failed: {why}", args.join(" ")))
        }
    }
}

fn is_telemetry(line: &str) -> bool {
    line.trim_start().starts_with("oath-tel")
}

/// The first JSON value in `stdout`, past `oath-tel` lines.
pub fn extract(stdout: &str) -> Option<Value> {
    let text: String = stdout
        .lines()
        .filter(|l| !is_telemetry(l))
        .collect::<Vec<_>>()
        .join("\n");
    let start = text.find(['{', '['])?;
    let mut de = serde_json::Deserializer::from_str(&text[start..]);
    Value::deserialize(&mut de).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn telemetry_lines_are_skipped() {
        let out = "oath-tel {\"verb\":\"get\"}\n{\"id\":\"pkg:mesa\"}\n";
        assert_eq!(extract(out).unwrap()["id"], "pkg:mesa");
    }

    #[test]
    fn an_array_comes_back_whole() {
        assert_eq!(extract("[\"pkg:a\",\"pkg:b\"]").unwrap().as_array().unwrap().len(), 2);
    }

    #[test]
    fn nothing_printed_is_none() {
        assert!(extract("oath-tel x\n").is_none());
    }
}
