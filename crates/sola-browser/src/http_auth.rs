//! HTTP Basic / Digest auth (CEF `GetAuthCredentials`).
//!
//! OSR has no usable Chromium auth chrome, and the in-memory auth cache
//! dies with the helper. We prompt with a kit modal and persist the
//! credentials per profile so a restart does not ask again.

use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};

use iced::widget::{Space, column, container, mouse_area, row, stack, text};
use iced::{Alignment, Element, Length};

use sola_core::Encrypted;
use sola_kit::components::button as kit_button;
use sola_kit::components::card;
use sola_kit::components::field::field;
use sola_kit::components::style::{SPACE_MD, SPACE_SM};
use sola_kit::components::text_input::text_input;
use sola_kit::fonts;
use tracing::{info, warn};

use crate::notify;
use crate::profiles;

static OPEN: AtomicBool = AtomicBool::new(false);

pub fn is_open() -> bool {
    OPEN.load(Ordering::Relaxed)
}

pub fn set_open(open: bool) {
    OPEN.store(open, Ordering::Relaxed);
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Key {
    pub host: String,
    pub port: i32,
    pub realm: String,
    pub scheme: String,
    pub is_proxy: bool,
}

impl Key {
    pub fn from_parts(host: &str, port: i32, realm: &str, scheme: &str, is_proxy: bool) -> Self {
        Self {
            host: host.trim().to_ascii_lowercase(),
            port,
            realm: realm.to_string(),
            scheme: scheme.to_ascii_lowercase(),
            is_proxy,
        }
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct Entry {
    host: String,
    port: i32,
    realm: String,
    scheme: String,
    #[serde(default)]
    is_proxy: bool,
    username: String,
    password: String,
}

impl Entry {
    fn key(&self) -> Key {
        Key::from_parts(
            &self.host,
            self.port,
            &self.realm,
            &self.scheme,
            self.is_proxy,
        )
    }
}

#[derive(serde::Serialize, serde::Deserialize, Default)]
struct OnDisk {
    #[serde(default)]
    entries: Option<Encrypted<Vec<Entry>>>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Ipc {
    pub id: u64,
    pub tab_id: u64,
    pub origin: String,
    pub host: String,
    pub realm: String,
    pub scheme: String,
    /// Prefill after a rejected attempt (or a stored username).
    #[serde(default)]
    pub username: String,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum Event {
    Open(Ipc),
    Reset { ids: Vec<u64> },
}

pub fn user_input_id() -> iced::advanced::widget::Id {
    iced::advanced::widget::Id::new("http-auth-user")
}

pub fn pass_input_id() -> iced::advanced::widget::Id {
    iced::advanced::widget::Id::new("http-auth-pass")
}

pub fn title(ipc: &Ipc) -> String {
    let host = if ipc.host.is_empty() {
        notify::host_of(&ipc.origin)
    } else {
        ipc.host.clone()
    };
    if host.is_empty() {
        "This site".into()
    } else {
        host
    }
}

pub fn message(ipc: &Ipc) -> String {
    let host = title(ipc);
    if ipc.realm.trim().is_empty() {
        format!("{host} requires a username and password.")
    } else {
        format!(
            "{host} requires a username and password for “{}”.",
            ipc.realm.trim()
        )
    }
}

fn store_path(profile_id: &str) -> PathBuf {
    profiles::data_dir_for(profile_id).join("http-auth.json")
}

fn load_entries(profile_id: &str) -> Vec<Entry> {
    let path = store_path(profile_id);
    let raw = match std::fs::read_to_string(&path) {
        Ok(s) => s,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Vec::new(),
        Err(e) => {
            warn!(path = %path.display(), error = %e, "http-auth: read failed");
            return Vec::new();
        }
    };
    match serde_json::from_str::<OnDisk>(&raw) {
        Ok(disk) => disk.entries.map(|e| e.0).unwrap_or_default(),
        Err(e) => {
            warn!(path = %path.display(), error = %e, "http-auth: parse failed");
            Vec::new()
        }
    }
}

fn save_entries(profile_id: &str, entries: Vec<Entry>) {
    let path = store_path(profile_id);
    if let Some(parent) = path.parent() {
        if let Err(e) = std::fs::create_dir_all(parent) {
            warn!(path = %parent.display(), error = %e, "http-auth: dir");
            return;
        }
    }
    let disk = OnDisk {
        entries: Some(Encrypted(entries)),
    };
    let body = match serde_json::to_string(&disk) {
        Ok(s) => s,
        Err(e) => {
            warn!(error = %e, "http-auth: serialize failed");
            return;
        }
    };
    let tmp = path.with_extension("json.tmp");
    if let Err(e) = std::fs::write(&tmp, body.as_bytes()) {
        warn!(path = %tmp.display(), error = %e, "http-auth: write failed");
        return;
    }
    if let Err(e) = std::fs::rename(&tmp, &path) {
        warn!(path = %path.display(), error = %e, "http-auth: rename failed");
        let _ = std::fs::remove_file(&tmp);
        return;
    }
    if let Err(e) = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)) {
        warn!(path = %path.display(), error = %e, "http-auth: chmod 0600 failed");
    }
}

pub fn lookup(profile_id: &str, key: &Key) -> Option<(String, String)> {
    load_entries(profile_id)
        .into_iter()
        .find(|e| e.key() == *key)
        .map(|e| (e.username, e.password))
}

pub fn save(profile_id: &str, key: &Key, username: &str, password: &str) {
    let mut entries = load_entries(profile_id);
    entries.retain(|e| e.key() != *key);
    entries.push(Entry {
        host: key.host.clone(),
        port: key.port,
        realm: key.realm.clone(),
        scheme: key.scheme.clone(),
        is_proxy: key.is_proxy,
        username: username.to_string(),
        password: password.to_string(),
    });
    save_entries(profile_id, entries);
    info!(host = %key.host, port = key.port, "http-auth: saved");
}

pub fn forget(profile_id: &str, key: &Key) {
    let mut entries = load_entries(profile_id);
    let before = entries.len();
    entries.retain(|e| e.key() != *key);
    if entries.len() != before {
        save_entries(profile_id, entries);
        info!(host = %key.host, "http-auth: forgot rejected credentials");
    }
}

pub fn overlay<'a, Message: Clone + 'a>(
    dlg: &'a Ipc,
    username: &'a str,
    password: &'a str,
    on_ok: Message,
    on_cancel: Message,
    on_user: impl Fn(String) -> Message + 'a,
    on_pass: impl Fn(String) -> Message + 'a,
    on_user_submit: Message,
) -> Element<'a, Message> {
    let heading = text(title(dlg)).size(15).font(fonts::ui_medium());
    let body = text(message(dlg)).size(13).width(Length::Fill);
    let user = text_input("Username", username)
        .id(user_input_id())
        .size(13)
        .style(sola_kit::components::text_input::style)
        .width(Length::Fill)
        .on_input(on_user)
        .on_submit(on_user_submit);
    let pass = text_input("Password", password)
        .id(pass_input_id())
        .size(13)
        .secure(true)
        .style(sola_kit::components::text_input::style)
        .width(Length::Fill)
        .on_input(on_pass)
        .on_submit(on_ok.clone());
    let actions = row![
        kit_button::labeled("Sign in", kit_button::primary).on_press(on_ok.clone()),
        kit_button::labeled("Cancel", kit_button::ghost).on_press(on_cancel.clone()),
    ]
    .spacing(SPACE_SM)
    .align_y(Alignment::Center);
    let col = column![
        heading,
        body,
        field("Username", user, None, None),
        field("Password", pass, None, None),
        actions,
    ]
    .spacing(SPACE_SM)
    .width(Length::Fixed(300.0));
    let panel =
        card::modal(container(col).padding(SPACE_MD + SPACE_SM)).width(Length::Fixed(340.0));
    let backdrop = mouse_area(
        container(Space::new().width(Length::Fill).height(Length::Fill)).style(|_t| {
            container::Style {
                background: Some(iced::Background::Color(iced::Color::from_rgba(
                    0.0, 0.0, 0.0, 0.22,
                ))),
                ..container::Style::default()
            }
        }),
    )
    .on_press(on_cancel);
    let centered = container(panel)
        .width(Length::Fill)
        .height(Length::Fill)
        .align_x(Alignment::Center)
        .align_y(Alignment::Center);
    stack![backdrop, centered].into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn key_normalizes_host_case() {
        let a = Key::from_parts("Example.COM", 443, "r", "basic", false);
        let b = Key::from_parts("example.com", 443, "r", "Basic", false);
        assert_eq!(a, b);
    }

    #[test]
    fn message_includes_realm() {
        let ipc = Ipc {
            id: 1,
            tab_id: 0,
            origin: "https://box.example/".into(),
            host: "box.example".into(),
            realm: "NAS".into(),
            scheme: "basic".into(),
            username: String::new(),
        };
        let m = message(&ipc);
        assert!(m.contains("box.example"));
        assert!(m.contains("NAS"));
        assert_eq!(title(&ipc), "box.example");
    }

    #[test]
    fn store_path_is_per_profile() {
        let p = store_path("abc");
        assert!(p.ends_with("http-auth.json"));
        assert!(p.to_string_lossy().contains("abc"));
    }
}
