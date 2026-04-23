//! Centralized config store: TOML file ↔ flat key-value bus representation.
//!
//! The store owns a `toml::Value::Table` tree, persists it as a TOML file,
//! and exposes it as a flat `Vec<(String, ConfigValue)>` for the bus.
//! Mutations are applied to the tree with validation, then flattened.

use std::path::{Path, PathBuf};

use serde::de::DeserializeOwned;
use serde::Serialize;

use super::{ConfigValue, MutateOp};
use tracing::{info, warn};

/// In-memory config store backed by a TOML file.
pub struct ConfigStore {
    path: PathBuf,
    tree: toml::Value,
}

/// Errors from mutation validation.
#[derive(Debug)]
pub enum MutateError {
    /// Key path is empty or malformed.
    InvalidKey(String),
    /// A Set/Append/Insert/Replace targeted a path whose parent isn't a table.
    NotATable { key: String, segment: String },
    /// An array op targeted a path that isn't an array.
    NotAnArray(String),
    /// Array index out of bounds.
    IndexOutOfBounds { key: String, index: u32, len: usize },
}

impl std::fmt::Display for MutateError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidKey(k) => write!(f, "invalid key: {k:?}"),
            Self::NotATable { key, segment } => {
                write!(f, "{segment:?} in {key:?} is not a table")
            }
            Self::NotAnArray(k) => write!(f, "{k:?} is not an array"),
            Self::IndexOutOfBounds { key, index, len } => {
                write!(f, "index {index} out of bounds for {key:?} (len {len})")
            }
        }
    }
}

impl ConfigStore {
    /// Load from disk, or start with an empty table if the file doesn't exist.
    pub fn load(path: impl Into<PathBuf>) -> Self {
        let path = path.into();
        let tree = match std::fs::read_to_string(&path) {
            Ok(raw) => match raw.parse::<toml::Value>() {
                Ok(v @ toml::Value::Table(_)) => {
                    info!(path = %path.display(), "loaded config");
                    v
                }
                Ok(_) => {
                    warn!(path = %path.display(), "config root is not a table, starting empty");
                    toml::Value::Table(toml::map::Map::new())
                }
                Err(e) => {
                    warn!(path = %path.display(), %e, "failed to parse config, starting empty");
                    toml::Value::Table(toml::map::Map::new())
                }
            },
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                info!(path = %path.display(), "no config file, starting empty");
                toml::Value::Table(toml::map::Map::new())
            }
            Err(e) => {
                warn!(path = %path.display(), %e, "failed to read config, starting empty");
                toml::Value::Table(toml::map::Map::new())
            }
        };
        Self { path, tree }
    }

    /// Persist the current tree to disk (atomic write via temp+rename).
    pub fn save(&self) {
        let content = toml::to_string_pretty(&self.tree).unwrap_or_default();
        if let Err(e) = atomic_write(&self.path, content.as_bytes()) {
            warn!(path = %self.path.display(), %e, "failed to save config");
        }
    }

    /// Flatten the tree into dotted-key / leaf-value pairs for the bus.
    pub fn flatten(&self) -> Vec<(String, ConfigValue)> {
        let mut out = Vec::new();
        flatten_value(&self.tree, &mut String::new(), &mut out);
        out
    }

    /// Deserialize a subtree into a typed config struct.
    ///
    /// `prefix` is a dotted key path (e.g. `"mail"`). Returns `None` if
    /// the subtree doesn't exist or deserialization fails.
    pub fn get_as<T: DeserializeOwned>(&self, prefix: &str) -> Option<T> {
        let node = self.navigate(prefix)?;
        node.clone().try_into().ok()
    }

    /// Serialize a typed config struct and merge it into the tree at `prefix`.
    ///
    /// Overwrites any existing value at that path.
    pub fn set_from<T: Serialize>(&mut self, prefix: &str, value: &T) {
        let toml_val = toml::Value::try_from(value).expect("config type must serialize to TOML");
        let segments: Vec<&str> = prefix.split('.').collect();
        if segments.is_empty() {
            return;
        }
        // Ensure parent tables exist, then set the leaf.
        if let Ok(parent) = self.ensure_tables(&segments) {
            let leaf = segments.last().unwrap();
            if let toml::Value::Table(map) = parent {
                map.insert((*leaf).to_string(), toml_val);
            }
        }
    }

    /// Read-only navigation to an existing node by dotted key path.
    fn navigate(&self, key: &str) -> Option<&toml::Value> {
        let mut node = &self.tree;
        for seg in key.split('.') {
            match node {
                toml::Value::Table(map) => node = map.get(seg)?,
                _ => return None,
            }
        }
        Some(node)
    }

    /// Apply a mutation. Returns `Ok(())` on success (caller should then
    /// `save()` and emit the new `Config`). Returns `Err` if validation fails.
    pub fn mutate(&mut self, key: &str, op: MutateOp) -> Result<(), MutateError> {
        let segments: Vec<&str> = key.split('.').collect();
        if segments.is_empty() || segments.iter().any(|s| s.is_empty()) {
            return Err(MutateError::InvalidKey(key.into()));
        }

        match op {
            MutateOp::Set(value) => self.apply_set(&segments, value),
            MutateOp::Delete => {
                self.apply_delete(&segments);
                Ok(())
            }
            MutateOp::Append(value) => self.apply_array_op(key, &segments, ArrayOp::Append(value)),
            MutateOp::Insert { index, value } => {
                self.apply_array_op(key, &segments, ArrayOp::Insert(index, value))
            }
            MutateOp::Remove { index } => {
                self.apply_array_op(key, &segments, ArrayOp::Remove(index))
            }
            MutateOp::Replace { index, value } => {
                self.apply_array_op(key, &segments, ArrayOp::Replace(index, value))
            }
        }
    }

    fn apply_set(
        &mut self,
        segments: &[&str],
        value: ConfigValue,
    ) -> Result<(), MutateError> {
        let parent = self.ensure_tables(segments)?;
        let leaf = segments.last().unwrap();
        if let toml::Value::Table(map) = parent {
            map.insert((*leaf).to_string(), value.to_toml());
        }
        Ok(())
    }

    fn apply_delete(&mut self, segments: &[&str]) {
        if segments.len() == 1 {
            if let toml::Value::Table(map) = &mut self.tree {
                map.remove(segments[0]);
            }
            return;
        }
        // Navigate to parent, remove the last segment.
        let (parent_segs, leaf) = segments.split_at(segments.len() - 1);
        let mut node = &mut self.tree;
        for seg in parent_segs {
            match node {
                toml::Value::Table(map) => match map.get_mut(*seg) {
                    Some(child) => node = child,
                    None => return,
                },
                _ => return,
            }
        }
        if let toml::Value::Table(map) = node {
            map.remove(leaf[0]);
        }
    }

    fn apply_array_op(
        &mut self,
        key: &str,
        segments: &[&str],
        op: ArrayOp,
    ) -> Result<(), MutateError> {
        let node = self.navigate_mut(segments).ok_or(MutateError::NotAnArray(key.into()))?;
        let arr = match node {
            toml::Value::Array(a) => a,
            _ => return Err(MutateError::NotAnArray(key.into())),
        };
        let len = arr.len();
        match op {
            ArrayOp::Append(value) => {
                arr.push(value.to_toml());
            }
            ArrayOp::Insert(index, value) => {
                let i = index as usize;
                if i > len {
                    return Err(MutateError::IndexOutOfBounds { key: key.into(), index, len });
                }
                arr.insert(i, value.to_toml());
            }
            ArrayOp::Remove(index) => {
                let i = index as usize;
                if i >= len {
                    return Err(MutateError::IndexOutOfBounds { key: key.into(), index, len });
                }
                arr.remove(i);
            }
            ArrayOp::Replace(index, value) => {
                let i = index as usize;
                if i >= len {
                    return Err(MutateError::IndexOutOfBounds { key: key.into(), index, len });
                }
                arr[i] = value.to_toml();
            }
        }
        Ok(())
    }

    /// Walk the tree to the parent of the leaf segment, creating
    /// intermediate tables as needed. Returns a mutable ref to the
    /// parent table (the one that will contain the leaf key).
    fn ensure_tables<'a>(
        &'a mut self,
        segments: &[&str],
    ) -> Result<&'a mut toml::Value, MutateError> {
        let key = segments.join(".");
        let parent_segs = &segments[..segments.len() - 1];
        let mut node = &mut self.tree;
        for seg in parent_segs {
            match node {
                toml::Value::Table(map) => {
                    node = map
                        .entry(*seg)
                        .or_insert_with(|| toml::Value::Table(toml::map::Map::new()));
                    if !node.is_table() {
                        return Err(MutateError::NotATable {
                            key,
                            segment: (*seg).to_string(),
                        });
                    }
                }
                _ => {
                    return Err(MutateError::NotATable {
                        key,
                        segment: (*seg).to_string(),
                    });
                }
            }
        }
        Ok(node)
    }

    /// Navigate to an existing node. Returns `None` if any segment is missing.
    fn navigate_mut(&mut self, segments: &[&str]) -> Option<&mut toml::Value> {
        let mut node = &mut self.tree;
        for seg in segments {
            match node {
                toml::Value::Table(map) => {
                    node = map.get_mut(*seg)?;
                }
                _ => return None,
            }
        }
        Some(node)
    }
}

enum ArrayOp {
    Append(ConfigValue),
    Insert(u32, ConfigValue),
    Remove(u32),
    Replace(u32, ConfigValue),
}

/// Reconstruct a typed config from a flat bus snapshot.
///
/// Filters entries by `prefix`, rebuilds a `toml::Value::Table`, and
/// deserializes into `T`. Returns `None` if no entries match or
/// deserialization fails.
///
/// ```ignore
/// let mail: MailConfig = from_entries(&snapshot, "mail").unwrap_or_default();
/// ```
pub fn from_entries<T: DeserializeOwned>(
    entries: &[(String, ConfigValue)],
    prefix: &str,
) -> Option<T> {
    let dot_prefix = format!("{prefix}.");
    let mut table = toml::map::Map::new();
    let mut found = false;

    for (key, value) in entries {
        if let Some(rest) = key.strip_prefix(&dot_prefix) {
            insert_dotted(&mut table, rest, value.to_toml());
            found = true;
        } else if key == prefix {
            // The prefix itself is a value (e.g., a table serialized as one entry).
            let toml_val = value.to_toml();
            return toml_val.try_into().ok();
        }
    }

    if !found {
        return None;
    }
    toml::Value::Table(table).try_into().ok()
}

/// Insert a value into a TOML map at a dotted path, creating intermediate
/// tables as needed.
fn insert_dotted(map: &mut toml::map::Map<String, toml::Value>, key: &str, value: toml::Value) {
    let mut segments = key.splitn(2, '.');
    let head = segments.next().unwrap();
    match segments.next() {
        None => {
            map.insert(head.to_string(), value);
        }
        Some(rest) => {
            let child = map
                .entry(head)
                .or_insert_with(|| toml::Value::Table(toml::map::Map::new()));
            if let toml::Value::Table(child_map) = child {
                insert_dotted(child_map, rest, value);
            }
        }
    }
}

/// Recursively flatten a toml::Value into dotted key paths with leaf ConfigValues.
fn flatten_value(value: &toml::Value, prefix: &mut String, out: &mut Vec<(String, ConfigValue)>) {
    match value {
        toml::Value::Table(map) => {
            for (k, v) in map {
                let prev_len = prefix.len();
                if !prefix.is_empty() {
                    prefix.push('.');
                }
                prefix.push_str(k);
                flatten_value(v, prefix, out);
                prefix.truncate(prev_len);
            }
        }
        toml::Value::Array(arr) => {
            // Arrays are emitted as a single leaf so consumers see the
            // full structure (including arrays of tables).
            out.push((prefix.clone(), ConfigValue::from_toml(value)));
            let _ = arr; // suppress unused
        }
        _ => {
            out.push((prefix.clone(), ConfigValue::from_toml(value)));
        }
    }
}

fn atomic_write(path: &Path, content: &[u8]) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let tmp = path.with_extension("toml.tmp");
    std::fs::write(&tmp, content)?;
    std::fs::rename(&tmp, path)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn store_from_toml(toml_str: &str) -> ConfigStore {
        let tree: toml::Value = toml_str.parse().unwrap();
        ConfigStore {
            path: PathBuf::from("/dev/null"),
            tree,
        }
    }

    #[test]
    fn flatten_simple() {
        let store = store_from_toml(
            r#"
            [mail]
            host = "imap.example.com"
            port = 993
            "#,
        );
        let flat = store.flatten();
        assert!(flat.iter().any(|(k, v)| k == "mail.host"
            && *v == ConfigValue::String("imap.example.com".into())));
        assert!(flat
            .iter()
            .any(|(k, v)| k == "mail.port" && *v == ConfigValue::Int(993)));
    }

    #[test]
    fn flatten_array_as_single_entry() {
        let store = store_from_toml(
            r#"
            [[apps]]
            id = "firefox"
            [[apps]]
            id = "terminal"
            "#,
        );
        let flat = store.flatten();
        assert_eq!(flat.len(), 1);
        assert_eq!(flat[0].0, "apps");
        assert!(matches!(flat[0].1, ConfigValue::Array(_)));
    }

    #[test]
    fn set_creates_intermediate_tables() {
        let mut store = store_from_toml("");
        store
            .mutate("a.b.c", MutateOp::Set(ConfigValue::Int(42)))
            .unwrap();
        let flat = store.flatten();
        assert!(flat.iter().any(|(k, v)| k == "a.b.c" && *v == ConfigValue::Int(42)));
    }

    #[test]
    fn set_overwrites_existing() {
        let mut store = store_from_toml("[mail]\nport = 143");
        store
            .mutate("mail.port", MutateOp::Set(ConfigValue::Int(993)))
            .unwrap();
        let flat = store.flatten();
        assert!(flat.iter().any(|(k, v)| k == "mail.port" && *v == ConfigValue::Int(993)));
    }

    #[test]
    fn delete_removes_key() {
        let mut store = store_from_toml("[mail]\nhost = \"x\"\nport = 993");
        store.mutate("mail.host", MutateOp::Delete).unwrap();
        let flat = store.flatten();
        assert!(!flat.iter().any(|(k, _)| k == "mail.host"));
        assert!(flat.iter().any(|(k, _)| k == "mail.port"));
    }

    #[test]
    fn delete_removes_subtree() {
        let mut store = store_from_toml("[mail]\nhost = \"x\"\nport = 993");
        store.mutate("mail", MutateOp::Delete).unwrap();
        assert!(store.flatten().is_empty());
    }

    #[test]
    fn append_to_array() {
        let mut store = store_from_toml("items = [1, 2]");
        store
            .mutate("items", MutateOp::Append(ConfigValue::Int(3)))
            .unwrap();
        let flat = store.flatten();
        let arr = &flat[0].1;
        assert_eq!(
            *arr,
            ConfigValue::Array(vec![ConfigValue::Int(1), ConfigValue::Int(2), ConfigValue::Int(3)])
        );
    }

    #[test]
    fn insert_at_index() {
        let mut store = store_from_toml("items = [1, 3]");
        store
            .mutate(
                "items",
                MutateOp::Insert {
                    index: 1,
                    value: ConfigValue::Int(2),
                },
            )
            .unwrap();
        let flat = store.flatten();
        assert_eq!(
            flat[0].1,
            ConfigValue::Array(vec![ConfigValue::Int(1), ConfigValue::Int(2), ConfigValue::Int(3)])
        );
    }

    #[test]
    fn insert_out_of_bounds() {
        let mut store = store_from_toml("items = [1]");
        let err = store
            .mutate(
                "items",
                MutateOp::Insert {
                    index: 5,
                    value: ConfigValue::Int(2),
                },
            )
            .unwrap_err();
        assert!(matches!(err, MutateError::IndexOutOfBounds { .. }));
    }

    #[test]
    fn remove_shifts_elements() {
        let mut store = store_from_toml("items = [1, 2, 3]");
        store
            .mutate("items", MutateOp::Remove { index: 1 })
            .unwrap();
        let flat = store.flatten();
        assert_eq!(
            flat[0].1,
            ConfigValue::Array(vec![ConfigValue::Int(1), ConfigValue::Int(3)])
        );
    }

    #[test]
    fn replace_at_index() {
        let mut store = store_from_toml("items = [1, 2, 3]");
        store
            .mutate(
                "items",
                MutateOp::Replace {
                    index: 1,
                    value: ConfigValue::Int(99),
                },
            )
            .unwrap();
        let flat = store.flatten();
        assert_eq!(
            flat[0].1,
            ConfigValue::Array(vec![ConfigValue::Int(1), ConfigValue::Int(99), ConfigValue::Int(3)])
        );
    }

    #[test]
    fn array_op_on_non_array_fails() {
        let mut store = store_from_toml("[mail]\nport = 993");
        let err = store
            .mutate("mail.port", MutateOp::Append(ConfigValue::Int(1)))
            .unwrap_err();
        assert!(matches!(err, MutateError::NotAnArray(_)));
    }

    #[test]
    fn set_through_non_table_fails() {
        let mut store = store_from_toml("[mail]\nhost = \"x\"");
        let err = store
            .mutate("mail.host.sub", MutateOp::Set(ConfigValue::Int(1)))
            .unwrap_err();
        assert!(matches!(err, MutateError::NotATable { .. }));
    }

    #[test]
    fn append_table_to_array_of_tables() {
        let mut store = store_from_toml(
            r#"
            [[shell.applications]]
            app_id = "firefox"
            command = "firefox"
            "#,
        );
        let new_app = ConfigValue::Table(vec![
            ("app_id".into(), ConfigValue::String("terminal".into())),
            ("command".into(), ConfigValue::String("sola-terminal".into())),
        ]);
        store
            .mutate("shell.applications", MutateOp::Append(new_app))
            .unwrap();
        let flat = store.flatten();
        let apps = flat.iter().find(|(k, _)| k == "shell.applications").unwrap();
        match &apps.1 {
            ConfigValue::Array(arr) => assert_eq!(arr.len(), 2),
            other => panic!("expected array, got {other:?}"),
        }
    }

    #[test]
    fn get_as_typed() {
        #[derive(Debug, serde::Deserialize, PartialEq)]
        struct Mail {
            host: String,
            port: i64,
        }
        let store = store_from_toml("[mail]\nhost = \"imap.x.com\"\nport = 993");
        let mail: Mail = store.get_as("mail").unwrap();
        assert_eq!(mail.host, "imap.x.com");
        assert_eq!(mail.port, 993);
    }

    #[test]
    fn set_from_typed() {
        #[derive(Debug, serde::Serialize)]
        struct Mail {
            host: String,
            port: u16,
        }
        let mut store = store_from_toml("");
        store.set_from("mail", &Mail { host: "smtp.x.com".into(), port: 587 });
        let flat = store.flatten();
        assert!(flat.iter().any(|(k, v)| k == "mail.host"
            && *v == ConfigValue::String("smtp.x.com".into())));
        assert!(flat.iter().any(|(k, v)| k == "mail.port" && *v == ConfigValue::Int(587)));
    }

    #[test]
    fn from_entries_roundtrip() {
        #[derive(Debug, serde::Serialize, serde::Deserialize, PartialEq)]
        struct Mail {
            host: String,
            port: u16,
        }
        let mut store = store_from_toml("");
        store.set_from("mail", &Mail { host: "imap.x.com".into(), port: 993 });
        let flat = store.flatten();
        let mail: Mail = from_entries(&flat, "mail").unwrap();
        assert_eq!(mail, Mail { host: "imap.x.com".into(), port: 993 });
    }

    #[test]
    fn from_entries_missing_prefix() {
        let flat = vec![("other.key".into(), ConfigValue::Int(1))];
        let result: Option<toml::Value> = from_entries(&flat, "mail");
        assert!(result.is_none());
    }
}
