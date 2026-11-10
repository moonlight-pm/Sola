//! Persistent sticky storage for the bus host.
//!
//! Persistent topics live in one of two layouts on disk:
//!
//! - **Default (no `namespace` annotation):** a single shared file
//!   `~/.config/sola/state.yaml` with one top-level key per topic
//!   kind (a YAML sequence for keyed topics, a mapping otherwise).
//! - **Namespaced (`#[persistent(namespace = "...")]`):** a dedicated
//!   YAML file per topic, named by the resolved namespace template.
//!   The payload is the entire document; no top-level key.
//!
//! On bus startup the host reads both layouts and replays each topic
//! as a sticky message keyed by `source = "sola-bus"`. Subsequent emits
//! and retracts route by `Topic::path_for()`: namespaced topics use the
//! namespaced helpers; the rest go through the shared state.yaml path.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde_yaml_ng::{Mapping, Value};
use tracing::{debug, info, warn};

use crate::Message;
use crate::topics::{Topic, TopicKind};

/// `Message::source` used for sticky entries restored from disk. A
/// client emit overrides this the first time it replaces the value.
pub const BUS_SOURCE: &str = "sola-bus";

/// Kinds retired from the bus. A `state.yaml` from before may still hold a
/// section for one; it is skipped quietly, where an unknown name warns.
pub const RETIRED: &[&str] = &[
    "CustomTheme", "Zones", "TerminalConfig", "MonitorConfig", "BrowserConfig",
    "BrowserHistory", "BrowserTab", "PinnedTabs",
];

/// `~/.config/sola/state.yaml`.
pub fn state_path() -> PathBuf {
    sola_core::config::sola_config_dir().join("state.yaml")
}

/// What the bus kept on disk for a kind since moved to its owner's own
/// file (`sola_core::state_file`): section `section` of state.yaml. The
/// owner reads it until it has saved its own copy.
pub fn legacy_section<T: serde::de::DeserializeOwned>(section: &str) -> Option<T> {
    legacy_section_at(&state_path(), section)
}

fn legacy_section_at<T: serde::de::DeserializeOwned>(path: &Path, section: &str) -> Option<T> {
    let raw = fs::read_to_string(path).ok()?;
    let mapping: Mapping = serde_yaml_ng::from_str(&raw).ok()?;
    serde_yaml_ng::from_value(mapping.get(section)?.clone()).ok()
}

/// [`legacy_section`] for a kind the bus kept in a file of its own under
/// the config dir (`shell/tabs.yaml`).
pub fn legacy_file<T: serde::de::DeserializeOwned>(relative: &str) -> Option<T> {
    let raw = fs::read_to_string(sola_core::config::sola_config_dir().join(relative)).ok()?;
    serde_yaml_ng::from_str(&raw).ok()
}

/// Read state.yaml and return one sticky `Message` per valid
/// persistent section. Missing file → empty vec. Parse errors,
/// unknown sections, non-persistent sections, and schema mismatches
/// are logged and skipped.
pub fn load(path: &Path) -> Vec<Message> {
    let raw = match fs::read_to_string(path) {
        Ok(s) => s,
        Err(e) if e.kind() == io::ErrorKind::NotFound => {
            info!(path = %path.display(), "no state.yaml yet");
            return Vec::new();
        }
        Err(e) => {
            warn!(path = %path.display(), %e, "state.yaml read failed");
            return Vec::new();
        }
    };

    let mapping: Mapping = match serde_yaml_ng::from_str(&raw) {
        Ok(m) => m,
        Err(e) => {
            warn!(path = %path.display(), %e, "state.yaml parse failed; starting empty");
            return Vec::new();
        }
    };

    let mut out = Vec::new();
    for (key, value) in mapping {
        let Some(section) = key.as_str().map(str::to_owned) else {
            warn!("non-string top-level key in state.yaml; skipping");
            continue;
        };
        let Some(kind) = TopicKind::from_str(&section) else {
            if RETIRED.contains(&section.as_str()) {
                debug!(section = %section, "retired topic; skipping");
            } else {
                warn!(section = %section, "unknown persistent topic; skipping");
            }
            continue;
        };
        if !kind.behavior().is_persistent() {
            warn!(section = %section, "section is not a persistent topic; skipping");
            continue;
        }

        if kind.has_keys() {
            // Expect a sequence of mappings. Each entry is a separate
            // record with its own keys.
            let Value::Sequence(entries) = value else {
                warn!(section = %section, "keyed topic expects sequence of mappings; skipping");
                continue;
            };
            for entry in entries {
                let Some(topic) = Topic::from_yaml_section(kind, entry) else {
                    warn!(section = %section, "failed to deserialize keyed entry; skipping");
                    continue;
                };
                let mut msg = topic.to_message();
                msg.sticky = true;
                msg.source = BUS_SOURCE.to_string();
                info!(section = %section, keys = ?msg.keys, "restored keyed sticky");
                out.push(msg);
            }
        } else {
            let Some(topic) = Topic::from_yaml_section(kind, value) else {
                warn!(section = %section, "failed to deserialize section; skipping");
                continue;
            };
            let mut msg = topic.to_message();
            msg.sticky = true;
            msg.source = BUS_SOURCE.to_string();
            info!(section = %section, "restored persistent sticky");
            out.push(msg);
        }
    }
    out
}

/// Update a single section in state.yaml with `topic`'s current
/// value. No-ops for non-persistent topics. Load → replace →
/// `temp + rename` atomic write so a crash mid-write can't leave a
/// torn file.
pub fn write_section(path: &Path, topic: &Topic) -> io::Result<()> {
    let Some(value) = topic.to_yaml_value() else {
        return Ok(());
    };
    let kind = topic.kind();
    let section = kind.as_str().to_string();

    let mut mapping = match fs::read_to_string(path) {
        Ok(s) => serde_yaml_ng::from_str::<Mapping>(&s).unwrap_or_else(|e| {
            warn!(path = %path.display(), %e, "state.yaml parse failed during write; rewriting from empty");
            Mapping::new()
        }),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Mapping::new(),
        Err(e) => return Err(e),
    };

    let section_key = Value::String(section);

    if kind.has_keys() {
        // Keyed: upsert into `Section:` sequence by key match.
        let keys = topic.keys_for();
        let existing = mapping.remove(&section_key);
        let mut entries = match existing {
            Some(Value::Sequence(arr)) => arr,
            _ => Vec::new(),
        };
        entries.retain(|entry| !entry_matches_keys(kind, entry, &keys));
        entries.push(value);
        mapping.insert(section_key, Value::Sequence(entries));
    } else {
        mapping.insert(section_key, value);
    }

    let content =
        serde_yaml_ng::to_string(&mapping).expect("top-level yaml mapping always serializes");
    atomic_write(path, content.as_bytes())
}

/// Remove the matching record from the persistent topic's section in
/// state.yaml. For keyed kinds, removes the entry whose key fields
/// match `event.keys`; if it was the last entry, drops the section.
/// For unkeyed kinds, drops the section.
pub fn retract_section(path: &Path, event: &Message) -> io::Result<()> {
    let Some(kind) = TopicKind::from_str(&event.topic) else {
        return Ok(());
    };
    if !kind.behavior().is_persistent() {
        return Ok(());
    }

    let mut mapping = match fs::read_to_string(path) {
        Ok(s) => serde_yaml_ng::from_str::<Mapping>(&s).unwrap_or_else(|e| {
            warn!(path = %path.display(), %e, "state.yaml parse failed during retract; rewriting from empty");
            Mapping::new()
        }),
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(e),
    };

    let section_key = Value::String(kind.as_str().to_string());

    if kind.has_keys() {
        if let Some(Value::Sequence(mut entries)) = mapping.remove(&section_key) {
            entries.retain(|entry| !entry_matches_keys(kind, entry, &event.keys));
            if !entries.is_empty() {
                mapping.insert(section_key, Value::Sequence(entries));
            }
        }
    } else {
        mapping.remove(&section_key);
    }

    let content =
        serde_yaml_ng::to_string(&mapping).expect("top-level yaml mapping always serializes");
    atomic_write(path, content.as_bytes())
}

/// True if the YAML entry's serialized payload, when parsed back through
/// the topic's `from_yaml_section`, would produce a `Topic` whose
/// `keys_for()` matches `keys`. We do the round-trip rather than
/// inspecting raw YAML so the key-extraction logic stays in one place
/// (the macro-generated `keys_for`).
fn entry_matches_keys(kind: TopicKind, entry: &Value, keys: &[String]) -> bool {
    let Some(topic) = Topic::from_yaml_section(kind, entry.clone()) else {
        return false;
    };
    topic.keys_for() == keys
}

fn atomic_write(path: &Path, content: &[u8]) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let tmp = path.with_extension("yaml.tmp");
    fs::write(&tmp, content)?;
    fs::rename(&tmp, path)?;
    Ok(())
}

// ---- Namespaced storage (per-topic dedicated files) ----------------------
//
// Topics annotated with `#[persistent(namespace = "...")]` live in their
// own file (no top-level key — the kind is implied by the path).
// Keyed namespaces interpolate `:placeholder` segments to a per-key file.

/// Write a namespaced topic's payload as the whole file's content.
/// No-op for non-persistent topics or payloads that can't serialize to
/// YAML.
pub fn write_namespaced(path: &Path, topic: &Topic) -> io::Result<()> {
    let Some(value) = topic.to_yaml_value() else {
        return Ok(());
    };
    let content =
        serde_yaml_ng::to_string(&value).expect("topic payload always serializes to yaml");
    atomic_write(path, content.as_bytes())
}

/// Load a singleton namespaced file as one sticky `Message`. Missing
/// file → empty vec. Parse errors and schema mismatches are logged and
/// skipped (returns empty).
pub fn load_namespaced_singleton(path: &Path, kind: TopicKind) -> Vec<Message> {
    let raw = match fs::read_to_string(path) {
        Ok(s) => s,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Vec::new(),
        Err(e) => {
            warn!(path = %path.display(), %e, "namespaced singleton read failed");
            return Vec::new();
        }
    };
    let value: Value = match serde_yaml_ng::from_str(&raw) {
        Ok(v) => v,
        Err(e) => {
            warn!(path = %path.display(), %e, "namespaced singleton parse failed");
            return Vec::new();
        }
    };
    let Some(topic) = Topic::from_yaml_section(kind, value) else {
        warn!(path = %path.display(), "namespaced singleton schema mismatch");
        return Vec::new();
    };
    let mut msg = topic.to_message();
    msg.sticky = true;
    msg.source = BUS_SOURCE.to_string();
    info!(path = %path.display(), "restored namespaced singleton");
    vec![msg]
}

/// Walk a directory and load every `*.yaml` file as one keyed sticky
/// `Message` of the given topic kind. Missing dir → empty vec.
pub fn load_namespaced_keyed(dir: &Path, kind: TopicKind) -> Vec<Message> {
    let entries = match fs::read_dir(dir) {
        Ok(e) => e,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Vec::new(),
        Err(e) => {
            warn!(dir = %dir.display(), %e, "namespaced keyed dir read failed");
            return Vec::new();
        }
    };
    let mut out = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|s| s.to_str()) != Some("yaml") {
            continue;
        }
        let raw = match fs::read_to_string(&path) {
            Ok(s) => s,
            Err(e) => {
                warn!(path = %path.display(), %e, "namespaced keyed read failed");
                continue;
            }
        };
        let value: Value = match serde_yaml_ng::from_str(&raw) {
            Ok(v) => v,
            Err(e) => {
                warn!(path = %path.display(), %e, "namespaced keyed parse failed");
                continue;
            }
        };
        let Some(topic) = Topic::from_yaml_section(kind, value) else {
            warn!(path = %path.display(), "namespaced keyed schema mismatch");
            continue;
        };
        let mut msg = topic.to_message();
        msg.sticky = true;
        msg.source = BUS_SOURCE.to_string();
        info!(path = %path.display(), keys = ?msg.keys, "restored namespaced keyed");
        out.push(msg);
    }
    out
}

/// Unlink a namespaced topic's file. Missing file is not an error
/// (idempotent retract).
pub fn retract_namespaced(path: &Path) -> io::Result<()> {
    match fs::remove_file(path) {
        Ok(_) => Ok(()),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e),
    }
}

/// Iterate every persistent `TopicKind` and load any that declares a
/// `namespace`. Singleton namespaces yield at most one message;
/// keyed namespaces yield one per file in the parent directory.
///
/// Currently supports keyed namespaces with a single trailing
/// `:placeholder` (e.g. `"browser/tabs/:id"`). Multi-placeholder
/// templates are valid for `path_for` resolution but not yet for
/// directory walking — they're treated as singletons.
pub fn load_namespaced_all(cfg_dir: &Path) -> Vec<Message> {
    let mut out = Vec::new();
    for kind in TopicKind::ALL.iter().copied() {
        if !kind.behavior().is_persistent() {
            continue;
        }
        let Some(template) = kind.namespace() else {
            continue;
        };
        if kind.has_keys() {
            // Walk the parent of the template's first `:placeholder`.
            let parent_template = template
                .split_once(':')
                .map(|(a, _)| a.trim_end_matches('/'))
                .unwrap_or(template);
            let parent = cfg_dir.join(parent_template);
            out.extend(load_namespaced_keyed(&parent, kind));
        } else {
            let path = cfg_dir.join(format!("{template}.yaml"));
            out.extend(load_namespaced_singleton(&path, kind));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn load_missing_file_is_empty() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("state.yaml");
        assert!(load(&path).is_empty());
    }

    #[test]
    fn load_invalid_yaml_is_empty() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("state.yaml");
        fs::write(&path, "not: [ valid: yaml\n: -").unwrap();
        assert!(load(&path).is_empty());
    }

    #[test]
    fn load_skips_unknown_sections() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("state.yaml");
        fs::write(&path, "NotARealTopic:\n  foo: bar\n").unwrap();
        assert!(load(&path).is_empty());
    }

    #[test]
    fn load_skips_non_persistent_sections() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("state.yaml");
        // Windows is sticky, not persistent — must be skipped.
        fs::write(&path, "Windows:\n  anything: here\n").unwrap();
        assert!(load(&path).is_empty());
    }

    #[test]
    fn atomic_write_leaves_no_tmp_file() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("state.yaml");
        atomic_write(&path, b"hello: 1\n").unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), "hello: 1\n");
        assert!(!path.with_extension("yaml.tmp").exists());
    }

    #[test]
    fn atomic_write_creates_parent_dir() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("nested/dir/state.yaml");
        atomic_write(&path, b"ok: true\n").unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), "ok: true\n");
    }

    #[test]
    fn write_section_noop_on_non_persistent_topic() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("state.yaml");
        // Shutdown is ephemeral; nothing should be written.
        write_section(&path, &Topic::Shutdown).unwrap();
        assert!(!path.exists());
    }

    #[test]
    fn zone_layout_round_trips_through_disk() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("state.yaml");

        let mut layout = crate::topics::ZoneLayout::default();
        layout.rows = 2;

        write_section(&path, &Topic::ZoneLayout(layout.clone())).unwrap();
        assert!(path.exists(), "write_section should create the file");

        let restored = load(&path);
        assert_eq!(restored.len(), 1, "exactly one sticky expected");
        let msg = &restored[0];
        assert_eq!(msg.topic, "ZoneLayout");
        assert!(msg.sticky);
        assert_eq!(msg.source, BUS_SOURCE);

        match Topic::parse(msg) {
            Some(Topic::ZoneLayout(back)) => assert_eq!(back, layout),
            other => panic!("expected ZoneLayout, got {other:?}"),
        }
    }

    #[test]
    fn retired_sections_are_skipped_and_the_rest_load() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("state.yaml");
        write_section(&path, &Topic::ZoneLayout(crate::topics::ZoneLayout::default())).unwrap();
        let mut raw = fs::read_to_string(&path).unwrap();
        for name in RETIRED {
            raw.push_str(&format!("{name}:\n  anything: here\n"));
        }
        raw.push_str("NotARealTopic:\n  foo: bar\n");
        fs::write(&path, raw).unwrap();
        let restored = load(&path);
        assert_eq!(restored.len(), 1);
        assert_eq!(restored[0].topic, "ZoneLayout");
    }

    #[test]
    fn a_legacy_section_reads_back_as_its_payload() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("state.yaml");
        let mut layout = crate::topics::ZoneLayout::default();
        layout.columns = 4;
        write_section(&path, &Topic::ZoneLayout(layout.clone())).unwrap();
        let back: Option<crate::topics::ZoneLayout> = legacy_section_at(&path, "ZoneLayout");
        assert_eq!(back, Some(layout));
        assert_eq!(legacy_section_at::<crate::topics::ZoneLayout>(&path, "Missing"), None);
    }

    #[test]
    fn retired_names_are_not_kinds() {
        for name in RETIRED {
            assert!(TopicKind::from_str(name).is_none(), "{name} is a kind again");
        }
    }

    #[test]
    fn write_section_preserves_unrelated_sections() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("state.yaml");
        // Seed the file with a hand-edit that isn't a real topic; the
        // bus should skip it on load but must not clobber it on write.
        fs::write(&path, "ManualNotes:\n  key: value\n").unwrap();

        write_section(&path, &Topic::ZoneLayout(crate::topics::ZoneLayout::default())).unwrap();

        let raw = fs::read_to_string(&path).unwrap();
        assert!(raw.contains("ManualNotes:"), "unrelated section lost");
        assert!(raw.contains("ZoneLayout:"), "new section missing");
    }

    #[test]
    fn retract_unkeyed_drops_section_entirely() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("state.yaml");

        write_section(&path, &Topic::ZoneLayout(crate::topics::ZoneLayout::default())).unwrap();
        assert!(path.exists());

        // Construct a synthetic ZoneLayout retract message.
        let mut msg = Topic::ZoneLayout(crate::topics::ZoneLayout::default()).to_message();
        msg.sticky = false;
        retract_section(&path, &msg).unwrap();

        let raw = fs::read_to_string(&path).unwrap();
        assert!(!raw.contains("ZoneLayout:"), "ZoneLayout section should be gone");
    }

    #[test]
    fn retract_on_missing_file_is_noop() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("state.yaml");
        let msg = Topic::ZoneLayout(crate::topics::ZoneLayout::default()).to_message();
        retract_section(&path, &msg).unwrap();
        assert!(!path.exists(), "retract should not create the file");
    }

    // ---- Namespaced storage tests --------------------------------------

    #[test]
    fn write_namespaced_singleton_has_no_section_header() {
        use crate::topics::{AgendaDay, CalendarAgenda};
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("agenda.yaml");
        let t = Topic::CalendarAgenda(CalendarAgenda {
            days: vec![AgendaDay::default()],
        });
        write_namespaced(&path, &t).unwrap();
        let raw = fs::read_to_string(&path).unwrap();
        assert!(
            !raw.contains("CalendarAgenda:"),
            "no section header expected"
        );
        assert!(raw.contains("days:"));
    }

    #[test]
    fn load_namespaced_singleton_yields_one_sticky() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("agenda.yaml");
        fs::write(&path, "days:\n- {}\n").unwrap();
        let msgs = load_namespaced_singleton(&path, TopicKind::CalendarAgenda);
        assert_eq!(msgs.len(), 1);
        match Topic::parse(&msgs[0]).unwrap() {
            Topic::CalendarAgenda(c) => assert_eq!(c.days.len(), 1),
            other => panic!("expected CalendarAgenda, got {other:?}"),
        }
        assert_eq!(msgs[0].source, BUS_SOURCE);
        assert!(msgs[0].sticky);
    }

    #[test]
    fn load_namespaced_singleton_missing_file_is_empty() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("absent.yaml");
        let msgs = load_namespaced_singleton(&path, TopicKind::CalendarAgenda);
        assert!(msgs.is_empty());
    }

    // No live kind is both keyed and namespaced, so the keyed walk is
    // tested on FloatGeometry (keyed by `app_id`), which is a kind with
    // keys; the walk does not look at the namespace.

    #[test]
    fn load_namespaced_keyed_walks_directory() {
        let tmp = TempDir::new().unwrap();
        let dir = tmp.path().join("float/geometry");
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("a.yaml"), "app_id: a\nx: 1\ny: 2\nwidth: 3\nheight: 4\n").unwrap();
        fs::write(dir.join("b.yaml"), "app_id: b\nx: 5\ny: 6\nwidth: 7\nheight: 8\n").unwrap();
        // A non-yaml file in the dir should be ignored.
        fs::write(dir.join("readme.txt"), "not a record").unwrap();

        let msgs = load_namespaced_keyed(&dir, TopicKind::FloatGeometry);
        assert_eq!(msgs.len(), 2);
        let mut keys: Vec<String> = msgs.iter().flat_map(|m| m.keys.clone()).collect();
        keys.sort();
        assert_eq!(keys, vec!["a", "b"]);
        for msg in &msgs {
            assert_eq!(msg.source, BUS_SOURCE);
            assert!(msg.sticky);
        }
    }

    #[test]
    fn load_namespaced_keyed_missing_dir_is_empty() {
        let tmp = TempDir::new().unwrap();
        let dir = tmp.path().join("nonexistent");
        let msgs = load_namespaced_keyed(&dir, TopicKind::FloatGeometry);
        assert!(msgs.is_empty());
    }

    #[test]
    fn retract_namespaced_unlinks_file() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("a.yaml");
        fs::write(&path, "x: 1\n").unwrap();
        retract_namespaced(&path).unwrap();
        assert!(!path.exists());
    }

    #[test]
    fn retract_namespaced_missing_is_ok() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("absent.yaml");
        retract_namespaced(&path).unwrap();
    }

    #[test]
    fn write_then_load_namespaced_keyed_round_trips() {
        use crate::topics::FloatGeometry;
        let tmp = TempDir::new().unwrap();
        let dir = tmp.path().join("float/geometry");

        let a = FloatGeometry { app_id: "a".into(), x: 1, y: 2, width: 3, height: 4 };
        write_namespaced(&dir.join("a.yaml"), &Topic::FloatGeometry(a.clone())).unwrap();

        let msgs = load_namespaced_keyed(&dir, TopicKind::FloatGeometry);
        assert_eq!(msgs.len(), 1);
        match Topic::parse(&msgs[0]).unwrap() {
            Topic::FloatGeometry(back) => assert_eq!(back, a),
            other => panic!("expected FloatGeometry, got {other:?}"),
        }
    }
}
