//! Web Notification API → Sola system notifications.
//!
//! Chromium's Linux libnotify path has no daemon on this desk. We override
//! `window.Notification` in every frame and ferry show / permission through
//! the helper console bridge.

use std::collections::HashMap;
use std::path::PathBuf;

use sola_bus::topics::AppNotification;

use crate::profiles;

pub const SHOW_PREFIX: &str = "__sola_notify__";
pub const PERM_PREFIX: &str = "__sola_notify_perm__";

pub const APP_ID: &str = "sola-browser";

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ShowPayload {
    pub title: String,
    #[serde(default)]
    pub body: String,
    #[serde(default)]
    pub tag: Option<String>,
    #[serde(default)]
    pub origin: String,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct PermPayload {
    pub id: u64,
    #[serde(default)]
    pub origin: String,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct IpcShow {
    pub tab_id: u64,
    pub origin: String,
    pub title: String,
    pub body: String,
    pub tag: Option<String>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct IpcPerm {
    pub req_id: u64,
    pub origin: String,
    pub tab_id: u64,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum Ipc {
    Show(IpcShow),
    Perm(IpcPerm),
}

pub fn permissions_path(profile_id: &str) -> PathBuf {
    profiles::profile_data_dir(profile_id).join("notifications.json")
}

pub fn load_map(profile_id: &str) -> HashMap<String, String> {
    let path = permissions_path(profile_id);
    let Ok(bytes) = std::fs::read(&path) else {
        return HashMap::new();
    };
    serde_json::from_slice(&bytes).unwrap_or_default()
}

pub fn permission_for(profile_id: &str, origin: &str) -> String {
    load_map(profile_id)
        .get(origin)
        .cloned()
        .unwrap_or_else(|| "default".into())
}

pub fn set_permission(profile_id: &str, origin: &str, value: &str) -> Result<(), String> {
    let mut map = load_map(profile_id);
    map.insert(origin.to_string(), value.to_string());
    let path = permissions_path(profile_id);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let json = serde_json::to_vec_pretty(&map).map_err(|e| e.to_string())?;
    std::fs::write(&path, json).map_err(|e| e.to_string())
}

/// Injected in every frame. `map_json` is a JSON object of origin → permission.
///
/// Does **not** replace `requestPermission` — Chromium's native call is what
/// fires CEF `OnShowPermissionPrompt`. We only wrap the constructor so a
/// granted `new Notification(...)` also exits to Sola's desk cards.
///
/// Safe in the CEF **renderer** (no profile bind): pass `"{}"`.
pub fn inject_script(map_json: &str) -> String {
    format!(
        r#"(function(){{
  var map = {map};
  window.__sola_notify_map = map;
  var Native = window.Notification;
  if (!Native) return;
  if (window.__sola_notify_hook) return;
  window.__sola_notify_hook = 1;
  function SolaNotification(title, opts) {{
    opts = opts || {{}};
    try {{
      var p = Native.permission || (map && map[location.origin]) || 'default';
      if (p === 'granted') {{
        console.info('{show}' + JSON.stringify({{
          title: String(title || ''),
          body: String(opts.body || ''),
          tag: opts.tag ? String(opts.tag) : null,
          origin: location.origin
        }}));
      }}
    }} catch (e) {{}}
    try {{ return new Native(title, opts); }} catch (e) {{}}
  }}
  try {{
    SolaNotification.prototype = Native.prototype;
    Object.setPrototypeOf(SolaNotification, Native);
  }} catch (e) {{}}
  try {{
    Object.defineProperty(SolaNotification, 'permission', {{
      get: function() {{ return Native.permission; }}
    }});
  }} catch (e) {{
    SolaNotification.permission = Native.permission;
  }}
  SolaNotification.requestPermission = function () {{
    return Native.requestPermission.apply(Native, arguments);
  }};
  window.Notification = SolaNotification;
}})();"#,
        map = map_json,
        show = SHOW_PREFIX,
    )
}

pub fn resolve_script(req_id: u64, result: &str) -> String {
    let result = if result == "granted" { "granted" } else { "denied" };
    format!(
        r#"(function(){{
  if (window.Notification) window.Notification.permission = '{result}';
  var r = window.__sola_notify_resolvers;
  if (r && r[{id}]) {{ r[{id}]('{result}'); delete r[{id}]; }}
}})();"#,
        result = result,
        id = req_id,
    )
}

pub fn host_of(origin: &str) -> String {
    let rest = origin.split("://").nth(1).unwrap_or(origin);
    let hostport = rest.split('/').next().unwrap_or(rest);
    let host = hostport.split(':').next().unwrap_or(hostport);
    if host.is_empty() {
        origin.to_string()
    } else {
        host.to_string()
    }
}

pub fn to_bus(show: &IpcShow) -> AppNotification {
    AppNotification {
        id: format!("web-{}-{}", show.tab_id, now_millis()),
        app_id: APP_ID.into(),
        source: host_of(&show.origin),
        title: show.title.clone(),
        body: show.body.clone(),
        tag: show.tag.clone(),
        tab_id: Some(show.tab_id),
        url: if show.origin.is_empty() {
            None
        } else {
            Some(show.origin.clone())
        },
    }
}

fn now_millis() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0)
}

pub fn parse_show(raw: &str) -> Option<ShowPayload> {
    serde_json::from_str(raw).ok()
}

pub fn parse_perm(raw: &str) -> Option<PermPayload> {
    serde_json::from_str(raw).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inject_defines_notification() {
        let s = inject_script(r#"{"https://ex.com":"granted"}"#);
        assert!(s.contains("window.Notification"));
        assert!(s.contains(SHOW_PREFIX));
        assert!(s.contains("requestPermission"));
        assert!(s.contains("https://ex.com"));
        assert!(s.contains("window.__sola_notify_hook"));
        let empty = inject_script("{}");
        assert!(empty.contains("var map = {}"));
    }

    #[test]
    fn host_strips_origin() {
        assert_eq!(host_of("https://news.ycombinator.com"), "news.ycombinator.com");
        assert_eq!(host_of("not-a-url"), "not-a-url");
    }

    #[test]
    fn to_bus_sets_browser_app() {
        let n = to_bus(&IpcShow {
            tab_id: 3,
            origin: "https://ex.com".into(),
            title: "Hi".into(),
            body: "there".into(),
            tag: Some("t".into()),
        });
        assert_eq!(n.app_id, APP_ID);
        assert_eq!(n.source, "ex.com");
        assert_eq!(n.tab_id, Some(3));
        assert_eq!(n.tag.as_deref(), Some("t"));
    }
}
