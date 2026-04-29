//! Persistent sticky storage for the bus host.
//!
//! Persistent topics live in one of two layouts on disk:
//!
//! - **Default (no `namespace` annotation):** a single shared file
//!   `~/.config/sola/state.toml` with one `[SectionName]` per topic
//!   kind (or `[[SectionName]]` array entries for keyed topics).
//! - **Namespaced (`#[persistent(namespace = "...")]`):** a dedicated
//!   file per topic, named by the resolved namespace template. The
//!   payload is the entire file content; no section header.
//!
//! On bus startup the host reads both layouts and replays each topic
//! as a sticky message keyed by `source = "sola-bus"`. Subsequent emits
//! and retracts route by `Topic::path_for()`: namespaced topics use the
//! namespaced helpers; the rest go through the shared state.toml path.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use tracing::{info, warn};

use crate::Message;
use crate::topics::{Topic, TopicKind};

/// `Message::source` used for sticky entries restored from disk. A
/// client emit overrides this the first time it replaces the value.
pub const BUS_SOURCE: &str = "sola-bus";

/// `~/.config/sola/state.toml`.
pub fn state_path() -> PathBuf {
    sola_core::config::sola_config_dir().join("state.toml")
}

/// Read state.toml and return one sticky `Message` per valid
/// persistent section. Missing file → empty vec. Parse errors,
/// unknown sections, non-persistent sections, and schema mismatches
/// are logged and skipped.
pub fn load(path: &Path) -> Vec<Message> {
    let raw = match fs::read_to_string(path) {
        Ok(s) => s,
        Err(e) if e.kind() == io::ErrorKind::NotFound => {
            info!(path = %path.display(), "no state.toml yet");
            return Vec::new();
        }
        Err(e) => {
            warn!(path = %path.display(), %e, "state.toml read failed");
            return Vec::new();
        }
    };

    let table: toml::Table = match raw.parse() {
        Ok(t) => t,
        Err(e) => {
            warn!(path = %path.display(), %e, "state.toml parse failed; starting empty");
            return Vec::new();
        }
    };

    let mut out = Vec::new();
    for (section, value) in table {
        let Some(kind) = TopicKind::from_str(&section) else {
            warn!(section = %section, "unknown persistent topic; skipping");
            continue;
        };
        if !kind.behavior().is_persistent() {
            warn!(section = %section, "section is not a persistent topic; skipping");
            continue;
        }

        if kind.has_keys() {
            // Expect an array of tables: `[[Section]]`. Each entry is a
            // separate record with its own keys.
            let toml::Value::Array(entries) = value else {
                warn!(section = %section, "keyed topic expects array of tables; skipping");
                continue;
            };
            for entry in entries {
                let Some(topic) = Topic::from_toml_section(kind, entry) else {
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
            let Some(topic) = Topic::from_toml_section(kind, value) else {
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

/// Update a single section in state.toml with `topic`'s current
/// value. No-ops for non-persistent topics. Load → replace →
/// `temp + rename` atomic write so a crash mid-write can't leave a
/// torn file.
pub fn write_section(path: &Path, topic: &Topic) -> io::Result<()> {
    let Some(value) = topic.to_toml_value() else {
        return Ok(());
    };
    let kind = topic.kind();
    let section = kind.as_str().to_string();

    let mut table = match fs::read_to_string(path) {
        Ok(s) => s.parse::<toml::Table>().unwrap_or_else(|e| {
            warn!(path = %path.display(), %e, "state.toml parse failed during write; rewriting from empty");
            toml::Table::new()
        }),
        Err(e) if e.kind() == io::ErrorKind::NotFound => toml::Table::new(),
        Err(e) => return Err(e),
    };

    if kind.has_keys() {
        // Keyed: upsert into `[[Section]]` array by key match.
        let keys = topic.keys_for();
        let existing = table.remove(&section);
        let mut entries = match existing {
            Some(toml::Value::Array(arr)) => arr,
            _ => Vec::new(),
        };
        // Remove any entry whose key fields match.
        entries.retain(|entry| !entry_matches_keys(kind, entry, &keys));
        entries.push(value);
        table.insert(section, toml::Value::Array(entries));
    } else {
        table.insert(section, value);
    }

    let content = toml::to_string_pretty(&table).expect("top-level toml table always serializes");
    atomic_write(path, content.as_bytes())
}

/// Remove the matching record from the persistent topic's section in
/// state.toml. For keyed kinds, removes the entry whose key fields
/// match `event.keys`; if it was the last entry, drops the section.
/// For unkeyed kinds, drops the section.
pub fn retract_section(path: &Path, event: &Message) -> io::Result<()> {
    let Some(kind) = TopicKind::from_str(&event.topic) else {
        return Ok(());
    };
    if !kind.behavior().is_persistent() {
        return Ok(());
    }

    let mut table = match fs::read_to_string(path) {
        Ok(s) => s.parse::<toml::Table>().unwrap_or_else(|e| {
            warn!(path = %path.display(), %e, "state.toml parse failed during retract; rewriting from empty");
            toml::Table::new()
        }),
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(e),
    };

    let section = kind.as_str().to_string();

    if kind.has_keys() {
        if let Some(toml::Value::Array(mut entries)) = table.remove(&section) {
            entries.retain(|entry| !entry_matches_keys(kind, entry, &event.keys));
            if !entries.is_empty() {
                table.insert(section, toml::Value::Array(entries));
            }
        }
    } else {
        table.remove(&section);
    }

    let content = toml::to_string_pretty(&table).expect("top-level toml table always serializes");
    atomic_write(path, content.as_bytes())
}

/// True if the TOML entry's serialized payload, when parsed back through
/// the topic's `from_toml_section`, would produce a `Topic` whose
/// `keys_for()` matches `keys`. We do the round-trip rather than
/// inspecting raw TOML so the key-extraction logic stays in one place
/// (the macro-generated `keys_for`).
fn entry_matches_keys(kind: TopicKind, entry: &toml::Value, keys: &[String]) -> bool {
    let Some(topic) = Topic::from_toml_section(kind, entry.clone()) else {
        return false;
    };
    topic.keys_for() == keys
}

fn atomic_write(path: &Path, content: &[u8]) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let tmp = path.with_extension("toml.tmp");
    fs::write(&tmp, content)?;
    fs::rename(&tmp, path)?;
    Ok(())
}

// ---- Namespaced storage (per-topic dedicated files) ----------------------
//
// Topics annotated with `#[persistent(namespace = "...")]` live in their
// own file (no `[Section]` header — the kind is implied by the path).
// Keyed namespaces interpolate `:placeholder` segments to a per-key file.

/// Write a namespaced topic's payload as the whole file's content.
/// No-op for non-persistent topics or payloads that can't serialize to
/// TOML.
pub fn write_namespaced(path: &Path, topic: &Topic) -> io::Result<()> {
    let Some(value) = topic.to_toml_value() else {
        return Ok(());
    };
    let content = match value {
        toml::Value::Table(t) => toml::to_string_pretty(&t)
            .expect("topic payload table always serializes"),
        // Non-table payloads (rare for persistent topics) get wrapped
        // so the file remains valid TOML.
        other => format!("value = {}\n", other),
    };
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
    let value: toml::Value = match raw.parse::<toml::Table>() {
        Ok(t) => toml::Value::Table(t),
        Err(e) => {
            warn!(path = %path.display(), %e, "namespaced singleton parse failed");
            return Vec::new();
        }
    };
    let Some(topic) = Topic::from_toml_section(kind, value) else {
        warn!(path = %path.display(), "namespaced singleton schema mismatch");
        return Vec::new();
    };
    let mut msg = topic.to_message();
    msg.sticky = true;
    msg.source = BUS_SOURCE.to_string();
    info!(path = %path.display(), "restored namespaced singleton");
    vec![msg]
}

/// Walk a directory and load every `*.toml` file as one keyed sticky
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
        if path.extension().and_then(|s| s.to_str()) != Some("toml") {
            continue;
        }
        let raw = match fs::read_to_string(&path) {
            Ok(s) => s,
            Err(e) => {
                warn!(path = %path.display(), %e, "namespaced keyed read failed");
                continue;
            }
        };
        let value: toml::Value = match raw.parse::<toml::Table>() {
            Ok(t) => toml::Value::Table(t),
            Err(e) => {
                warn!(path = %path.display(), %e, "namespaced keyed parse failed");
                continue;
            }
        };
        let Some(topic) = Topic::from_toml_section(kind, value) else {
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
            let path = cfg_dir.join(format!("{template}.toml"));
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
        let path = tmp.path().join("state.toml");
        assert!(load(&path).is_empty());
    }

    #[test]
    fn load_invalid_toml_is_empty() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("state.toml");
        fs::write(&path, "not [[ valid toml").unwrap();
        assert!(load(&path).is_empty());
    }

    #[test]
    fn load_skips_unknown_sections() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("state.toml");
        fs::write(
            &path,
            r#"
[NotARealTopic]
foo = "bar"
"#,
        )
        .unwrap();
        assert!(load(&path).is_empty());
    }

    #[test]
    fn load_skips_non_persistent_sections() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("state.toml");
        // Windows is sticky, not persistent — must be skipped.
        fs::write(
            &path,
            r#"
[Windows]
anything = "here"
"#,
        )
        .unwrap();
        assert!(load(&path).is_empty());
    }

    #[test]
    fn atomic_write_leaves_no_tmp_file() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("state.toml");
        atomic_write(&path, b"hello = 1\n").unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), "hello = 1\n");
        assert!(!path.with_extension("toml.tmp").exists());
    }

    #[test]
    fn atomic_write_creates_parent_dir() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("nested/dir/state.toml");
        atomic_write(&path, b"ok = true\n").unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), "ok = true\n");
    }

    #[test]
    fn write_section_noop_on_non_persistent_topic() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("state.toml");
        // Shutdown is ephemeral; nothing should be written.
        write_section(&path, &Topic::Shutdown).unwrap();
        assert!(!path.exists());
    }

    #[test]
    fn zones_round_trip_through_disk() {
        use crate::topics::Zone;
        use std::collections::HashMap;

        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("state.toml");

        let mut zones = HashMap::new();
        zones.insert("sola-browser".to_string(), Zone::Left);
        zones.insert("sola-terminal".to_string(), Zone::Right);

        write_section(&path, &Topic::Zones(zones.clone())).unwrap();
        assert!(path.exists(), "write_section should create the file");

        let restored = load(&path);
        assert_eq!(restored.len(), 1, "exactly one sticky expected");
        let msg = &restored[0];
        assert_eq!(msg.topic, "Zones");
        assert!(msg.sticky);
        assert_eq!(msg.source, BUS_SOURCE);

        match Topic::parse(msg) {
            Some(Topic::Zones(map)) => assert_eq!(map, zones),
            other => panic!("expected Zones, got {other:?}"),
        }
    }

    #[test]
    fn write_section_preserves_unrelated_sections() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("state.toml");
        // Seed the file with a hand-edit that isn't a real topic; the
        // bus should skip it on load but must not clobber it on write.
        fs::write(&path, "[ManualNotes]\nkey = \"value\"\n").unwrap();

        let zones = std::collections::HashMap::from([(
            "sola-browser".to_string(),
            crate::topics::Zone::Left,
        )]);
        write_section(&path, &Topic::Zones(zones)).unwrap();

        let raw = fs::read_to_string(&path).unwrap();
        assert!(raw.contains("[ManualNotes]"), "unrelated section lost");
        assert!(raw.contains("[Zones]"), "new section missing");
    }

    #[test]
    fn retract_unkeyed_drops_section_entirely() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("state.toml");

        let mut zones = std::collections::HashMap::new();
        zones.insert("sola-browser".to_string(), crate::topics::Zone::Left);
        write_section(&path, &Topic::Zones(zones)).unwrap();
        assert!(path.exists());

        // Construct a synthetic Zones retract message.
        let mut msg = Topic::Zones(std::collections::HashMap::new()).to_message();
        msg.sticky = false;
        retract_section(&path, &msg).unwrap();

        let raw = fs::read_to_string(&path).unwrap();
        assert!(!raw.contains("[Zones]"), "Zones section should be gone");
    }

    #[test]
    fn retract_on_missing_file_is_noop() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("state.toml");
        let msg = Topic::Zones(std::collections::HashMap::new()).to_message();
        retract_section(&path, &msg).unwrap();
        assert!(!path.exists(), "retract should not create the file");
    }

    // ---- Namespaced storage tests --------------------------------------

    #[test]
    fn write_namespaced_singleton_has_no_section_header() {
        use crate::topics::BrowserConfig;
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("browser.toml");
        let t = Topic::BrowserConfig(BrowserConfig {
            active_tab_id: Some("abc".into()),
        });
        write_namespaced(&path, &t).unwrap();
        let raw = fs::read_to_string(&path).unwrap();
        assert!(!raw.contains("[BrowserConfig]"), "no section header expected");
        assert!(raw.contains("active_tab_id = \"abc\""));
    }

    #[test]
    fn load_namespaced_singleton_yields_one_sticky() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("browser.toml");
        fs::write(&path, "active_tab_id = \"abc\"\n").unwrap();
        let msgs = load_namespaced_singleton(&path, TopicKind::BrowserConfig);
        assert_eq!(msgs.len(), 1);
        match Topic::parse(&msgs[0]).unwrap() {
            Topic::BrowserConfig(c) => assert_eq!(c.active_tab_id.as_deref(), Some("abc")),
            other => panic!("expected BrowserConfig, got {other:?}"),
        }
        assert_eq!(msgs[0].source, BUS_SOURCE);
        assert!(msgs[0].sticky);
    }

    #[test]
    fn load_namespaced_singleton_missing_file_is_empty() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("absent.toml");
        let msgs = load_namespaced_singleton(&path, TopicKind::BrowserConfig);
        assert!(msgs.is_empty());
    }

    #[test]
    fn load_namespaced_keyed_walks_directory() {
        let tmp = TempDir::new().unwrap();
        let dir = tmp.path().join("browser/tabs");
        fs::create_dir_all(&dir).unwrap();
        fs::write(
            dir.join("a.toml"),
            "id = \"a\"\nurl = \"https://a/\"\ntitle = \"A\"\nordinal = 0\n",
        )
        .unwrap();
        fs::write(
            dir.join("b.toml"),
            "id = \"b\"\nurl = \"https://b/\"\ntitle = \"B\"\nordinal = 1\n",
        )
        .unwrap();
        // A non-toml file in the dir should be ignored.
        fs::write(dir.join("readme.txt"), "not a tab").unwrap();

        let msgs = load_namespaced_keyed(&dir, TopicKind::BrowserTab);
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
        let msgs = load_namespaced_keyed(&dir, TopicKind::BrowserTab);
        assert!(msgs.is_empty());
    }

    #[test]
    fn retract_namespaced_unlinks_file() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("a.toml");
        fs::write(&path, "x = 1\n").unwrap();
        retract_namespaced(&path).unwrap();
        assert!(!path.exists());
    }

    #[test]
    fn retract_namespaced_missing_is_ok() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("absent.toml");
        retract_namespaced(&path).unwrap();
    }

    #[test]
    fn write_then_load_namespaced_keyed_round_trips() {
        use crate::topics::BrowserTab;
        let tmp = TempDir::new().unwrap();
        let dir = tmp.path().join("browser/tabs");

        let tab_a = BrowserTab {
            id: "a".into(),
            url: "https://a/".into(),
            title: "A".into(),
            ordinal: 0,
            session_state: None,
        };
        write_namespaced(&dir.join("a.toml"), &Topic::BrowserTab(tab_a.clone())).unwrap();

        let msgs = load_namespaced_keyed(&dir, TopicKind::BrowserTab);
        assert_eq!(msgs.len(), 1);
        match Topic::parse(&msgs[0]).unwrap() {
            Topic::BrowserTab(tab) => assert_eq!(tab, tab_a),
            other => panic!("expected BrowserTab, got {other:?}"),
        }
    }
}
