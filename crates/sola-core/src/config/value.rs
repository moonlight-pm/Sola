//! Config value and mutation types shared across the bus and config store.

use serde::{Deserialize, Serialize};

/// A configuration value. Mirrors TOML's type system minus Datetime.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum ConfigValue {
    String(String),
    Int(i64),
    Float(f64),
    Bool(bool),
    Array(Vec<ConfigValue>),
    Table(Vec<(String, ConfigValue)>),
}

impl ConfigValue {
    /// Convert from `toml::Value`.
    pub fn from_toml(v: &toml::Value) -> Self {
        match v {
            toml::Value::String(s) => Self::String(s.clone()),
            toml::Value::Integer(i) => Self::Int(*i),
            toml::Value::Float(f) => Self::Float(*f),
            toml::Value::Boolean(b) => Self::Bool(*b),
            toml::Value::Array(a) => Self::Array(a.iter().map(Self::from_toml).collect()),
            toml::Value::Table(t) => {
                Self::Table(t.iter().map(|(k, v)| (k.clone(), Self::from_toml(v))).collect())
            }
            toml::Value::Datetime(d) => Self::String(d.to_string()),
        }
    }

    /// Convert to `toml::Value`.
    pub fn to_toml(&self) -> toml::Value {
        match self {
            Self::String(s) => toml::Value::String(s.clone()),
            Self::Int(i) => toml::Value::Integer(*i),
            Self::Float(f) => toml::Value::Float(*f),
            Self::Bool(b) => toml::Value::Boolean(*b),
            Self::Array(a) => toml::Value::Array(a.iter().map(|v| v.to_toml()).collect()),
            Self::Table(t) => {
                let mut map = toml::map::Map::new();
                for (k, v) in t {
                    map.insert(k.clone(), v.to_toml());
                }
                toml::Value::Table(map)
            }
        }
    }
}

/// Operation for a config mutation request.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum MutateOp {
    /// Set a key to a value. Creates intermediate tables as needed.
    Set(ConfigValue),
    /// Delete a key (and all children under that prefix).
    Delete,
    /// Append a value to the array at key.
    Append(ConfigValue),
    /// Insert a value at the given index, shifting later elements right.
    Insert { index: u32, value: ConfigValue },
    /// Remove the element at the given index, shifting later elements left.
    Remove { index: u32 },
    /// Replace the element at the given index.
    Replace { index: u32, value: ConfigValue },
}

/// Payload for a `MutateConfig` bus message.
///
/// Any app can emit this. Session validates, applies, persists,
/// and emits a new `Config` sticky if the mutation succeeded.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MutateConfigPayload {
    /// Dotted key path, e.g. `"mail.imap_port"` or `"shell.applications"`.
    pub key: String,
    pub op: MutateOp,
}
