//! getUserMedia / huddle mic (and camera) — kit Allow / Block.
//!
//! CEF OSR has no Chromium permission bubble. Alloy default-denies
//! `OnRequestMediaAccessPermission`. We persist per origin next to the
//! profile (wrapper: `…/wrapper/<id>/media.json`) and let chrome show
//! the same graphite overlay as notifications.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::mpsc::Sender;

use crate::engine::{Cmd, Engine};
use crate::notify;
use crate::profiles;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
pub struct IpcMedia {
    pub origin: String,
    pub tab_id: u64,
    pub audio: bool,
    pub video: bool,
    pub screen: bool,
    #[serde(default)]
    pub prompt_id: Option<u64>,
    #[serde(default)]
    pub access_id: Option<u64>,
}

pub fn permissions_path(profile_id: &str) -> PathBuf {
    profiles::data_dir_for(profile_id).join("media.json")
}

pub fn load_map(profile_id: &str) -> HashMap<String, String> {
    let path = permissions_path(profile_id);
    let Ok(bytes) = std::fs::read(&path) else {
        return HashMap::new();
    };
    serde_json::from_slice(&bytes).unwrap_or_default()
}

pub fn permission_for(profile_id: &str, origin: &str) -> String {
    let map = load_map(profile_id);
    let key = notify::canon_origin(origin);
    map.get(&key)
        .or_else(|| map.get(origin))
        .or_else(|| map.get(&format!("{key}/")))
        .cloned()
        .unwrap_or_else(|| "default".into())
}

pub fn set_permission(profile_id: &str, origin: &str, value: &str) -> Result<(), String> {
    let key = notify::canon_origin(origin);
    let mut map = load_map(profile_id);
    map.remove(origin);
    map.remove(&format!("{key}/"));
    map.insert(key, value.to_string());
    let path = permissions_path(profile_id);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let json = serde_json::to_vec_pretty(&map).map_err(|e| e.to_string())?;
    std::fs::write(&path, json).map_err(|e| e.to_string())
}

pub fn merge(into: &mut IpcMedia, from: &IpcMedia) {
    into.audio |= from.audio;
    into.video |= from.video;
    into.screen |= from.screen;
    if into.prompt_id.is_none() {
        into.prompt_id = from.prompt_id;
    }
    if into.access_id.is_none() {
        into.access_id = from.access_id;
    }
}

/// Title + `{host} wants to …` body for the kit overlay.
pub fn copy(media: &IpcMedia) -> (&'static str, String) {
    let host = notify::host_of(&media.origin);
    if media.screen {
        return (
            "Screen share",
            format!("{host} wants to share your screen."),
        );
    }
    match (media.audio, media.video) {
        (true, true) => (
            "Camera and microphone",
            format!("{host} wants to use your camera and microphone."),
        ),
        (false, true) => ("Camera", format!("{host} wants to use your camera.")),
        (true, false) | (false, false) => (
            "Microphone",
            format!("{host} wants to use your microphone."),
        ),
    }
}

pub fn from_access_bits(origin: String, tab_id: u64, bits: u32, access_id: u64) -> IpcMedia {
    let audio = bits & audio_capture_bits() != 0;
    let video = bits & video_capture_bits() != 0;
    let screen = bits & desktop_bits() != 0;
    IpcMedia {
        origin,
        tab_id,
        audio: audio || (!video && !screen),
        video,
        screen,
        prompt_id: None,
        access_id: Some(access_id),
    }
}

pub fn from_prompt_bits(origin: String, tab_id: u64, bits: u32, prompt_id: u64) -> IpcMedia {
    IpcMedia {
        origin,
        tab_id,
        audio: bits & mic_stream_bit() != 0,
        video: bits & camera_stream_bit() != 0,
        screen: false,
        prompt_id: Some(prompt_id),
        access_id: None,
    }
}

pub fn is_media_prompt(bits: u32) -> bool {
    bits & (mic_stream_bit() | camera_stream_bit()) != 0
}

/// Origin URL Chromium `SetContentSetting` will accept. `about:` / `data:`
/// / empty are skipped — huddle `about:blank` inherits the opener origin
/// for getUserMedia; we never persist a grant on the blank URL itself.
pub fn content_setting_origin(origin: &str) -> Option<String> {
    let url = crate::notify::canon_origin(origin);
    if url.is_empty() || url == "null" {
        return None;
    }
    let lower = url.to_ascii_lowercase();
    if lower.starts_with("about:") || lower.starts_with("data:") {
        return None;
    }
    Some(url)
}

pub fn send_resolve<E: Engine>(tx: &Sender<Cmd<E>>, media: &IpcMedia, granted: bool) {
    if let Some(prompt_id) = media.prompt_id {
        let _ = tx.send(Cmd::NotifyPermission { prompt_id, granted });
    }
    if let Some(req_id) = media.access_id {
        let _ = tx.send(Cmd::MediaPermission { req_id, granted });
    }
}

pub fn audio_capture_bits() -> u32 {
    cef::MediaAccessPermissionTypes::DEVICE_AUDIO_CAPTURE.get_raw()
        | cef::MediaAccessPermissionTypes::DESKTOP_AUDIO_CAPTURE.get_raw()
}

pub fn video_capture_bits() -> u32 {
    cef::MediaAccessPermissionTypes::DEVICE_VIDEO_CAPTURE.get_raw()
}

pub fn desktop_bits() -> u32 {
    cef::MediaAccessPermissionTypes::DESKTOP_VIDEO_CAPTURE.get_raw()
        | cef::MediaAccessPermissionTypes::DESKTOP_AUDIO_CAPTURE.get_raw()
}

pub fn mic_stream_bit() -> u32 {
    cef::PermissionRequestTypes::MIC_STREAM.get_raw()
}

pub fn camera_stream_bit() -> u32 {
    cef::PermissionRequestTypes::CAMERA_STREAM.get_raw()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn copy_microphone() {
        let m = IpcMedia {
            origin: "https://app.slack.com".into(),
            tab_id: 1,
            audio: true,
            video: false,
            screen: false,
            prompt_id: None,
            access_id: Some(1),
        };
        let (title, hint) = copy(&m);
        assert_eq!(title, "Microphone");
        assert!(hint.contains("app.slack.com"));
        assert!(hint.contains("microphone"));
    }

    #[test]
    fn copy_both() {
        let m = IpcMedia {
            origin: "https://app.slack.com/".into(),
            tab_id: 1,
            audio: true,
            video: true,
            screen: false,
            prompt_id: Some(2),
            access_id: None,
        };
        let (title, hint) = copy(&m);
        assert_eq!(title, "Camera and microphone");
        assert!(hint.contains("camera and microphone"));
    }

    #[test]
    fn merge_attaches_access_id() {
        let mut a = IpcMedia {
            origin: "https://ex.com".into(),
            tab_id: 1,
            audio: true,
            video: false,
            screen: false,
            prompt_id: Some(9),
            access_id: None,
        };
        let b = IpcMedia {
            origin: "https://ex.com".into(),
            tab_id: 1,
            audio: true,
            video: true,
            screen: false,
            prompt_id: None,
            access_id: Some(4),
        };
        merge(&mut a, &b);
        assert_eq!(a.prompt_id, Some(9));
        assert_eq!(a.access_id, Some(4));
        assert!(a.video);
    }

    #[test]
    fn media_prompt_bits_include_mic() {
        assert!(is_media_prompt(mic_stream_bit()));
        assert!(is_media_prompt(camera_stream_bit()));
        assert!(!is_media_prompt(
            cef::PermissionRequestTypes::NOTIFICATIONS.get_raw()
        ));
    }

    #[test]
    fn content_setting_origin_skips_blank() {
        assert_eq!(
            content_setting_origin("https://app.slack.com/"),
            Some("https://app.slack.com".into())
        );
        assert_eq!(content_setting_origin("about:blank"), None);
        assert_eq!(content_setting_origin(""), None);
    }
}
