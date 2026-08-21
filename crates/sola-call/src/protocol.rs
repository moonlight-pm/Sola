//! Wire types for the call plane. JSON is the contract.

use serde::{Deserialize, Serialize};

pub const DEFAULT_TIMEOUT_MS: u64 = 8_000;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum Role {
    Caller,
    Provider,
    /// Long-lived auditor. Receives [`Wire::Catalog`] snapshots and
    /// [`Wire::Trace`] copies of invoke/reply/timeout/advertise/unregister.
    /// Same socket privilege as a caller — local-user `0600`.
    Observer,
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
    /// Suggested caller deadline. `solactl` uses this when the method
    /// does not take a `timeout` arg. Omit for the 8s default.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timeout_ms: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct OwnerCatalog {
    pub owner: String,
    pub app_id: String,
    pub methods: Vec<MethodSpec>,
}

/// One audited call-plane event, fanned out to observers.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TraceEvent {
    pub kind: TraceKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub owner: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub caller: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub method: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub params: Option<serde_json::Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ok: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub data: Option<serde_json::Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub duration_ms: Option<u64>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum TraceKind {
    Invoke,
    Reply,
    Timeout,
    Advertise,
    Unregister,
}

impl TraceEvent {
    pub fn invoke(
        id: impl Into<String>,
        owner: impl Into<String>,
        caller: impl Into<String>,
        method: impl Into<String>,
        params: serde_json::Value,
    ) -> Self {
        Self {
            kind: TraceKind::Invoke,
            id: Some(id.into()),
            owner: Some(owner.into()),
            caller: Some(caller.into()),
            method: Some(method.into()),
            params: Some(params),
            ok: None,
            error: None,
            data: None,
            duration_ms: None,
        }
    }

    pub fn reply(
        id: impl Into<String>,
        owner: impl Into<String>,
        caller: impl Into<String>,
        method: impl Into<String>,
        ok: bool,
        error: Option<String>,
        data: Option<serde_json::Value>,
        duration_ms: u64,
    ) -> Self {
        Self {
            kind: TraceKind::Reply,
            id: Some(id.into()),
            owner: Some(owner.into()),
            caller: Some(caller.into()),
            method: Some(method.into()),
            params: None,
            ok: Some(ok),
            error,
            data,
            duration_ms: Some(duration_ms),
        }
    }

    pub fn timeout(
        id: impl Into<String>,
        owner: impl Into<String>,
        caller: impl Into<String>,
        method: impl Into<String>,
        duration_ms: u64,
    ) -> Self {
        Self {
            kind: TraceKind::Timeout,
            id: Some(id.into()),
            owner: Some(owner.into()),
            caller: Some(caller.into()),
            method: Some(method.into()),
            params: None,
            ok: Some(false),
            error: Some("timeout".into()),
            data: None,
            duration_ms: Some(duration_ms),
        }
    }

    pub fn advertise(owner: impl Into<String>, methods: Vec<MethodSpec>) -> Self {
        Self {
            kind: TraceKind::Advertise,
            id: None,
            owner: Some(owner.into()),
            caller: None,
            method: None,
            params: None,
            ok: None,
            error: None,
            data: Some(serde_json::to_value(methods).unwrap_or(serde_json::Value::Null)),
            duration_ms: None,
        }
    }

    pub fn unregister(owner: impl Into<String>) -> Self {
        Self {
            kind: TraceKind::Unregister,
            id: None,
            owner: Some(owner.into()),
            caller: None,
            method: None,
            params: None,
            ok: None,
            error: None,
            data: None,
            duration_ms: None,
        }
    }
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
    Trace(TraceEvent),
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trace_wire_roundtrip() {
        let ev = TraceEvent::invoke(
            "abc",
            "compositor",
            "solactl",
            "windows",
            serde_json::json!({"k": 1}),
        );
        let bytes = serde_json::to_vec(&Wire::Trace(ev.clone())).unwrap();
        let back: Wire = serde_json::from_slice(&bytes).unwrap();
        match back {
            Wire::Trace(got) => {
                assert_eq!(got.kind, TraceKind::Invoke);
                assert_eq!(got.id.as_deref(), Some("abc"));
                assert_eq!(got.owner.as_deref(), Some("compositor"));
                assert_eq!(got.params, Some(serde_json::json!({"k": 1})));
            }
            other => panic!("expected Trace, got {other:?}"),
        }
        let s = String::from_utf8(bytes).unwrap();
        assert!(s.contains("\"type\":\"trace\""), "{s}");
    }
}
