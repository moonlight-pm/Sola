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

    if args.len() == 1 {
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
    match params_from_args(spec, &rest) {
        Ok(params) => call::run(&entry.owner, method, params, 8),
        Err(e) => {
            eprintln!("solactl: {e}");
            3
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
            let val = rest.get(i + 1).copied().unwrap_or("true");
            map.insert(name.to_string(), coerce(spec, name, val));
            i += 2;
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
            let val = rest.get(i + 1).copied().unwrap_or("true");
            map.insert(name.to_string(), coerce(spec, name, val));
            i += 2;
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
