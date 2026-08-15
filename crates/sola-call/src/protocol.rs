//! Wire types for the call plane. JSON is the contract.

use serde::{Deserialize, Serialize};

pub const DEFAULT_TIMEOUT_MS: u64 = 8_000;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum Role {
    Caller,
    Provider,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum ArgType {
    String,
    Int,
    Float,
    Bool,
    Path,
    Json,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ArgSpec {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub long: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub short: Option<char>,
    #[serde(rename = "type")]
    pub ty: ArgType,
    #[serde(default)]
    pub required: bool,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub help: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct MethodSpec {
    pub name: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub summary: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub args: Vec<ArgSpec>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct OwnerCatalog {
    pub owner: String,
    pub app_id: String,
    pub methods: Vec<MethodSpec>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub enum Wire {
    Hello {
        role: Role,
        app_id: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        owner: Option<String>,
    },
    Advertise {
        methods: Vec<MethodSpec>,
    },
    Invoke {
        id: String,
        method: String,
        #[serde(default)]
        params: serde_json::Value,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        owner: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        timeout_ms: Option<u64>,
    },
    Reply {
        id: String,
        ok: bool,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        error: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        data: Option<serde_json::Value>,
    },
    List,
    Catalog {
        owners: Vec<OwnerCatalog>,
    },
}

impl Wire {
    pub fn reply_ok(id: impl Into<String>, data: serde_json::Value) -> Self {
        Self::Reply {
            id: id.into(),
            ok: true,
            error: None,
            data: Some(data),
        }
    }

    pub fn reply_err(id: impl Into<String>, error: impl Into<String>) -> Self {
        Self::Reply {
            id: id.into(),
            ok: false,
            error: Some(error.into()),
            data: None,
        }
    }
}

pub fn new_id() -> String {
    uuid::Uuid::now_v7().to_string()
}
