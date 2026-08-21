//! Live owners that are not compiled into the clap tree.

use sola_call::{ArgType, MethodSpec, catalog};

use crate::call;

pub fn run(args: Vec<String>) -> i32 {
    if args.is_empty() {
        eprintln!("solactl: missing owner");
        return 3;
    }
    let owner = &args[0];
    let owners = match catalog() {
        Ok(c) => c,
        Err(e) => {
            eprintln!("solactl: {e}");
            return 3;
        }
    };
    let Some(entry) = owners.iter().find(|o| o.owner == *owner || o.app_id == *owner)
    else {
        eprintln!("solactl: {owner} is not running");
        return 3;
    };

    if args.len() == 1 || matches!(args.get(1).map(String::as_str), Some("-h" | "--help")) {
        println!("solactl {owner} — {}", entry.app_id);
        if entry.methods.is_empty() {
            println!("  (no methods advertised)");
            return 0;
        }
        for m in &entry.methods {
            println!("  {:<16} {}", m.name, m.summary);
        }
        return 0;
    }

    let method = &args[1];
    let Some(spec) = entry.methods.iter().find(|m| m.name == *method) else {
        eprintln!("solactl: {owner} has no method {method}");
        return 3;
    };
    let rest: Vec<&str> = args[2..].iter().map(String::as_str).collect();
    if rest.iter().any(|t| *t == "-h" || *t == "--help") || rest.is_empty() && method == "--help" {
        print_method_help(owner, spec);
        return 0;
    }
    match params_from_args(spec, &rest) {
        Ok(mut params) => {
            if entry.owner == "workspaces" {
                inject_ws_context(method, &mut params);
            }
            let timeout = invoke_timeout_secs(spec, &params);
            call::run(&entry.owner, method, params, timeout)
        }
        Err(e) => {
            eprintln!("solactl: {e}");
            3
        }
    }
}

fn inject_ws_context(method: &str, params: &mut serde_json::Value) {
    let Some(map) = params.as_object_mut() else {
        return;
    };
    if method == "whoami" {
        if !map.contains_key("pane") {
            if let Ok(p) = std::env::var("SOLA_PANE_ID") {
                if !p.is_empty() {
                    map.insert("pane".into(), serde_json::Value::String(p));
                }
            }
        }
        if !map.contains_key("path") {
            if let Ok(p) = std::env::var("SOLA_WS_PATH") {
                if !p.is_empty() {
                    map.insert("path".into(), serde_json::Value::String(p));
                }
            }
        }
    }
    if method == "workspace.spawn" && !map.contains_key("parent") {
        if let Ok(p) = std::env::var("SOLA_PANE_ID") {
            if !p.is_empty() {
                map.insert("parent".into(), serde_json::Value::String(p));
            }
        }
    }
}

fn invoke_timeout_secs(spec: &MethodSpec, params: &serde_json::Value) -> u64 {
    if let Some(secs) = params.get("timeout").and_then(json_u64) {
        return secs.saturating_add(2).max(8);
    }
    spec.timeout_ms
        .map(|ms| ms.div_ceil(1000).max(1))
        .unwrap_or(8)
}

fn json_u64(v: &serde_json::Value) -> Option<u64> {
    v.as_u64()
        .or_else(|| v.as_i64().and_then(|n| u64::try_from(n).ok()))
}

fn print_method_help(owner: &str, spec: &MethodSpec) {
    println!("solactl {owner} {} — {}", spec.name, spec.summary);
    if spec.args.is_empty() {
        println!("  (no flags)");
        return;
    }
    println!();
    for a in &spec.args {
        let long = a.long.as_deref().unwrap_or(a.name.as_str());
        let mut flag = format!("--{long}");
        if let Some(ch) = a.short {
            flag = format!("-{ch}, {flag}");
        }
        if !matches!(a.ty, ArgType::Bool) {
            flag.push_str(" <value>");
        }
        let req = if a.required { "required" } else { "optional" };
        if a.help.is_empty() {
            println!("  {flag:<28} ({req})");
        } else {
            println!("  {flag:<28} {} ({req})", a.help);
        }
    }
}

fn params_from_args(spec: &MethodSpec, rest: &[&str]) -> Result<serde_json::Value, String> {
    if rest.len() == 1 && rest[0].starts_with('{') {
        return serde_json::from_str(rest[0]).map_err(|e| e.to_string());
    }
    let mut map = serde_json::Map::new();
    let mut i = 0;
    while i < rest.len() {
        let tok = rest[i];
        if let Some(name) = tok.strip_prefix("--") {
            let (val, next) = flag_value(spec, name, rest, i)?;
            map.insert(name.to_string(), val);
            i = next;
            continue;
        }
        if let Some(s) = tok.strip_prefix('-')
            && s.len() == 1
        {
            let ch = s.chars().next().unwrap();
            let name = spec
                .args
                .iter()
                .find(|a| a.short == Some(ch))
                .map(|a| a.name.as_str())
                .ok_or_else(|| format!("unknown flag -{ch}"))?;
            let (val, next) = flag_value(spec, name, rest, i)?;
            map.insert(name.to_string(), val);
            i = next;
            continue;
        }
        // Positional: fill the next required arg that isn't set.
        if let Some(arg) = spec.args.iter().find(|a| a.required && !map.contains_key(&a.name))
        {
            map.insert(arg.name.clone(), coerce(spec, &arg.name, tok));
            i += 1;
            continue;
        }
        return Err(format!("unexpected argument {tok}"));
    }
    Ok(serde_json::Value::Object(map))
}

fn flag_value(
    spec: &MethodSpec,
    name: &str,
    rest: &[&str],
    i: usize,
) -> Result<(serde_json::Value, usize), String> {
    let ty = spec
        .args
        .iter()
        .find(|a| a.name == name || a.long.as_deref() == Some(name))
        .map(|a| &a.ty)
        .unwrap_or(&ArgType::String);
    if matches!(ty, ArgType::Bool) {
        if let Some(next) = rest.get(i + 1).copied() {
            if matches!(next, "true" | "false" | "0" | "1") {
                return Ok((coerce(spec, name, next), i + 2));
            }
        }
        return Ok((serde_json::Value::Bool(true), i + 1));
    }
    let val = rest
        .get(i + 1)
        .copied()
        .ok_or_else(|| format!("--{name} needs a value"))?;
    if val.starts_with('-') && !looks_like_negative_number(val) {
        return Err(format!("--{name} needs a value"));
    }
    Ok((coerce(spec, name, val), i + 2))
}

fn looks_like_negative_number(s: &str) -> bool {
    s[1..].chars().next().is_some_and(|c| c.is_ascii_digit())
}

fn coerce(spec: &MethodSpec, name: &str, raw: &str) -> serde_json::Value {
    let ty = spec
        .args
        .iter()
        .find(|a| a.name == name || a.long.as_deref() == Some(name))
        .map(|a| &a.ty)
        .unwrap_or(&ArgType::String);
    match ty {
        ArgType::Int => raw
            .parse::<i64>()
            .map(serde_json::Value::from)
            .unwrap_or_else(|_| serde_json::Value::String(raw.into())),
        ArgType::Float => raw
            .parse::<f64>()
            .map(serde_json::Value::from)
            .unwrap_or_else(|_| serde_json::Value::String(raw.into())),
        ArgType::Bool => serde_json::Value::Bool(raw != "false" && raw != "0"),
        ArgType::Json => serde_json::from_str(raw)
            .unwrap_or_else(|_| serde_json::Value::String(raw.into())),
        ArgType::String | ArgType::Path => serde_json::Value::String(raw.into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sola_call::{ArgSpec, ArgType, MethodSpec};

    fn spec() -> MethodSpec {
        MethodSpec {
            name: "pane.send".into(),
            summary: String::new(),
            args: vec![
                ArgSpec {
                    name: "pane".into(),
                    long: Some("pane".into()),
                    short: None,
                    ty: ArgType::String,
                    required: false,
                    help: String::new(),
                },
                ArgSpec {
                    name: "text".into(),
                    long: Some("text".into()),
                    short: Some('t'),
                    ty: ArgType::String,
                    required: true,
                    help: String::new(),
                },
                ArgSpec {
                    name: "enter".into(),
                    long: Some("enter".into()),
                    short: Some('e'),
                    ty: ArgType::Bool,
                    required: false,
                    help: String::new(),
                },
                ArgSpec {
                    name: "timeout".into(),
                    long: Some("timeout".into()),
                    short: None,
                    ty: ArgType::Int,
                    required: false,
                    help: String::new(),
                },
            ],
            timeout_ms: Some(8_000),
        }
    }

    #[test]
    fn enter_before_text_does_not_eat_flag() {
        let p = params_from_args(&spec(), &["--enter", "--text", "hi"]).unwrap();
        assert_eq!(p["enter"], true);
        assert_eq!(p["text"], "hi");
    }

    fn spawn_spec() -> MethodSpec {
        MethodSpec {
            name: "workspace.spawn".into(),
            summary: String::new(),
            args: vec![
                ArgSpec {
                    name: "project".into(),
                    long: Some("project".into()),
                    short: Some('p'),
                    ty: ArgType::String,
                    required: true,
                    help: String::new(),
                },
                ArgSpec {
                    name: "name".into(),
                    long: Some("name".into()),
                    short: Some('n'),
                    ty: ArgType::String,
                    required: true,
                    help: String::new(),
                },
                ArgSpec {
                    name: "select".into(),
                    long: Some("select".into()),
                    short: Some('s'),
                    ty: ArgType::Bool,
                    required: false,
                    help: String::new(),
                },
            ],
            timeout_ms: Some(60_000),
        }
    }

    #[test]
    fn select_before_name_does_not_eat_flag() {
        let p = params_from_args(
            &spawn_spec(),
            &["--select", "--name", "bg-test", "--project", "Illuno"],
        )
        .unwrap();
        assert_eq!(p["select"], true);
        assert_eq!(p["name"], "bg-test");
        assert_eq!(p["project"], "Illuno");
    }

    #[test]
    fn invoke_timeout_follows_arg() {
        let spec = spec();
        let p = params_from_args(&spec, &["--text", "x", "--timeout", "120"]).unwrap();
        assert_eq!(invoke_timeout_secs(&spec, &p), 122);
        let empty = serde_json::json!({});
        assert_eq!(invoke_timeout_secs(&spec, &empty), 8);
    }
}
