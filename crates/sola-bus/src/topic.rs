use std::io;

use serde::Deserialize;

use crate::Message;

/// Decode a payload from a `Message` into the expected type.
pub fn decode_payload<T: for<'de> Deserialize<'de>>(msg: &Message) -> io::Result<T> {
    let bytes = msg
        .payload
        .as_ref()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "missing payload"))?;
    postcard::from_bytes(bytes).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))
}

/// Serialize a value to a payload byte vec.
pub fn encode_payload<T: serde::Serialize>(value: &T) -> Vec<u8> {
    postcard::to_allocvec(value).expect("failed to serialize topic payload")
}

/// Convert a persistent topic's typed payload to a TOML value for
/// storage in `state.toml`. Returns `None` if the payload can't be
/// represented in TOML (e.g. non-string map keys). Used by the macro-
/// generated `Topic::to_toml_value`.
pub fn payload_to_toml<T: serde::Serialize>(value: &T) -> Option<toml::Value> {
    toml::Value::try_from(value).ok()
}

/// Deserialize a `state.toml` section into a topic payload. Returns
/// `None` on schema mismatch — the bus logs and leaves the topic
/// unset. Used by the macro-generated `Topic::from_toml_section`.
pub fn payload_from_toml<T: serde::de::DeserializeOwned>(value: toml::Value) -> Option<T> {
    value.try_into::<T>().ok()
}

/// Empty TOML table. Used as the serialized form of a persistent unit
/// variant (presence-only persistent signal; section exists in the
/// file but carries no data).
pub fn empty_toml_section() -> toml::Value {
    toml::Value::Table(toml::map::Map::new())
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

/// Define a Topic enum with typed variants, a parse function, and to_message.
///
/// Variant forms:
/// - Unit: `Shutdown,` — no payload
/// - Payload: `GrabInput(String),` — carries typed data
///
/// Optional per-variant attributes declare delivery behavior:
/// - no attribute → ephemeral (default)
/// - `#[sticky]` → latest value retained and replayed to new subscribers
/// - `#[persistent]` → sticky + saved to disk (not yet wired; Phase 1 only adds metadata)
///
/// # Example
/// ```ignore
/// define_topics! {
///     Shutdown,
///     GrabInput(String),
///     #[sticky]
///     Windows(Vec<Window>),
///     #[persistent]
///     Zones(HashMap<String, Zone>),
/// }
/// ```
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
    // --- #[persistent] payload variants ---
    ( [ $($eu:ident)* ] [ $($ep:ident($ept:ty))* ]
      [ $($su:ident)* ] [ $($sp:ident($spt:ty))* ]
      [ $($pu:ident)* ] [ $($pp:ident($ppt:ty))* ]
      #[persistent] $name:ident ( $payload:ty ), $($rest:tt)* ) => {
        $crate::_define_topics_inner!{
            [ $($eu)* ] [ $($ep($ept))* ]
            [ $($su)* ] [ $($sp($spt))* ]
            [ $($pu)* ] [ $($pp($ppt))* $name($payload) ]
            $($rest)*
        }
    };
    ( [ $($eu:ident)* ] [ $($ep:ident($ept:ty))* ]
      [ $($su:ident)* ] [ $($sp:ident($spt:ty))* ]
      [ $($pu:ident)* ] [ $($pp:ident($ppt:ty))* ]
      #[persistent] $name:ident ( $payload:ty ) ) => {
        $crate::_define_topics_inner!{
            [ $($eu)* ] [ $($ep($ept))* ]
            [ $($su)* ] [ $($sp($spt))* ]
            [ $($pu)* ] [ $($pp($ppt))* $name($payload) ]
        }
    };

    // --- #[persistent] unit variants ---
    ( [ $($eu:ident)* ] [ $($ep:ident($ept:ty))* ]
      [ $($su:ident)* ] [ $($sp:ident($spt:ty))* ]
      [ $($pu:ident)* ] [ $($pp:ident($ppt:ty))* ]
      #[persistent] $name:ident, $($rest:tt)* ) => {
        $crate::_define_topics_inner!{
            [ $($eu)* ] [ $($ep($ept))* ]
            [ $($su)* ] [ $($sp($spt))* ]
            [ $($pu)* $name ] [ $($pp($ppt))* ]
            $($rest)*
        }
    };
    ( [ $($eu:ident)* ] [ $($ep:ident($ept:ty))* ]
      [ $($su:ident)* ] [ $($sp:ident($spt:ty))* ]
      [ $($pu:ident)* ] [ $($pp:ident($ppt:ty))* ]
      #[persistent] $name:ident ) => {
        $crate::_define_topics_inner!{
            [ $($eu)* ] [ $($ep($ept))* ]
            [ $($su)* ] [ $($sp($spt))* ]
            [ $($pu)* $name ] [ $($pp($ppt))* ]
        }
    };

    // --- #[sticky] payload variants ---
    ( [ $($eu:ident)* ] [ $($ep:ident($ept:ty))* ]
      [ $($su:ident)* ] [ $($sp:ident($spt:ty))* ]
      [ $($pu:ident)* ] [ $($pp:ident($ppt:ty))* ]
      #[sticky] $name:ident ( $payload:ty ), $($rest:tt)* ) => {
        $crate::_define_topics_inner!{
            [ $($eu)* ] [ $($ep($ept))* ]
            [ $($su)* ] [ $($sp($spt))* $name($payload) ]
            [ $($pu)* ] [ $($pp($ppt))* ]
            $($rest)*
        }
    };
    ( [ $($eu:ident)* ] [ $($ep:ident($ept:ty))* ]
      [ $($su:ident)* ] [ $($sp:ident($spt:ty))* ]
      [ $($pu:ident)* ] [ $($pp:ident($ppt:ty))* ]
      #[sticky] $name:ident ( $payload:ty ) ) => {
        $crate::_define_topics_inner!{
            [ $($eu)* ] [ $($ep($ept))* ]
            [ $($su)* ] [ $($sp($spt))* $name($payload) ]
            [ $($pu)* ] [ $($pp($ppt))* ]
        }
    };

    // --- #[sticky] unit variants ---
    ( [ $($eu:ident)* ] [ $($ep:ident($ept:ty))* ]
      [ $($su:ident)* ] [ $($sp:ident($spt:ty))* ]
      [ $($pu:ident)* ] [ $($pp:ident($ppt:ty))* ]
      #[sticky] $name:ident, $($rest:tt)* ) => {
        $crate::_define_topics_inner!{
            [ $($eu)* ] [ $($ep($ept))* ]
            [ $($su)* $name ] [ $($sp($spt))* ]
            [ $($pu)* ] [ $($pp($ppt))* ]
            $($rest)*
        }
    };
    ( [ $($eu:ident)* ] [ $($ep:ident($ept:ty))* ]
      [ $($su:ident)* ] [ $($sp:ident($spt:ty))* ]
      [ $($pu:ident)* ] [ $($pp:ident($ppt:ty))* ]
      #[sticky] $name:ident ) => {
        $crate::_define_topics_inner!{
            [ $($eu)* ] [ $($ep($ept))* ]
            [ $($su)* $name ] [ $($sp($spt))* ]
            [ $($pu)* ] [ $($pp($ppt))* ]
        }
    };

    // --- Ephemeral (unannotated) payload variants ---
    ( [ $($eu:ident)* ] [ $($ep:ident($ept:ty))* ]
      [ $($su:ident)* ] [ $($sp:ident($spt:ty))* ]
      [ $($pu:ident)* ] [ $($pp:ident($ppt:ty))* ]
      $name:ident ( $payload:ty ), $($rest:tt)* ) => {
        $crate::_define_topics_inner!{
            [ $($eu)* ] [ $($ep($ept))* $name($payload) ]
            [ $($su)* ] [ $($sp($spt))* ]
            [ $($pu)* ] [ $($pp($ppt))* ]
            $($rest)*
        }
    };
    ( [ $($eu:ident)* ] [ $($ep:ident($ept:ty))* ]
      [ $($su:ident)* ] [ $($sp:ident($spt:ty))* ]
      [ $($pu:ident)* ] [ $($pp:ident($ppt:ty))* ]
      $name:ident ( $payload:ty ) ) => {
        $crate::_define_topics_inner!{
            [ $($eu)* ] [ $($ep($ept))* $name($payload) ]
            [ $($su)* ] [ $($sp($spt))* ]
            [ $($pu)* ] [ $($pp($ppt))* ]
        }
    };

    // --- Ephemeral (unannotated) unit variants ---
    ( [ $($eu:ident)* ] [ $($ep:ident($ept:ty))* ]
      [ $($su:ident)* ] [ $($sp:ident($spt:ty))* ]
      [ $($pu:ident)* ] [ $($pp:ident($ppt:ty))* ]
      $name:ident, $($rest:tt)* ) => {
        $crate::_define_topics_inner!{
            [ $($eu)* $name ] [ $($ep($ept))* ]
            [ $($su)* ] [ $($sp($spt))* ]
            [ $($pu)* ] [ $($pp($ppt))* ]
            $($rest)*
        }
    };
    ( [ $($eu:ident)* ] [ $($ep:ident($ept:ty))* ]
      [ $($su:ident)* ] [ $($sp:ident($spt:ty))* ]
      [ $($pu:ident)* ] [ $($pp:ident($ppt:ty))* ]
      $name:ident ) => {
        $crate::_define_topics_inner!{
            [ $($eu)* $name ] [ $($ep($ept))* ]
            [ $($su)* ] [ $($sp($spt))* ]
            [ $($pu)* ] [ $($pp($ppt))* ]
        }
    };

    // --- Terminal: generate Topic, TopicKind, Behavior wiring ---
    ( [ $($eu:ident)* ] [ $($ep:ident($ept:ty))* ]
      [ $($su:ident)* ] [ $($sp:ident($spt:ty))* ]
      [ $($pu:ident)* ] [ $($pp:ident($ppt:ty))* ]
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

            /// Map a section name (as it appears in `state.toml`) back
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
                match self {
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
                }
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
            /// Used by `sola-debug emit` to construct topics from CLI
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

            /// Serialize a persistent topic's payload to a TOML value
            /// suitable for writing to `state.toml`. Returns `None` for
            /// non-persistent variants (ephemeral / sticky topics never
            /// touch disk) and for persistent payloads whose shape
            /// can't be represented in TOML.
            #[allow(unreachable_patterns, unused_variables)]
            pub fn to_toml_value(&self) -> Option<toml::Value> {
                match self {
                    $( Topic::$pp(payload) => $crate::topic::payload_to_toml(payload), )*
                    $( Topic::$pu => Some($crate::topic::empty_toml_section()), )*
                    _ => None,
                }
            }

            /// Deserialize a `state.toml` section into the matching
            /// persistent topic variant. Returns `None` if `kind` is
            /// not persistent, or if the TOML value can't be
            /// deserialized into the expected payload type.
            #[allow(unreachable_patterns, unused_variables)]
            pub fn from_toml_section(kind: TopicKind, value: toml::Value) -> Option<Topic> {
                match kind {
                    $( TopicKind::$pp => {
                        $crate::topic::payload_from_toml::<$ppt>(value).map(Topic::$pp)
                    }, )*
                    $( TopicKind::$pu => Some(Topic::$pu), )*
                    _ => None,
                }
            }
        }
    };
}
