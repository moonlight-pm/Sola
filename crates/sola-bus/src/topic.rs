use std::any::{Any, TypeId};
use std::io;

use serde::Deserialize;
use serde::de::DeserializeOwned;

use crate::Message;

/// Decode a payload from a `Message` into the expected type.
///
/// Postcard is positional: adding fields to a payload breaks replay of
/// stickies still encoded by an older bus. [`Application`] grows
/// `kind`/`url` — if the current layout misses those trailing fields,
/// fall back to the pre-wrapper four-string record.
pub fn decode_payload<T: DeserializeOwned + 'static>(msg: &Message) -> io::Result<T> {
    let bytes = msg
        .payload
        .as_ref()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "missing payload"))?;
    match postcard::from_bytes(bytes) {
        Ok(v) => Ok(v),
        Err(e) => match decode_legacy_application::<T>(bytes)
            .or_else(|| crate::topics::try_decode_legacy_mail::<T>(bytes))
        {
            Some(v) => Ok(v),
            None => Err(io::Error::new(io::ErrorKind::InvalidData, e)),
        },
    }
}

fn decode_legacy_application<T: 'static>(bytes: &[u8]) -> Option<T> {
    use sola_core::applications::{AppKind, Application};
    if TypeId::of::<T>() != TypeId::of::<Application>() {
        return None;
    }
    #[derive(Deserialize)]
    struct ApplicationV1 {
        app_id: String,
        label: String,
        command: String,
        icon: String,
    }
    let v1: ApplicationV1 = postcard::from_bytes(bytes).ok()?;
    let app = Application {
        app_id: v1.app_id,
        label: v1.label,
        command: v1.command,
        icon: v1.icon,
        kind: AppKind::Command,
        url: None,
    };
    let boxed: Box<dyn Any> = Box::new(app);
    boxed.downcast::<T>().ok().map(|b| *b)
}

/// Serialize a value to a payload byte vec.
pub fn encode_payload<T: serde::Serialize>(value: &T) -> Vec<u8> {
    postcard::to_allocvec(value).expect("failed to serialize topic payload")
}

/// Convert a persistent topic's typed payload to a YAML value for
/// storage in `state.yaml`. Returns `None` if the payload can't be
/// represented in YAML. Used by the macro-generated
/// `Topic::to_yaml_value`.
pub fn payload_to_yaml<T: serde::Serialize>(value: &T) -> Option<serde_yaml_ng::Value> {
    serde_yaml_ng::to_value(value).ok()
}

/// Deserialize a `state.yaml` section into a topic payload. Returns
/// `None` on schema mismatch — the bus logs and leaves the topic
/// unset. Used by the macro-generated `Topic::from_yaml_section`.
pub fn payload_from_yaml<T: serde::de::DeserializeOwned>(value: serde_yaml_ng::Value) -> Option<T> {
    serde_yaml_ng::from_value(value).ok()
}

/// Empty YAML mapping. Used as the serialized form of a persistent unit
/// variant (presence-only persistent signal; section exists in the
/// file but carries no data).
pub fn empty_yaml_section() -> serde_yaml_ng::Value {
    serde_yaml_ng::Value::Mapping(serde_yaml_ng::Mapping::new())
}

/// Delivery behavior for a topic kind.
///
/// - `Ephemeral` — delivered once to current subscribers; nothing retained.
/// - `Sticky` — latest value per (kind, emitter) retained in memory and
///   replayed to new subscribers.
/// - `Persistent` — sticky + written to disk and restored on bus start.
///   Persistent implies sticky behavior.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Behavior {
    Ephemeral,
    Sticky,
    Persistent,
}

impl Behavior {
    /// True if the latest value should be retained and replayed to new
    /// subscribers. Both `Sticky` and `Persistent` qualify.
    pub fn is_sticky(self) -> bool {
        matches!(self, Behavior::Sticky | Behavior::Persistent)
    }

    /// True if the latest value should be saved to disk and restored on
    /// bus start.
    pub fn is_persistent(self) -> bool {
        matches!(self, Behavior::Persistent)
    }
}

/// Failure modes for namespace path interpolation.
#[derive(Debug, PartialEq, Eq)]
pub enum PathError {
    /// The interpolated key value was empty.
    Empty(&'static str),
    /// The interpolated key value contained `/`, `\0`, or a `..` segment.
    Forbidden(&'static str, String),
    /// The namespace template referenced a `:placeholder` that doesn't
    /// match any declared key field.
    UnknownPlaceholder(String),
}

impl std::fmt::Display for PathError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PathError::Empty(name) => write!(f, "namespace key `{name}` is empty"),
            PathError::Forbidden(name, value) => {
                write!(
                    f,
                    "namespace key `{name}` contains forbidden characters: {value:?}"
                )
            }
            PathError::UnknownPlaceholder(name) => {
                write!(f, "namespace template references unknown key `{name}`")
            }
        }
    }
}

impl std::error::Error for PathError {}

/// Substitute `:keyname` placeholders in `template` with values from
/// `values`, matched by index against `names`. Each value is validated:
/// non-empty, no `/`, no `\0`, no `..` segment.
///
/// `names[i]` is the declared name of the `i`th key field;
/// `values[i]` is its `Display`-stringified value at runtime.
pub fn interpolate_namespace(
    template: &str,
    names: &[&'static str],
    values: &[String],
) -> Result<String, PathError> {
    let mut out = String::with_capacity(template.len());
    let mut rest = template;
    while let Some(idx) = rest.find(':') {
        out.push_str(&rest[..idx]);
        let after = &rest[idx + 1..];
        let mut matched: Option<(&'static str, &str)> = None;
        for (i, name) in names.iter().enumerate() {
            if after.starts_with(name)
                && after[name.len()..]
                    .chars()
                    .next()
                    .is_none_or(|c| !c.is_ascii_alphanumeric() && c != '_')
            {
                let value = values
                    .get(i)
                    .ok_or(PathError::UnknownPlaceholder((*name).into()))?;
                if value.is_empty() {
                    return Err(PathError::Empty(*name));
                }
                if value.contains('/') || value.contains('\0') {
                    return Err(PathError::Forbidden(*name, value.clone()));
                }
                if value == ".." || value.split('/').any(|seg| seg == "..") {
                    return Err(PathError::Forbidden(*name, value.clone()));
                }
                matched = Some((*name, value));
                break;
            }
        }
        let (name, value) = matched.ok_or_else(|| {
            let end = after
                .find(|c: char| !c.is_ascii_alphanumeric() && c != '_')
                .unwrap_or(after.len());
            PathError::UnknownPlaceholder(after[..end].into())
        })?;
        out.push_str(value);
        rest = &after[name.len()..];
    }
    out.push_str(rest);
    Ok(out)
}

/// Define a Topic enum with typed variants, a parse function, and to_message.
///
/// Variant forms:
/// - Unit: `Shutdown,` — no payload
/// - Payload: `GrabInput(String),` — carries typed data
///
/// Optional per-variant attributes declare delivery behavior:
/// - no attribute → ephemeral (default)
/// - `#[sticky]` → latest value retained and replayed to new subscribers
/// - `#[persistent]` → sticky + saved to disk
///
/// `#[sticky]` and `#[persistent]` payload variants may also declare
/// one or more **key fields** (unquoted idents, in declaration order):
/// - `#[sticky(keys = [field])]`
/// - `#[persistent(keys = [field_a, field_b])]`
///
/// Key fields are extracted from the payload via `Display` and used by
/// the bus to address sticky records as `(topic, keys)`. A keyed topic
/// kind can have many concurrent stickies; an unkeyed sticky/persistent
/// kind has at most one.
///
/// # Example
/// ```ignore
/// define_topics! {
///     Shutdown,
///     GrabInput(String),
///     #[sticky]
///     Windows(Vec<Window>),
///     #[sticky(keys = [app_id])]
///     SetAppMenu(AppMenuPayload),
///     #[persistent]
///     Zones(HashMap<String, Zone>),
/// }
/// ```
/// Internal helper for the `define_topics!` namespace match arm.
/// Captures the `ns=()` (no namespace) and `ns=("…")` (namespace literal)
/// shapes carried through the macro accumulator and converts them into
/// `Option<&'static str>`.
#[macro_export]
#[doc(hidden)]
macro_rules! __ns_or_none {
    ( () ) => {
        None
    };
    ( ($ns:literal) ) => {
        Some($ns)
    };
}

#[macro_export]
macro_rules! define_topics {
    ( $($tt:tt)* ) => {
        $crate::_define_topics_inner!{
            [] []     // ephemeral units, ephemeral payloads
            [] []     // sticky units, sticky payloads
            [] []     // persistent units, persistent payloads
            $($tt)*
        }
    };
}

#[macro_export]
#[doc(hidden)]
macro_rules! _define_topics_inner {
    // --- #[persistent(keys = [...], namespace = "...")] payload variants ---
    // (Order: keys first, namespace second when both are present.)
    ( [ $($eu:ident)* ] [ $($ep:ident($ept:ty))* ]
      [ $($su:ident)* ] [ $($sp:ident($spt:ty) keys=[$($spk:ident)*])* ]
      [ $($pu:ident)* ] [ $($pp:ident($ppt:ty) keys=[$($ppk:ident)*] ns=$ppns:tt)* ]
      #[persistent(keys = [ $($k:ident),+ $(,)? ], namespace = $ns:literal)] $name:ident ( $payload:ty ), $($rest:tt)* ) => {
        $crate::_define_topics_inner!{
            [ $($eu)* ] [ $($ep($ept))* ]
            [ $($su)* ] [ $($sp($spt) keys=[$($spk)*])* ]
            [ $($pu)* ] [ $($pp($ppt) keys=[$($ppk)*] ns=$ppns)* $name($payload) keys=[$($k)*] ns=($ns) ]
            $($rest)*
        }
    };
    ( [ $($eu:ident)* ] [ $($ep:ident($ept:ty))* ]
      [ $($su:ident)* ] [ $($sp:ident($spt:ty) keys=[$($spk:ident)*])* ]
      [ $($pu:ident)* ] [ $($pp:ident($ppt:ty) keys=[$($ppk:ident)*] ns=$ppns:tt)* ]
      #[persistent(keys = [ $($k:ident),+ $(,)? ], namespace = $ns:literal)] $name:ident ( $payload:ty ) ) => {
        $crate::_define_topics_inner!{
            [ $($eu)* ] [ $($ep($ept))* ]
            [ $($su)* ] [ $($sp($spt) keys=[$($spk)*])* ]
            [ $($pu)* ] [ $($pp($ppt) keys=[$($ppk)*] ns=$ppns)* $name($payload) keys=[$($k)*] ns=($ns) ]
        }
    };

    // --- #[persistent(namespace = "...")] payload variants (no keys) ---
    ( [ $($eu:ident)* ] [ $($ep:ident($ept:ty))* ]
      [ $($su:ident)* ] [ $($sp:ident($spt:ty) keys=[$($spk:ident)*])* ]
      [ $($pu:ident)* ] [ $($pp:ident($ppt:ty) keys=[$($ppk:ident)*] ns=$ppns:tt)* ]
      #[persistent(namespace = $ns:literal)] $name:ident ( $payload:ty ), $($rest:tt)* ) => {
        $crate::_define_topics_inner!{
            [ $($eu)* ] [ $($ep($ept))* ]
            [ $($su)* ] [ $($sp($spt) keys=[$($spk)*])* ]
            [ $($pu)* ] [ $($pp($ppt) keys=[$($ppk)*] ns=$ppns)* $name($payload) keys=[] ns=($ns) ]
            $($rest)*
        }
    };
    ( [ $($eu:ident)* ] [ $($ep:ident($ept:ty))* ]
      [ $($su:ident)* ] [ $($sp:ident($spt:ty) keys=[$($spk:ident)*])* ]
      [ $($pu:ident)* ] [ $($pp:ident($ppt:ty) keys=[$($ppk:ident)*] ns=$ppns:tt)* ]
      #[persistent(namespace = $ns:literal)] $name:ident ( $payload:ty ) ) => {
        $crate::_define_topics_inner!{
            [ $($eu)* ] [ $($ep($ept))* ]
            [ $($su)* ] [ $($sp($spt) keys=[$($spk)*])* ]
            [ $($pu)* ] [ $($pp($ppt) keys=[$($ppk)*] ns=$ppns)* $name($payload) keys=[] ns=($ns) ]
        }
    };

    // --- #[persistent(keys = [...])] payload variants ---
    ( [ $($eu:ident)* ] [ $($ep:ident($ept:ty))* ]
      [ $($su:ident)* ] [ $($sp:ident($spt:ty) keys=[$($spk:ident)*])* ]
      [ $($pu:ident)* ] [ $($pp:ident($ppt:ty) keys=[$($ppk:ident)*] ns=$ppns:tt)* ]
      #[persistent(keys = [ $($k:ident),+ $(,)? ])] $name:ident ( $payload:ty ), $($rest:tt)* ) => {
        $crate::_define_topics_inner!{
            [ $($eu)* ] [ $($ep($ept))* ]
            [ $($su)* ] [ $($sp($spt) keys=[$($spk)*])* ]
            [ $($pu)* ] [ $($pp($ppt) keys=[$($ppk)*] ns=$ppns)* $name($payload) keys=[$($k)*] ns=() ]
            $($rest)*
        }
    };
    ( [ $($eu:ident)* ] [ $($ep:ident($ept:ty))* ]
      [ $($su:ident)* ] [ $($sp:ident($spt:ty) keys=[$($spk:ident)*])* ]
      [ $($pu:ident)* ] [ $($pp:ident($ppt:ty) keys=[$($ppk:ident)*] ns=$ppns:tt)* ]
      #[persistent(keys = [ $($k:ident),+ $(,)? ])] $name:ident ( $payload:ty ) ) => {
        $crate::_define_topics_inner!{
            [ $($eu)* ] [ $($ep($ept))* ]
            [ $($su)* ] [ $($sp($spt) keys=[$($spk)*])* ]
            [ $($pu)* ] [ $($pp($ppt) keys=[$($ppk)*] ns=$ppns)* $name($payload) keys=[$($k)*] ns=() ]
        }
    };

    // --- #[persistent] payload variants (no keys) ---
    ( [ $($eu:ident)* ] [ $($ep:ident($ept:ty))* ]
      [ $($su:ident)* ] [ $($sp:ident($spt:ty) keys=[$($spk:ident)*])* ]
      [ $($pu:ident)* ] [ $($pp:ident($ppt:ty) keys=[$($ppk:ident)*] ns=$ppns:tt)* ]
      #[persistent] $name:ident ( $payload:ty ), $($rest:tt)* ) => {
        $crate::_define_topics_inner!{
            [ $($eu)* ] [ $($ep($ept))* ]
            [ $($su)* ] [ $($sp($spt) keys=[$($spk)*])* ]
            [ $($pu)* ] [ $($pp($ppt) keys=[$($ppk)*] ns=$ppns)* $name($payload) keys=[] ns=() ]
            $($rest)*
        }
    };
    ( [ $($eu:ident)* ] [ $($ep:ident($ept:ty))* ]
      [ $($su:ident)* ] [ $($sp:ident($spt:ty) keys=[$($spk:ident)*])* ]
      [ $($pu:ident)* ] [ $($pp:ident($ppt:ty) keys=[$($ppk:ident)*] ns=$ppns:tt)* ]
      #[persistent] $name:ident ( $payload:ty ) ) => {
        $crate::_define_topics_inner!{
            [ $($eu)* ] [ $($ep($ept))* ]
            [ $($su)* ] [ $($sp($spt) keys=[$($spk)*])* ]
            [ $($pu)* ] [ $($pp($ppt) keys=[$($ppk)*] ns=$ppns)* $name($payload) keys=[] ns=() ]
        }
    };

    // --- #[persistent] unit variants (keys not allowed on units) ---
    ( [ $($eu:ident)* ] [ $($ep:ident($ept:ty))* ]
      [ $($su:ident)* ] [ $($sp:ident($spt:ty) keys=[$($spk:ident)*])* ]
      [ $($pu:ident)* ] [ $($pp:ident($ppt:ty) keys=[$($ppk:ident)*] ns=$ppns:tt)* ]
      #[persistent] $name:ident, $($rest:tt)* ) => {
        $crate::_define_topics_inner!{
            [ $($eu)* ] [ $($ep($ept))* ]
            [ $($su)* ] [ $($sp($spt) keys=[$($spk)*])* ]
            [ $($pu)* $name ] [ $($pp($ppt) keys=[$($ppk)*] ns=$ppns)* ]
            $($rest)*
        }
    };
    ( [ $($eu:ident)* ] [ $($ep:ident($ept:ty))* ]
      [ $($su:ident)* ] [ $($sp:ident($spt:ty) keys=[$($spk:ident)*])* ]
      [ $($pu:ident)* ] [ $($pp:ident($ppt:ty) keys=[$($ppk:ident)*] ns=$ppns:tt)* ]
      #[persistent] $name:ident ) => {
        $crate::_define_topics_inner!{
            [ $($eu)* ] [ $($ep($ept))* ]
            [ $($su)* ] [ $($sp($spt) keys=[$($spk)*])* ]
            [ $($pu)* $name ] [ $($pp($ppt) keys=[$($ppk)*] ns=$ppns)* ]
        }
    };

    // --- #[sticky(keys = [...])] payload variants ---
    ( [ $($eu:ident)* ] [ $($ep:ident($ept:ty))* ]
      [ $($su:ident)* ] [ $($sp:ident($spt:ty) keys=[$($spk:ident)*])* ]
      [ $($pu:ident)* ] [ $($pp:ident($ppt:ty) keys=[$($ppk:ident)*] ns=$ppns:tt)* ]
      #[sticky(keys = [ $($k:ident),+ $(,)? ])] $name:ident ( $payload:ty ), $($rest:tt)* ) => {
        $crate::_define_topics_inner!{
            [ $($eu)* ] [ $($ep($ept))* ]
            [ $($su)* ] [ $($sp($spt) keys=[$($spk)*])* $name($payload) keys=[$($k)*] ]
            [ $($pu)* ] [ $($pp($ppt) keys=[$($ppk)*] ns=$ppns)* ]
            $($rest)*
        }
    };
    ( [ $($eu:ident)* ] [ $($ep:ident($ept:ty))* ]
      [ $($su:ident)* ] [ $($sp:ident($spt:ty) keys=[$($spk:ident)*])* ]
      [ $($pu:ident)* ] [ $($pp:ident($ppt:ty) keys=[$($ppk:ident)*] ns=$ppns:tt)* ]
      #[sticky(keys = [ $($k:ident),+ $(,)? ])] $name:ident ( $payload:ty ) ) => {
        $crate::_define_topics_inner!{
            [ $($eu)* ] [ $($ep($ept))* ]
            [ $($su)* ] [ $($sp($spt) keys=[$($spk)*])* $name($payload) keys=[$($k)*] ]
            [ $($pu)* ] [ $($pp($ppt) keys=[$($ppk)*] ns=$ppns)* ]
        }
    };

    // --- #[sticky] payload variants (no keys) ---
    ( [ $($eu:ident)* ] [ $($ep:ident($ept:ty))* ]
      [ $($su:ident)* ] [ $($sp:ident($spt:ty) keys=[$($spk:ident)*])* ]
      [ $($pu:ident)* ] [ $($pp:ident($ppt:ty) keys=[$($ppk:ident)*] ns=$ppns:tt)* ]
      #[sticky] $name:ident ( $payload:ty ), $($rest:tt)* ) => {
        $crate::_define_topics_inner!{
            [ $($eu)* ] [ $($ep($ept))* ]
            [ $($su)* ] [ $($sp($spt) keys=[$($spk)*])* $name($payload) keys=[] ]
            [ $($pu)* ] [ $($pp($ppt) keys=[$($ppk)*] ns=$ppns)* ]
            $($rest)*
        }
    };
    ( [ $($eu:ident)* ] [ $($ep:ident($ept:ty))* ]
      [ $($su:ident)* ] [ $($sp:ident($spt:ty) keys=[$($spk:ident)*])* ]
      [ $($pu:ident)* ] [ $($pp:ident($ppt:ty) keys=[$($ppk:ident)*] ns=$ppns:tt)* ]
      #[sticky] $name:ident ( $payload:ty ) ) => {
        $crate::_define_topics_inner!{
            [ $($eu)* ] [ $($ep($ept))* ]
            [ $($su)* ] [ $($sp($spt) keys=[$($spk)*])* $name($payload) keys=[] ]
            [ $($pu)* ] [ $($pp($ppt) keys=[$($ppk)*] ns=$ppns)* ]
        }
    };

    // --- #[sticky] unit variants ---
    ( [ $($eu:ident)* ] [ $($ep:ident($ept:ty))* ]
      [ $($su:ident)* ] [ $($sp:ident($spt:ty) keys=[$($spk:ident)*])* ]
      [ $($pu:ident)* ] [ $($pp:ident($ppt:ty) keys=[$($ppk:ident)*] ns=$ppns:tt)* ]
      #[sticky] $name:ident, $($rest:tt)* ) => {
        $crate::_define_topics_inner!{
            [ $($eu)* ] [ $($ep($ept))* ]
            [ $($su)* $name ] [ $($sp($spt) keys=[$($spk)*])* ]
            [ $($pu)* ] [ $($pp($ppt) keys=[$($ppk)*] ns=$ppns)* ]
            $($rest)*
        }
    };
    ( [ $($eu:ident)* ] [ $($ep:ident($ept:ty))* ]
      [ $($su:ident)* ] [ $($sp:ident($spt:ty) keys=[$($spk:ident)*])* ]
      [ $($pu:ident)* ] [ $($pp:ident($ppt:ty) keys=[$($ppk:ident)*] ns=$ppns:tt)* ]
      #[sticky] $name:ident ) => {
        $crate::_define_topics_inner!{
            [ $($eu)* ] [ $($ep($ept))* ]
            [ $($su)* $name ] [ $($sp($spt) keys=[$($spk)*])* ]
            [ $($pu)* ] [ $($pp($ppt) keys=[$($ppk)*] ns=$ppns)* ]
        }
    };

    // --- Ephemeral payload variants ---
    ( [ $($eu:ident)* ] [ $($ep:ident($ept:ty))* ]
      [ $($su:ident)* ] [ $($sp:ident($spt:ty) keys=[$($spk:ident)*])* ]
      [ $($pu:ident)* ] [ $($pp:ident($ppt:ty) keys=[$($ppk:ident)*] ns=$ppns:tt)* ]
      $name:ident ( $payload:ty ), $($rest:tt)* ) => {
        $crate::_define_topics_inner!{
            [ $($eu)* ] [ $($ep($ept))* $name($payload) ]
            [ $($su)* ] [ $($sp($spt) keys=[$($spk)*])* ]
            [ $($pu)* ] [ $($pp($ppt) keys=[$($ppk)*] ns=$ppns)* ]
            $($rest)*
        }
    };
    ( [ $($eu:ident)* ] [ $($ep:ident($ept:ty))* ]
      [ $($su:ident)* ] [ $($sp:ident($spt:ty) keys=[$($spk:ident)*])* ]
      [ $($pu:ident)* ] [ $($pp:ident($ppt:ty) keys=[$($ppk:ident)*] ns=$ppns:tt)* ]
      $name:ident ( $payload:ty ) ) => {
        $crate::_define_topics_inner!{
            [ $($eu)* ] [ $($ep($ept))* $name($payload) ]
            [ $($su)* ] [ $($sp($spt) keys=[$($spk)*])* ]
            [ $($pu)* ] [ $($pp($ppt) keys=[$($ppk)*] ns=$ppns)* ]
        }
    };

    // --- Ephemeral unit variants ---
    ( [ $($eu:ident)* ] [ $($ep:ident($ept:ty))* ]
      [ $($su:ident)* ] [ $($sp:ident($spt:ty) keys=[$($spk:ident)*])* ]
      [ $($pu:ident)* ] [ $($pp:ident($ppt:ty) keys=[$($ppk:ident)*] ns=$ppns:tt)* ]
      $name:ident, $($rest:tt)* ) => {
        $crate::_define_topics_inner!{
            [ $($eu)* $name ] [ $($ep($ept))* ]
            [ $($su)* ] [ $($sp($spt) keys=[$($spk)*])* ]
            [ $($pu)* ] [ $($pp($ppt) keys=[$($ppk)*] ns=$ppns)* ]
            $($rest)*
        }
    };
    ( [ $($eu:ident)* ] [ $($ep:ident($ept:ty))* ]
      [ $($su:ident)* ] [ $($sp:ident($spt:ty) keys=[$($spk:ident)*])* ]
      [ $($pu:ident)* ] [ $($pp:ident($ppt:ty) keys=[$($ppk:ident)*] ns=$ppns:tt)* ]
      $name:ident ) => {
        $crate::_define_topics_inner!{
            [ $($eu)* $name ] [ $($ep($ept))* ]
            [ $($su)* ] [ $($sp($spt) keys=[$($spk)*])* ]
            [ $($pu)* ] [ $($pp($ppt) keys=[$($ppk)*] ns=$ppns)* ]
        }
    };

    // --- Terminal: generate Topic, TopicKind, Behavior wiring ---
    ( [ $($eu:ident)* ] [ $($ep:ident($ept:ty))* ]
      [ $($su:ident)* ] [ $($sp:ident($spt:ty) keys=[$($spk:ident)*])* ]
      [ $($pu:ident)* ] [ $($pp:ident($ppt:ty) keys=[$($ppk:ident)*] ns=$ppns:tt)* ]
    ) => {
        #[derive(Debug, Clone)]
        pub enum Topic {
            $( $eu, )*
            $( $ep($ept), )*
            $( $su, )*
            $( $sp($spt), )*
            $( $pu, )*
            $( $pp($ppt), )*
        }

        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
        #[derive(serde::Serialize, serde::Deserialize)]
        pub enum TopicKind {
            $( $eu, )*
            $( $ep, )*
            $( $su, )*
            $( $sp, )*
            $( $pu, )*
            $( $pp, )*
        }

        impl TopicKind {
            pub const ALL: &'static [TopicKind] = &[
                $( TopicKind::$eu, )*
                $( TopicKind::$ep, )*
                $( TopicKind::$su, )*
                $( TopicKind::$sp, )*
                $( TopicKind::$pu, )*
                $( TopicKind::$pp, )*
            ];

            pub fn as_str(self) -> &'static str {
                match self {
                    $( TopicKind::$eu => stringify!($eu), )*
                    $( TopicKind::$ep => stringify!($ep), )*
                    $( TopicKind::$su => stringify!($su), )*
                    $( TopicKind::$sp => stringify!($sp), )*
                    $( TopicKind::$pu => stringify!($pu), )*
                    $( TopicKind::$pp => stringify!($pp), )*
                }
            }

            /// Map a section name (as it appears in `state.yaml`) back
            /// to a `TopicKind`. Returns `None` for unknown sections so
            /// the bus can log and skip rather than fail to start.
            pub fn from_str(name: &str) -> Option<TopicKind> {
                match name {
                    $( stringify!($eu) => Some(TopicKind::$eu), )*
                    $( stringify!($ep) => Some(TopicKind::$ep), )*
                    $( stringify!($su) => Some(TopicKind::$su), )*
                    $( stringify!($sp) => Some(TopicKind::$sp), )*
                    $( stringify!($pu) => Some(TopicKind::$pu), )*
                    $( stringify!($pp) => Some(TopicKind::$pp), )*
                    _ => None,
                }
            }

            /// Delivery behavior for this topic kind.
            pub fn behavior(self) -> $crate::topic::Behavior {
                match self {
                    $( TopicKind::$eu => $crate::topic::Behavior::Ephemeral, )*
                    $( TopicKind::$ep => $crate::topic::Behavior::Ephemeral, )*
                    $( TopicKind::$su => $crate::topic::Behavior::Sticky, )*
                    $( TopicKind::$sp => $crate::topic::Behavior::Sticky, )*
                    $( TopicKind::$pu => $crate::topic::Behavior::Persistent, )*
                    $( TopicKind::$pp => $crate::topic::Behavior::Persistent, )*
                }
            }

            /// True if this topic kind declared one or more key fields.
            /// Keyed kinds have many concurrent stickies on the bus,
            /// addressed by `(topic, keys)`. Unkeyed sticky/persistent
            /// kinds have at most one sticky in flight at a time.
            #[allow(unused_variables)]
            pub fn has_keys(self) -> bool {
                match self {
                    $( TopicKind::$sp => { let _ = stringify!($($spk)*); !stringify!($($spk)*).is_empty() }, )*
                    $( TopicKind::$pp => { let _ = stringify!($($ppk)*); !stringify!($($ppk)*).is_empty() }, )*
                    _ => false,
                }
            }

            /// Path-namespace template declared via
            /// `#[persistent(namespace = "...")]`. Returns `None` for
            /// unnamespaced and non-persistent kinds. The string may
            /// contain `:keyname` placeholders that interpolate from
            /// `keys_for()` at runtime — see `Topic::path_for`.
            #[allow(unused_variables)]
            pub fn namespace(self) -> Option<&'static str> {
                match self {
                    $( TopicKind::$pp => $crate::__ns_or_none!($ppns), )*
                    _ => None,
                }
            }

            /// Names of the key fields declared on this topic kind, in
            /// declaration order. Empty slice for unkeyed kinds. Used
            /// alongside `Topic::keys_for()` to interpolate `:keyname`
            /// placeholders in a namespace template.
            pub fn key_names(self) -> &'static [&'static str] {
                match self {
                    $( TopicKind::$sp => &[ $( stringify!($spk), )* ], )*
                    $( TopicKind::$pp => &[ $( stringify!($ppk), )* ], )*
                    _ => &[],
                }
            }
        }

        impl Topic {
            pub fn parse(msg: &$crate::Message) -> Option<Self> {
                match msg.topic.as_str() {
                    $( stringify!($eu) => Some(Topic::$eu), )*
                    $( stringify!($ep) => {
                        $crate::topic::decode_payload::<$ept>(msg).ok().map(Topic::$ep)
                    }, )*
                    $( stringify!($su) => Some(Topic::$su), )*
                    $( stringify!($sp) => {
                        $crate::topic::decode_payload::<$spt>(msg).ok().map(Topic::$sp)
                    }, )*
                    $( stringify!($pu) => Some(Topic::$pu), )*
                    $( stringify!($pp) => {
                        $crate::topic::decode_payload::<$ppt>(msg).ok().map(Topic::$pp)
                    }, )*
                    _ => None,
                }
            }

            pub fn to_message(&self) -> $crate::Message {
                let mut msg = match self {
                    $( Topic::$eu => $crate::Message::new(stringify!($eu)), )*
                    $( Topic::$ep(payload) => $crate::Message::with_payload(
                        stringify!($ep),
                        $crate::topic::encode_payload(payload),
                    ), )*
                    $( Topic::$su => $crate::Message::new(stringify!($su)), )*
                    $( Topic::$sp(payload) => $crate::Message::with_payload(
                        stringify!($sp),
                        $crate::topic::encode_payload(payload),
                    ), )*
                    $( Topic::$pu => $crate::Message::new(stringify!($pu)), )*
                    $( Topic::$pp(payload) => $crate::Message::with_payload(
                        stringify!($pp),
                        $crate::topic::encode_payload(payload),
                    ), )*
                };
                msg.keys = self.keys_for();
                msg
            }

            pub fn kind(&self) -> TopicKind {
                match self {
                    $( Topic::$eu => TopicKind::$eu, )*
                    $( Topic::$ep(_) => TopicKind::$ep, )*
                    $( Topic::$su => TopicKind::$su, )*
                    $( Topic::$sp(_) => TopicKind::$sp, )*
                    $( Topic::$pu => TopicKind::$pu, )*
                    $( Topic::$pp(_) => TopicKind::$pp, )*
                }
            }

            /// Extract the declared key fields from this topic's payload,
            /// stringified via `Display` in declaration order. Returns an
            /// empty vec for variants without `keys = [...]`.
            #[allow(unused_variables)]
            pub fn keys_for(&self) -> Vec<String> {
                match self {
                    $( Topic::$sp(payload) => vec![ $( payload.$spk.to_string(), )* ], )*
                    $( Topic::$pp(payload) => vec![ $( payload.$ppk.to_string(), )* ], )*
                    _ => Vec::new(),
                }
            }

            /// Resolved on-disk path for this topic's persistent storage.
            /// For topics without a `namespace` annotation, returns the
            /// shared `~/.config/sola/state.yaml` path. For namespaced
            /// topics, returns `~/.config/sola/<interpolated>.yaml`.
            ///
            /// Panics if an interpolated key value is unsafe (slash,
            /// `..`, empty). Use `path_for_safe` for a typed error.
            pub fn path_for(&self) -> std::path::PathBuf {
                self.path_for_safe()
                    .expect("invalid key segment in namespace interpolation")
            }

            /// Same as `path_for`, but returns `Err(PathError)` when an
            /// interpolated key value is unsafe (contains `/`, `\0`,
            /// `..`, or is empty), or when the namespace template
            /// references an unknown placeholder.
            pub fn path_for_safe(&self) -> Result<std::path::PathBuf, $crate::topic::PathError> {
                let kind = self.kind();
                let cfg = sola_core::config::sola_config_dir();
                match kind.namespace() {
                    None => Ok(cfg.join("state.yaml")),
                    Some(template) => {
                        let names = kind.key_names();
                        let values = self.keys_for();
                        let resolved = $crate::topic::interpolate_namespace(template, names, &values)?;
                        Ok(cfg.join(format!("{resolved}.yaml")))
                    }
                }
            }

            /// Serialize the topic's payload to a JSON value. Unit
            /// variants produce `Value::Null`. Used by sola-monitor and
            /// any other consumer that wants to surface raw bus traffic
            /// to a WebView. Note: encrypted payload fields (e.g.
            /// `Encrypted<T>`) serialize as the `age1enc:...` ciphertext
            /// here — JSON is human-readable.
            pub fn to_json_value(&self) -> serde_json::Value {
                match self {
                    $( Topic::$eu => serde_json::Value::Null, )*
                    $( Topic::$ep(payload) => {
                        serde_json::to_value(payload).unwrap_or(serde_json::Value::Null)
                    }, )*
                    $( Topic::$su => serde_json::Value::Null, )*
                    $( Topic::$sp(payload) => {
                        serde_json::to_value(payload).unwrap_or(serde_json::Value::Null)
                    }, )*
                    $( Topic::$pu => serde_json::Value::Null, )*
                    $( Topic::$pp(payload) => {
                        serde_json::to_value(payload).unwrap_or(serde_json::Value::Null)
                    }, )*
                }
            }

            /// Build a `Topic` from a kind plus a JSON payload value.
            /// The inverse of `to_json_value`. Unit variants ignore the
            /// payload (any value is accepted). Payload variants are
            /// deserialized via `serde_json::from_value`; on schema
            /// mismatch, returns `None`.
            ///
            /// Used by `solactl emit` to construct topics from CLI
            /// arguments without per-variant client-side code.
            pub fn from_json_kind(kind: TopicKind, value: serde_json::Value) -> Option<Topic> {
                let _ = &value; // unused for kinds with no payload variants
                match kind {
                    $( TopicKind::$eu => Some(Topic::$eu), )*
                    $( TopicKind::$ep => {
                        serde_json::from_value::<$ept>(value.clone()).ok().map(Topic::$ep)
                    }, )*
                    $( TopicKind::$su => Some(Topic::$su), )*
                    $( TopicKind::$sp => {
                        serde_json::from_value::<$spt>(value.clone()).ok().map(Topic::$sp)
                    }, )*
                    $( TopicKind::$pu => Some(Topic::$pu), )*
                    $( TopicKind::$pp => {
                        serde_json::from_value::<$ppt>(value.clone()).ok().map(Topic::$pp)
                    }, )*
                }
            }

            /// Serialize a persistent topic's payload to a YAML value
            /// suitable for writing to `state.yaml`. Returns `None` for
            /// non-persistent variants (ephemeral / sticky topics never
            /// touch disk) and for persistent payloads whose shape
            /// can't be represented in YAML.
            #[allow(unreachable_patterns, unused_variables)]
            pub fn to_yaml_value(&self) -> Option<serde_yaml_ng::Value> {
                match self {
                    $( Topic::$pp(payload) => $crate::topic::payload_to_yaml(payload), )*
                    $( Topic::$pu => Some($crate::topic::empty_yaml_section()), )*
                    _ => None,
                }
            }

            /// Deserialize a `state.yaml` section into the matching
            /// persistent topic variant. Returns `None` if `kind` is
            /// not persistent, or if the YAML value can't be
            /// deserialized into the expected payload type.
            #[allow(unreachable_patterns, unused_variables)]
            pub fn from_yaml_section(kind: TopicKind, value: serde_yaml_ng::Value) -> Option<Topic> {
                match kind {
                    $( TopicKind::$pp => {
                        $crate::topic::payload_from_yaml::<$ppt>(value).map(Topic::$pp)
                    }, )*
                    $( TopicKind::$pu => Some(Topic::$pu), )*
                    _ => None,
                }
            }
        }
    };
}

#[cfg(test)]
mod tests {
    use serde::{Deserialize, Serialize};

    #[derive(Debug, Clone, Serialize, Deserialize)]
    pub struct StickyKeyed {
        pub id: String,
        pub other: u32,
    }

    #[derive(Debug, Clone, Serialize, Deserialize)]
    pub struct StickyMultiKey {
        pub window_id: u32,
        pub menu_id: String,
    }

    crate::define_topics! {
        Plain(String),
        #[sticky]
        Bare(String),
        #[sticky(keys = [id])]
        Single(StickyKeyed),
        #[persistent(keys = [window_id, menu_id])]
        Multi(StickyMultiKey),
        #[persistent]
        UnnamedSingleton(StickyKeyed),
        #[persistent(namespace = "ns/single")]
        NamespacedSingleton(StickyKeyed),
        #[persistent(keys = [id], namespace = "ns/keyed/:id")]
        NamespacedKeyed(StickyKeyed),
    }

    #[test]
    fn keys_for_unkeyed_returns_empty() {
        let t = Topic::Plain("hi".into());
        assert!(t.keys_for().is_empty());
        let t = Topic::Bare("hi".into());
        assert!(t.keys_for().is_empty());
    }

    #[test]
    fn keys_for_single_key_extracts_field() {
        let t = Topic::Single(StickyKeyed {
            id: "abc".into(),
            other: 7,
        });
        assert_eq!(t.keys_for(), vec!["abc".to_string()]);
    }

    #[test]
    fn keys_for_multi_key_extracts_in_declaration_order() {
        let t = Topic::Multi(StickyMultiKey {
            window_id: 42,
            menu_id: "file".into(),
        });
        assert_eq!(t.keys_for(), vec!["42".to_string(), "file".to_string()]);
    }

    #[test]
    fn topic_kind_has_keys_reflects_declaration() {
        assert!(!TopicKind::Plain.has_keys());
        assert!(!TopicKind::Bare.has_keys());
        assert!(TopicKind::Single.has_keys());
        assert!(TopicKind::Multi.has_keys());
    }

    #[test]
    fn namespace_returns_some_only_when_declared() {
        assert_eq!(TopicKind::UnnamedSingleton.namespace(), None);
        assert_eq!(
            TopicKind::NamespacedSingleton.namespace(),
            Some("ns/single")
        );
        assert_eq!(TopicKind::NamespacedKeyed.namespace(), Some("ns/keyed/:id"));
        // Non-persistent kinds always None.
        assert_eq!(TopicKind::Plain.namespace(), None);
        assert_eq!(TopicKind::Bare.namespace(), None);
        assert_eq!(TopicKind::Single.namespace(), None);
        // Existing persistent without namespace.
        assert_eq!(TopicKind::Multi.namespace(), None);
    }

    #[test]
    fn key_names_in_declaration_order() {
        assert!(TopicKind::Plain.key_names().is_empty());
        assert!(TopicKind::UnnamedSingleton.key_names().is_empty());
        assert!(TopicKind::NamespacedSingleton.key_names().is_empty());
        assert_eq!(TopicKind::Single.key_names(), &["id"]);
        assert_eq!(TopicKind::NamespacedKeyed.key_names(), &["id"]);
        assert_eq!(TopicKind::Multi.key_names(), &["window_id", "menu_id"]);
    }

    #[test]
    fn path_for_no_namespace_falls_back_to_state_yaml() {
        let t = Topic::UnnamedSingleton(StickyKeyed {
            id: "x".into(),
            other: 1,
        });
        let p = t.path_for();
        assert_eq!(p, sola_core::config::sola_config_dir().join("state.yaml"));
    }

    #[test]
    fn path_for_singleton_namespaced() {
        let t = Topic::NamespacedSingleton(StickyKeyed {
            id: "x".into(),
            other: 1,
        });
        let p = t.path_for();
        assert_eq!(
            p,
            sola_core::config::sola_config_dir().join("ns/single.yaml")
        );
    }

    #[test]
    fn path_for_keyed_namespaced_interpolates() {
        let t = Topic::NamespacedKeyed(StickyKeyed {
            id: "abc".into(),
            other: 1,
        });
        let p = t.path_for();
        assert_eq!(
            p,
            sola_core::config::sola_config_dir().join("ns/keyed/abc.yaml")
        );
    }

    #[test]
    fn path_for_safe_rejects_path_traversal() {
        let t = Topic::NamespacedKeyed(StickyKeyed {
            id: "../escape".into(),
            other: 1,
        });
        assert!(matches!(
            t.path_for_safe(),
            Err(super::PathError::Forbidden(_, _))
        ));
    }

    #[test]
    fn path_for_safe_rejects_forward_slash() {
        let t = Topic::NamespacedKeyed(StickyKeyed {
            id: "a/b".into(),
            other: 1,
        });
        assert!(matches!(
            t.path_for_safe(),
            Err(super::PathError::Forbidden(_, _))
        ));
    }

    #[test]
    fn path_for_safe_rejects_empty_key() {
        let t = Topic::NamespacedKeyed(StickyKeyed {
            id: "".into(),
            other: 1,
        });
        assert!(matches!(t.path_for_safe(), Err(super::PathError::Empty(_))));
    }

    #[test]
    fn interpolate_namespace_passes_through_when_no_placeholder() {
        let r = super::interpolate_namespace("plain/path", &[], &[]).unwrap();
        assert_eq!(r, "plain/path");
    }

    #[test]
    fn interpolate_namespace_replaces_known_keys() {
        let r =
            super::interpolate_namespace("a/:x/b/:y", &["x", "y"], &["one".into(), "two".into()])
                .unwrap();
        assert_eq!(r, "a/one/b/two");
    }
}
