//! Prune Chromium `Accessibility.getFullAXTree` into a Playwright-shaped
//! YAML snapshot with opaque refs.
//!
//! Pipeline (freeze): rebuild tree → Puppeteer `interestingOnly` → collapse
//! nameless `generic` wrappers → assign refs (`backendDOMNodeId`).

use serde::{Deserialize, Serialize};

pub const MAX_DEPTH: usize = 24;
pub const MAX_YAML_CHARS: usize = 200_000;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RefEntry {
    pub r#ref: String,
    pub backend_node_id: i32,
    pub role: String,
    pub name: String,
    pub frame: u32,
}

#[derive(Debug, Clone)]
pub struct SnapshotOpts {
    pub interactive: bool,
    pub subtree_backend: Option<i32>,
    pub url: String,
    pub title: String,
    pub frame: u32,
}

impl Default for SnapshotOpts {
    fn default() -> Self {
        Self {
            interactive: false,
            subtree_backend: None,
            url: String::new(),
            title: String::new(),
            frame: 0,
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct Snapshot {
    pub url: String,
    pub title: String,
    pub focused: Option<String>,
    pub dialog_open: bool,
    pub yaml: String,
    pub refs: Vec<RefEntry>,
}

impl Snapshot {
    pub fn render(&self) -> String {
        let mut out = String::new();
        if !self.url.is_empty() {
            out.push_str("url: ");
            out.push_str(&self.url);
            out.push('\n');
        }
        if !self.title.is_empty() {
            out.push_str("title: ");
            out.push_str(&self.title);
            out.push('\n');
        }
        if let Some(f) = &self.focused {
            out.push_str("focused: ");
            out.push_str(f);
            out.push('\n');
        }
        if self.dialog_open {
            out.push_str("dialog: open\n");
        }
        out.push_str(&self.yaml);
        out
    }

    pub fn as_debug_json(&self) -> serde_json::Value {
        serde_json::json!({
            "url": self.url,
            "title": self.title,
            "focused": self.focused,
            "dialog": self.dialog_open,
            "yaml": self.yaml,
            "refs": self.refs.iter().map(|r| r.r#ref.clone()).collect::<Vec<_>>(),
        })
    }
}

/// One CDP `Accessibility.AXNode` (subset).
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CdpAxNode {
    pub node_id: Option<String>,
    #[serde(default)]
    pub ignored: bool,
    pub role: Option<CdpAxValue>,
    pub name: Option<CdpAxValue>,
    pub description: Option<CdpAxValue>,
    pub value: Option<CdpAxValue>,
    #[serde(default)]
    pub child_ids: Vec<String>,
    #[serde(default, rename = "backendDOMNodeId")]
    pub backend_dom_node_id: Option<i32>,
    #[serde(default)]
    pub properties: Vec<CdpAxProp>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct CdpAxValue {
    #[serde(default)]
    pub value: serde_json::Value,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct CdpAxProp {
    pub name: String,
    pub value: Option<CdpAxValue>,
}

#[derive(Debug, Clone)]
struct Node {
    role: String,
    name: String,
    value: String,
    backend: i32,
    ignored: bool,
    hidden: bool,
    focusable: bool,
    focused: bool,
    richly_editable: bool,
    editable: bool,
    busy: bool,
    live: String,
    modal: bool,
    disabled: bool,
    checked: Option<String>,
    pressed: Option<String>,
    expanded: Option<bool>,
    selected: Option<bool>,
    required: bool,
    readonly: bool,
    level: Option<i64>,
    children: Vec<Node>,
}

pub fn snapshot_from_cdp(nodes: &[CdpAxNode], opts: SnapshotOpts) -> Result<Snapshot, String> {
    let Some(root) = rebuild(nodes) else {
        return Err("empty accessibility tree".into());
    };
    let root = if let Some(backend) = opts.subtree_backend {
        find_backend(&root, backend).ok_or_else(|| {
            "subtree ref not in this tree; snapshot again".to_string()
        })?
    } else {
        root
    };
    let mut interesting = Vec::new();
    collect_interesting(&root, false, &mut interesting);
    let mut tree = serialize_interesting(&root, &interesting);
    collapse_wrappers(&mut tree);
    if opts.interactive {
        filter_interactive(&mut tree);
    }
    cap_depth(&mut tree, 0);
    let mut refs = Vec::new();
    let mut focused = None;
    let mut dialog_open = false;
    let mut next = 1u32;
    assign_refs(
        &mut tree,
        opts.frame,
        &mut next,
        &mut refs,
        &mut focused,
        &mut dialog_open,
    );
    let yaml = render_yaml(&tree, 0);
    if yaml.len() > MAX_YAML_CHARS {
        return Err(format!(
            "snapshot too large ({} chars); use --ref or --interactive",
            yaml.len()
        ));
    }
    Ok(Snapshot {
        url: opts.url,
        title: opts.title,
        focused,
        dialog_open,
        yaml,
        refs,
    })
}

pub fn parse_cdp_nodes(json: &str) -> Result<Vec<CdpAxNode>, String> {
    let v: serde_json::Value =
        serde_json::from_str(json).map_err(|e| format!("ax json: {e}"))?;
    let nodes = v
        .get("nodes")
        .cloned()
        .or_else(|| v.as_array().cloned().map(serde_json::Value::Array))
        .ok_or_else(|| "ax result missing nodes".to_string())?;
    serde_json::from_value(nodes).map_err(|e| format!("ax nodes: {e}"))
}

pub fn find_in_yaml(yaml: &str, query: &str) -> Vec<String> {
    let q = query.to_ascii_lowercase();
    yaml.lines()
        .filter(|l| l.to_ascii_lowercase().contains(&q))
        .map(|l| l.to_string())
        .collect()
}

fn rebuild(nodes: &[CdpAxNode]) -> Option<Node> {
    if nodes.is_empty() {
        return None;
    }
    let mut by_id: std::collections::HashMap<String, Node> = std::collections::HashMap::new();
    let mut child_map: std::collections::HashMap<String, Vec<String>> =
        std::collections::HashMap::new();
    for n in nodes {
        let id = n.node_id.clone().unwrap_or_default();
        child_map.insert(id.clone(), n.child_ids.clone());
        by_id.insert(id, node_from_cdp(n));
    }
    fn take(
        id: &str,
        by_id: &mut std::collections::HashMap<String, Node>,
        child_map: &std::collections::HashMap<String, Vec<String>>,
    ) -> Option<Node> {
        let mut n = by_id.remove(id)?;
        let kids = child_map.get(id).cloned().unwrap_or_default();
        n.children = kids
            .into_iter()
            .filter_map(|cid| take(&cid, by_id, child_map))
            .collect();
        Some(n)
    }
    let root_id = nodes[0].node_id.clone().unwrap_or_default();
    take(&root_id, &mut by_id, &child_map)
}

fn node_from_cdp(n: &CdpAxNode) -> Node {
    let mut node = Node {
        role: ax_string(&n.role),
        name: ax_string(&n.name),
        value: ax_string(&n.value),
        backend: n.backend_dom_node_id.unwrap_or(0),
        ignored: n.ignored,
        hidden: false,
        focusable: false,
        focused: false,
        richly_editable: false,
        editable: false,
        busy: false,
        live: String::new(),
        modal: false,
        disabled: false,
        checked: None,
        pressed: None,
        expanded: None,
        selected: None,
        required: false,
        readonly: false,
        level: None,
        children: Vec::new(),
    };
    for p in &n.properties {
        let key = p.name.to_ascii_lowercase();
        let val = p.value.as_ref().map(|v| &v.value);
        match key.as_str() {
            "hidden" => node.hidden = as_bool(val),
            "focusable" => node.focusable = as_bool(val),
            "focused" => node.focused = as_bool(val),
            "editable" => {
                node.editable = true;
                if as_str(val) == "richtext" {
                    node.richly_editable = true;
                }
            }
            "busy" => node.busy = as_bool(val),
            "live" => node.live = as_str(val),
            "modal" => node.modal = as_bool(val),
            "disabled" => node.disabled = as_bool(val),
            "checked" => node.checked = Some(tri(val)),
            "pressed" => node.pressed = Some(tri(val)),
            "expanded" => node.expanded = Some(as_bool(val)),
            "selected" => node.selected = Some(as_bool(val)),
            "required" => node.required = as_bool(val),
            "readonly" => node.readonly = as_bool(val),
            "level" => node.level = as_i64(val),
            _ => {}
        }
    }
    node.role = normalize_role(&node.role);
    node
}

fn ax_string(v: &Option<CdpAxValue>) -> String {
    v.as_ref().map(|v| as_str(Some(&v.value))).unwrap_or_default()
}

fn as_str(v: Option<&serde_json::Value>) -> String {
    match v {
        Some(serde_json::Value::String(s)) => s.clone(),
        Some(serde_json::Value::Bool(b)) => b.to_string(),
        Some(serde_json::Value::Number(n)) => n.to_string(),
        _ => String::new(),
    }
}

fn as_bool(v: Option<&serde_json::Value>) -> bool {
    match v {
        Some(serde_json::Value::Bool(b)) => *b,
        Some(serde_json::Value::String(s)) => s == "true",
        _ => false,
    }
}

fn as_i64(v: Option<&serde_json::Value>) -> Option<i64> {
    match v {
        Some(serde_json::Value::Number(n)) => n.as_i64(),
        Some(serde_json::Value::String(s)) => s.parse().ok(),
        _ => None,
    }
}

fn tri(v: Option<&serde_json::Value>) -> String {
    match v {
        Some(serde_json::Value::String(s)) if s == "mixed" => "mixed".into(),
        Some(serde_json::Value::Bool(true)) => "true".into(),
        Some(serde_json::Value::String(s)) if s == "true" => "true".into(),
        _ => "false".into(),
    }
}

fn normalize_role(role: &str) -> String {
    match role {
        "RootWebArea" | "WebArea" => "document".into(),
        "StaticText" => "text".into(),
        "InlineTextBox" => "InlineTextBox".into(),
        "Ignored" => "Ignored".into(),
        other => {
            let mut s = other.to_string();
            if s.chars().any(|c| c.is_ascii_uppercase()) && !s.contains('-') {
                s = s.to_ascii_lowercase();
            }
            s
        }
    }
}

fn is_control(role: &str) -> bool {
    matches!(
        role,
        "button"
            | "checkbox"
            | "colorwell"
            | "combobox"
            | "disclosuretriangle"
            | "listbox"
            | "menu"
            | "menubar"
            | "menuitem"
            | "menuitemcheckbox"
            | "menuitemradio"
            | "radio"
            | "scrollbar"
            | "searchbox"
            | "slider"
            | "spinbutton"
            | "switch"
            | "tab"
            | "textbox"
            | "tree"
            | "treeitem"
            | "link"
    )
}

fn is_landmark(role: &str) -> bool {
    matches!(
        role,
        "banner"
            | "complementary"
            | "contentinfo"
            | "form"
            | "main"
            | "navigation"
            | "region"
            | "search"
            | "document"
    )
}

fn is_leaf(n: &Node) -> bool {
    if n.children.is_empty() {
        return true;
    }
    if n.richly_editable {
        return false;
    }
    if n.editable || n.role == "textbox" || n.role == "searchbox" || n.role == "text" {
        return true;
    }
    matches!(
        n.role.as_str(),
        "img" | "image" | "meter" | "scrollbar" | "slider" | "separator" | "progressbar"
    )
}

fn is_interesting(n: &Node, inside_control: bool) -> bool {
    if n.role == "Ignored" || n.hidden || n.ignored {
        return false;
    }
    if n.role == "InlineTextBox" {
        return false;
    }
    if is_landmark(&n.role) {
        return true;
    }
    if n.focusable
        || n.richly_editable
        || n.busy
        || (!n.live.is_empty() && n.live != "off")
        || n.modal
    {
        return true;
    }
    if is_control(&n.role) {
        return true;
    }
    if inside_control {
        return false;
    }
    is_leaf(n) && (!n.name.is_empty() || !n.value.is_empty())
}

fn collect_interesting(n: &Node, inside_control: bool, out: &mut Vec<i32>) {
    if is_interesting(n, inside_control) {
        out.push(n.backend);
    }
    if is_leaf(n) && !n.children.is_empty() && (n.role == "textbox" || n.role == "text") {
        return;
    }
    let inside = inside_control || is_control(&n.role);
    for c in &n.children {
        collect_interesting(c, inside, out);
    }
}

fn serialize_interesting(n: &Node, keep: &[i32]) -> Node {
    let mut children = Vec::new();
    for c in &n.children {
        let sub = serialize_interesting(c, keep);
        if keep.contains(&c.backend) {
            children.push(sub);
        } else {
            children.extend(sub.children);
        }
    }
    let mut out = n.clone();
    out.children = children;
    out
}

fn is_wrapper(n: &Node) -> bool {
    matches!(n.role.as_str(), "generic" | "none" | "group" | "InlineTextBox")
        && n.name.is_empty()
        && !n.focusable
        && !is_control(&n.role)
        && !n.modal
}

fn collapse_wrappers(n: &mut Node) {
    let mut i = 0;
    while i < n.children.len() {
        collapse_wrappers(&mut n.children[i]);
        if is_wrapper(&n.children[i]) {
            let kids = std::mem::take(&mut n.children[i].children);
            n.children.remove(i);
            for (k, kid) in kids.into_iter().enumerate() {
                n.children.insert(i + k, kid);
            }
        } else {
            i += 1;
        }
    }
}

fn filter_interactive(n: &mut Node) {
    n.children.retain(|c| {
        is_control(&c.role) || c.focusable || is_landmark(&c.role) || has_interactive(c)
    });
    for c in &mut n.children {
        filter_interactive(c);
    }
}

fn has_interactive(n: &Node) -> bool {
    is_control(&n.role)
        || n.focusable
        || n.children.iter().any(has_interactive)
}

fn cap_depth(n: &mut Node, depth: usize) {
    if depth >= MAX_DEPTH {
        n.children.clear();
        return;
    }
    for c in &mut n.children {
        cap_depth(c, depth + 1);
    }
}

fn find_backend(n: &Node, backend: i32) -> Option<Node> {
    if n.backend == backend {
        return Some(n.clone());
    }
    for c in &n.children {
        if let Some(found) = find_backend(c, backend) {
            return Some(found);
        }
    }
    None
}

fn assign_refs(
    n: &mut Node,
    frame: u32,
    next: &mut u32,
    refs: &mut Vec<RefEntry>,
    focused: &mut Option<String>,
    dialog: &mut bool,
) {
    if n.role == "dialog" || n.modal {
        *dialog = true;
    }
    let r = if frame == 0 {
        format!("e{next}")
    } else {
        format!("f{frame}e{next}")
    };
    *next += 1;
    if n.focused && n.role != "document" {
        *focused = Some(r.clone());
    }
    refs.push(RefEntry {
        r#ref: r.clone(),
        backend_node_id: n.backend,
        role: n.role.clone(),
        name: n.name.clone(),
        frame,
    });
    // stash ref in value field via a side channel: encode in name? Better: temp field.
    // Reuse `live` as ref storage after prune.
    n.live = r;
    for c in &mut n.children {
        assign_refs(c, frame, next, refs, focused, dialog);
    }
}

fn yaml_escape(s: &str) -> String {
    if s.is_empty() {
        return String::new();
    }
    if s.chars().any(|c| c.is_whitespace() || matches!(c, '"' | ':' | '#' | '[' | ']')) {
        format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\""))
    } else {
        format!("\"{s}\"")
    }
}

fn render_yaml(n: &Node, indent: usize) -> String {
    if n.ignored || n.role == "Ignored" {
        let mut out = String::new();
        for c in &n.children {
            out.push_str(&render_yaml(c, indent));
        }
        return out;
    }
    // Skip the document root wrapper in the body (header carries url/title).
    if indent == 0 && n.role == "document" {
        let mut out = String::new();
        for c in &n.children {
            out.push_str(&render_yaml(c, 0));
        }
        return out;
    }
    let pad = "  ".repeat(indent);
    let mut line = format!("{pad}- {}", n.role);
    if !n.name.is_empty() {
        line.push(' ');
        line.push_str(&yaml_escape(&n.name));
    }
    let mut attrs = Vec::new();
    if !n.live.is_empty() {
        attrs.push(format!("ref={}", n.live));
    }
    if let Some(l) = n.level {
        attrs.push(format!("level={l}"));
    }
    if let Some(c) = &n.checked {
        attrs.push(format!("checked={c}"));
    }
    if let Some(p) = &n.pressed {
        attrs.push(format!("pressed={p}"));
    }
    if n.expanded == Some(true) {
        attrs.push("expanded".into());
    }
    if n.selected == Some(true) {
        attrs.push("selected".into());
    }
    if n.disabled {
        attrs.push("disabled".into());
    }
    if n.required {
        attrs.push("required".into());
    }
    if n.readonly {
        attrs.push("readonly".into());
    }
    if n.focused && n.role != "document" {
        attrs.push("active".into());
    }
    if (n.role == "textbox" || n.role == "searchbox" || n.role == "combobox") && !n.value.is_empty()
    {
        attrs.push(format!("value={}", yaml_escape(&n.value)));
    }
    if !attrs.is_empty() {
        line.push_str(" [");
        line.push_str(&attrs.join(", "));
        line.push(']');
    }
    let mut out = line;
    if n.children.is_empty() {
        out.push('\n');
        return out;
    }
    out.push_str(":\n");
    for c in &n.children {
        out.push_str(&render_yaml(c, indent + 1));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn nodes_json() -> &'static str {
        r#"{
          "nodes": [
            {"nodeId":"1","role":{"value":"RootWebArea"},"name":{"value":"Checkout"},"childIds":["2","3"],"backendDOMNodeId":1},
            {"nodeId":"2","role":{"value":"heading"},"name":{"value":"Checkout"},"childIds":[],"backendDOMNodeId":2,"properties":[{"name":"level","value":{"value":1}}]},
            {"nodeId":"3","role":{"value":"generic"},"name":{"value":""},"childIds":["4","5","6"],"backendDOMNodeId":3},
            {"nodeId":"4","role":{"value":"main"},"name":{"value":""},"childIds":["7","8"],"backendDOMNodeId":4},
            {"nodeId":"5","ignored":true,"role":{"value":"Ignored"},"childIds":[],"backendDOMNodeId":5},
            {"nodeId":"6","role":{"value":"banner"},"name":{"value":""},"childIds":["9"],"backendDOMNodeId":6},
            {"nodeId":"7","role":{"value":"textbox"},"name":{"value":"Email"},"childIds":[],"backendDOMNodeId":7,"properties":[{"name":"focusable","value":{"value":true}},{"name":"focused","value":{"value":true}}]},
            {"nodeId":"8","role":{"value":"button"},"name":{"value":"Pay now"},"childIds":[],"backendDOMNodeId":8,"properties":[{"name":"focusable","value":{"value":true}}]},
            {"nodeId":"9","role":{"value":"link"},"name":{"value":"Home"},"childIds":[],"backendDOMNodeId":9}
          ]
        }"#
    }

    #[test]
    fn prunes_generic_and_assigns_stable_backend_refs() {
        let nodes = parse_cdp_nodes(nodes_json()).unwrap();
        let snap = snapshot_from_cdp(
            &nodes,
            SnapshotOpts {
                url: "https://example.com/cart".into(),
                title: "Checkout".into(),
                ..SnapshotOpts::default()
            },
        )
        .unwrap();
        let text = snap.render();
        assert!(text.contains("url: https://example.com/cart"));
        assert!(text.contains("title: Checkout"));
        assert!(text.contains("focused:"));
        assert!(text.contains("heading \"Checkout\""));
        assert!(text.contains("textbox \"Email\""));
        assert!(text.contains("button \"Pay now\""));
        assert!(!text.contains("generic"), "nameless generic collapsed:\n{text}");
        assert!(
            !text.to_ascii_lowercase().contains("- ignored"),
            "ignored node kept:\n{text}"
        );
        let pay = snap
            .refs
            .iter()
            .find(|r| r.name == "Pay now")
            .expect("pay ref");
        assert_eq!(pay.backend_node_id, 8);
        assert!(pay.r#ref.starts_with('e'));
    }

    #[test]
    fn interactive_drops_heading_keeps_controls() {
        let nodes = parse_cdp_nodes(nodes_json()).unwrap();
        let snap = snapshot_from_cdp(
            &nodes,
            SnapshotOpts {
                interactive: true,
                ..SnapshotOpts::default()
            },
        )
        .unwrap();
        assert!(!snap.yaml.contains("heading"));
        assert!(snap.yaml.contains("textbox"));
        assert!(snap.yaml.contains("button"));
    }

    #[test]
    fn subtree_and_stale_backend() {
        let nodes = parse_cdp_nodes(nodes_json()).unwrap();
        let snap = snapshot_from_cdp(
            &nodes,
            SnapshotOpts {
                subtree_backend: Some(4),
                ..SnapshotOpts::default()
            },
        )
        .unwrap();
        assert!(snap.yaml.contains("textbox"));
        assert!(!snap.yaml.contains("Home"));
        let err = snapshot_from_cdp(
            &nodes,
            SnapshotOpts {
                subtree_backend: Some(999),
                ..SnapshotOpts::default()
            },
        )
        .unwrap_err();
        assert!(err.contains("snapshot again"));
    }

    #[test]
    fn find_lines() {
        let yaml = "- button \"Pay now\" [ref=e12]\n- link \"Home\" [ref=e3]\n";
        let hits = find_in_yaml(yaml, "pay");
        assert_eq!(hits.len(), 1);
        assert!(hits[0].contains("e12"));
    }

    #[test]
    fn parses_role_without_value_field() {
        let json = r#"{"nodes":[{"nodeId":"1","role":{"type":"internalRole"},"childIds":[],"backendDOMNodeId":1}]}"#;
        let nodes = parse_cdp_nodes(json).unwrap();
        assert_eq!(nodes[0].role.as_ref().unwrap().value, serde_json::Value::Null);
    }

    #[test]
    fn debug_json_omits_backend_ids() {
        let nodes = parse_cdp_nodes(nodes_json()).unwrap();
        let snap = snapshot_from_cdp(&nodes, SnapshotOpts::default()).unwrap();
        let j = snap.as_debug_json().to_string();
        assert!(!j.contains("backend"));
        assert!(j.contains("yaml"));
    }
}
