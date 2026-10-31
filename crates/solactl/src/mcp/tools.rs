//! Call-plane owners → MCP tools, and one tool call → an MCP result.
//!
//! A tool is `<owner>_<method>` (dots become `_`): `design_screenshot`,
//! `browser_tab_open`, `projects_pane_send`. Its input schema comes from
//! the method's `ArgSpec`s. Calls go through the same preparation as
//! `solactl <owner> <method>` (Path args, Projects pane context, bot
//! policy, timeout). A reply whose `path` is a small PNG / JPEG also comes
//! back as an image, so the agent sees a screenshot without a second read.

use std::sync::Mutex;
use std::time::Duration;

use base64::Engine as _;
use serde_json::{Map, Value, json};
use sola_call::{ArgType, CallError, MethodSpec, OwnerCatalog};

use crate::dynamic;

/// Largest image inlined (raw bytes; base64 grows it by a third).
const MAX_IMAGE_BYTES: u64 = 3_700_000;

/// The canvas this agent last opened or made. One MCP server runs per
/// agent, so this is per agent: its page calls go to its own canvas, not
/// to whatever the human has on screen — several agents work at once.
static DESIGN_CANVAS: Mutex<Option<String>> = Mutex::new(None);
/// Design calls that act on one canvas (they take `canvas`).
const DESIGN_PAGE_CALLS: &[&str] = &[
    "status",
    "share",
    "share.rotate",
    "view",
    "wait",
    "reload",
    "screenshot",
    "pdf",
    "inspect",
];

/// One live method as a tool.
#[derive(Debug, Clone)]
pub struct Tool {
    pub name: String,
    pub owner: String,
    pub app_id: String,
    pub spec: MethodSpec,
}

/// Tools for every owner (or just `only`, when non-empty), sorted by name.
pub fn tools(catalog: &[OwnerCatalog], only: &[String]) -> Vec<Tool> {
    let mut out: Vec<Tool> = catalog
        .iter()
        .filter(|o| only.is_empty() || only.iter().any(|w| *w == o.owner || *w == o.app_id))
        .flat_map(|o| {
            o.methods.iter().map(move |m| Tool {
                name: tool_name(&o.owner, &m.name),
                owner: o.owner.clone(),
                app_id: o.app_id.clone(),
                spec: m.clone(),
            })
        })
        .collect();
    out.sort_by(|a, b| a.name.cmp(&b.name));
    out.dedup_by(|a, b| a.name == b.name);
    out
}

/// `[a-zA-Z0-9_-]{1,64}` as MCP clients require.
pub fn tool_name(owner: &str, method: &str) -> String {
    let raw = format!("{owner}_{method}");
    let mut name: String = raw
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() || c == '-' { c } else { '_' })
        .collect();
    name.truncate(64);
    name
}

/// `tools/list` entry.
pub fn describe(t: &Tool) -> Value {
    let mut properties = Map::new();
    let mut required = Vec::new();
    for a in &t.spec.args {
        let ty = match a.ty {
            ArgType::String | ArgType::Path => "string",
            ArgType::Int => "integer",
            ArgType::Float => "number",
            ArgType::Bool => "boolean",
            ArgType::Json => "object",
        };
        let mut help = a.help.clone();
        if matches!(a.ty, ArgType::Path) {
            help.push_str(if help.is_empty() { "Path" } else { " (path)" });
        }
        properties.insert(a.name.clone(), json!({"type": ty, "description": help}));
        if a.required {
            required.push(a.name.clone());
        }
    }
    let summary = if t.spec.summary.is_empty() {
        t.spec.name.clone()
    } else {
        t.spec.summary.clone()
    };
    json!({
        "name": t.name,
        "description": format!("{summary} — {} (`solactl {} {}`)", t.app_id, t.owner, t.spec.name),
        "inputSchema": {
            "type": "object",
            "properties": properties,
            "required": required,
        },
    })
}

/// Prepare and invoke one tool; always an MCP `CallToolResult`.
pub fn call(t: &Tool, arguments: Option<&Value>) -> Value {
    let mut params = match arguments {
        Some(Value::Object(m)) => Value::Object(m.clone()),
        None | Some(Value::Null) => json!({}),
        Some(other) => return error_result(&format!("arguments must be an object, got {other}")),
    };
    resolve_paths(&t.spec, &mut params);
    if t.owner == "projects" {
        dynamic::inject_ws_context(&t.spec.name, &mut params);
    }
    if t.owner == "design" {
        dynamic::inject_design_context(&t.spec.name, &mut params);
        let mine = DESIGN_CANVAS.lock().ok().and_then(|c| c.clone());
        default_canvas(&t.spec.name, &mut params, mine);
    }
    if let Err(e) = dynamic::apply_bot_policy(&t.owner, &t.spec.name, &params) {
        return error_result(&e);
    }
    let timeout = Duration::from_secs(dynamic::invoke_timeout_secs(&t.spec, &params));
    match sola_call::invoke(&t.owner, &t.spec.name, params, timeout) {
        Ok(data) => {
            if t.owner == "design" && matches!(t.spec.name.as_str(), "open" | "canvas.new") {
                remember_canvas(&data);
            }
            ok_result(&data)
        }
        Err(CallError::Timeout) => error_result(&format!(
            "timeout waiting for {}.{} — if {} is still on screen its call channel is wedged; \
             close it from its window and launch it again",
            t.owner, t.spec.name, t.app_id
        )),
        Err(e) => error_result(&e.to_string()),
    }
}

/// A page call without `canvas` goes to this agent's canvas (`mine`).
fn default_canvas(method: &str, params: &mut Value, mine: Option<String>) {
    let (Some(map), Some(path)) = (params.as_object_mut(), mine) else {
        return;
    };
    if DESIGN_PAGE_CALLS.contains(&method) && !map.contains_key("canvas") {
        map.insert("canvas".into(), Value::String(path));
    }
}

/// Keep the canvas an `open` / `canvas.new` reply names for this agent.
fn remember_canvas(data: &Value) {
    let path = data.pointer("/canvas/path").and_then(Value::as_str);
    if let (Some(path), Ok(mut mine)) = (path, DESIGN_CANVAS.lock()) {
        *mine = Some(path.to_string());
    }
}

/// Relative / `~` Path args resolve against this server's cwd (the agent's).
fn resolve_paths(spec: &MethodSpec, params: &mut Value) {
    let Some(map) = params.as_object_mut() else {
        return;
    };
    for a in spec.args.iter().filter(|a| matches!(a.ty, ArgType::Path)) {
        if let Some(Value::String(raw)) = map.get_mut(&a.name) {
            *raw = dynamic::absolute_path(raw);
        }
    }
}

fn ok_result(data: &Value) -> Value {
    let text = if data.is_null() {
        "ok".to_string()
    } else {
        serde_json::to_string_pretty(data).unwrap_or_else(|_| data.to_string())
    };
    let mut content = vec![json!({"type": "text", "text": text})];
    content.extend(image_for(data));
    json!({"content": content, "isError": false})
}

pub fn error_result(message: &str) -> Value {
    json!({"content": [{"type": "text", "text": message}], "isError": true})
}

/// A reply `path` naming a small PNG / JPEG on disk, as image content.
fn image_for(data: &Value) -> Option<Value> {
    let path = data.get("path")?.as_str()?;
    let lower = path.to_ascii_lowercase();
    let mime = if lower.ends_with(".png") {
        "image/png"
    } else if lower.ends_with(".jpg") || lower.ends_with(".jpeg") {
        "image/jpeg"
    } else {
        return None;
    };
    let size = std::fs::metadata(path).ok()?.len();
    if size > MAX_IMAGE_BYTES {
        return Some(json!({
            "type": "text",
            "text": format!("({path} is {size} bytes — too large to inline; read the file)"),
        }));
    }
    let bytes = std::fs::read(path).ok()?;
    Some(json!({
        "type": "image",
        "data": base64::engine::general_purpose::STANDARD.encode(bytes),
        "mimeType": mime,
    }))
}

#[cfg(test)]
mod tests {

    #[test]
    fn page_calls_default_to_the_agents_canvas() {
        let mine = Some("/w/p/design/home".to_string());
        let mut p = json!({"artboard": "a"});
        default_canvas("screenshot", &mut p, mine.clone());
        assert_eq!(p["canvas"], "/w/p/design/home");
        let mut p = json!({"canvas": "/other"});
        default_canvas("inspect", &mut p, mine.clone());
        assert_eq!(p["canvas"], "/other", "explicit wins");
        let mut p = json!({});
        default_canvas("selection", &mut p, mine.clone());
        assert!(p.get("canvas").is_none(), "not a page call");
        let mut p = json!({});
        default_canvas("status", &mut p, None);
        assert!(p.get("canvas").is_none(), "no canvas yet: the human's");
    }

    use super::*;
    use sola_call::ArgSpec;

    fn spec(name: &str, args: Vec<ArgSpec>) -> MethodSpec {
        MethodSpec {
            name: name.into(),
            summary: "Do it".into(),
            args,
            timeout_ms: None,
        }
    }

    fn arg(name: &str, ty: ArgType, required: bool) -> ArgSpec {
        ArgSpec {
            name: name.into(),
            long: Some(name.into()),
            short: None,
            ty,
            required,
            help: format!("{name} help"),
        }
    }

    #[test]
    fn names_are_mcp_safe() {
        assert_eq!(tool_name("browser", "tab.open"), "browser_tab_open");
        assert_eq!(tool_name("design", "project.list"), "design_project_list");
        assert_eq!(tool_name("x y", "a/b"), "x_y_a_b");
        assert_eq!(tool_name(&"o".repeat(40), &"m".repeat(40)).len(), 64);
    }

    #[test]
    fn schema_maps_types_and_required() {
        let t = Tool {
            name: "design_open".into(),
            owner: "design".into(),
            app_id: "sola-design".into(),
            spec: spec(
                "open",
                vec![
                    arg("path", ArgType::Path, true),
                    arg("raise", ArgType::Bool, false),
                    arg("zoom", ArgType::Float, false),
                ],
            ),
        };
        let d = describe(&t);
        assert_eq!(d["inputSchema"]["required"], json!(["path"]));
        assert_eq!(d["inputSchema"]["properties"]["raise"]["type"], "boolean");
        assert_eq!(d["inputSchema"]["properties"]["zoom"]["type"], "number");
        assert!(d["description"].as_str().unwrap().contains("solactl design open"));
    }

    #[test]
    fn filter_by_owner_or_app_id() {
        let cat = vec![
            OwnerCatalog {
                owner: "design".into(),
                app_id: "sola-design".into(),
                methods: vec![spec("status", vec![])],
            },
            OwnerCatalog {
                owner: "browser".into(),
                app_id: "sola-browser".into(),
                methods: vec![spec("tabs", vec![])],
            },
        ];
        assert_eq!(tools(&cat, &[]).len(), 2);
        let only = tools(&cat, &["sola-design".into()]);
        assert_eq!(only.len(), 1);
        assert_eq!(only[0].name, "design_status");
    }

    #[test]
    fn relative_paths_resolve_and_images_inline() {
        let s = spec("open", vec![arg("path", ArgType::Path, true)]);
        let mut p = json!({"path": "design/home"});
        resolve_paths(&s, &mut p);
        assert!(p["path"].as_str().unwrap().starts_with('/'));

        let dir = std::env::temp_dir().join(format!("solactl-mcp-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let png = dir.join("shot.png");
        std::fs::write(&png, b"\x89PNG fake").unwrap();
        let r = ok_result(&json!({"path": png}));
        assert_eq!(r["content"][1]["type"], "image");
        assert_eq!(r["content"][1]["mimeType"], "image/png");
        let r = ok_result(&json!({"path": dir.join("a.pdf")}));
        assert_eq!(r["content"].as_array().unwrap().len(), 1);
        let _ = std::fs::remove_dir_all(dir);
    }
}
