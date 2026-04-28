# Keyed Stickies + Retract Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Extend `sola-bus` so a `#[sticky]` or `#[persistent]` topic can declare key fields, the bus stores stickies by `(topic, keys)`, and clients can `retract(topic)` symmetrically with `emit(topic)`. Subscribers receive a `Delivery { topic, retracted }` so they can branch on add/remove.

**Architecture:** Threading approach. (1) `Message` grows a `keys: Vec<String>` field; the `sticky` bit is reused as the retract signal for sticky/persistent topic kinds. (2) The `define_topics!` macro accepts `keys = ["field"]` and generates a `Topic::keys_for(&self) -> Vec<String>` extractor that stringifies each named field via `Display`. (3) The bus host's sticky map becomes `HashMap<(String, Vec<String>), Message>`. (4) The framework's `BusHandler` signature changes from `(&mut Self, &Topic, &mut AppCtx)` to `(&mut Self, &Delivery, &mut AppCtx)`; every in-tree handler is migrated. (5) Disk persistence picks `[Section]` (no keys) vs `[[Section]]` (keyed) per topic.

**Tech Stack:** Rust 2021, `macro_rules!`, postcard wire format, TOML on-disk, `tracing` for diagnostics, `tempfile` for tests.

**Reference spec:** `docs/specs/2026-04-27-keyed-stickies-design.md` (commit `af35394`).

---

## File Structure

**Modify:**
- `crates/sola-bus/src/message.rs` — add `keys` field + constructors
- `crates/sola-bus/src/topic.rs` — extend `define_topics!` macro to accept `keys = [...]`; generate `Topic::keys_for` and `TopicKind::has_keys`
- `crates/sola-bus/src/topics.rs` — re-key `SetAppMenu` to `#[sticky(keys = [app_id])]`
- `crates/sola-bus/src/registry.rs` — change `BusHandler` type alias and `dispatch` signature
- `crates/sola-bus/src/client.rs` — add `BusClient::retract(topic)`
- `crates/sola-bus/src/main.rs` — change sticky map type; drop bootstrap eviction; add retract handling; wire disk retract
- `crates/sola-bus/src/state.rs` — load/write `[[Section]]` for keyed kinds; add `retract_section`
- `crates/sola-bus/src/lib.rs` — export `Delivery`
- `crates/sola-app/src/lib.rs` — wrap `Delivery` in dispatch loop; update framework `CloseApp` handler
- `crates/sola-shell/src/app.rs` — migrate 11 handlers to `&Delivery`
- `crates/sola-settings/src/main.rs` — migrate 4 handlers
- `crates/sola-monitor/src/main.rs` — migrate `on_menu_action`

**Create:** none — all changes land in existing files.

---

## Task 1: Add `keys: Vec<String>` field to `Message`

**Files:**
- Modify: `crates/sola-bus/src/message.rs`

- [ ] **Step 1: Read current Message struct**

```bash
cat crates/sola-bus/src/message.rs
```

- [ ] **Step 2: Write failing postcard roundtrip test**

Append to `crates/sola-bus/src/message.rs` (replace the existing `tests` mod if present, otherwise add):

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn message_keys_default_empty_and_roundtrips() {
        let mut msg = Message::new("Foo");
        msg.keys = vec!["abc".to_string(), "def".to_string()];
        let bytes = postcard::to_allocvec(&msg).unwrap();
        let back: Message = postcard::from_bytes(&bytes).unwrap();
        assert_eq!(back.keys, vec!["abc".to_string(), "def".to_string()]);
    }

    #[test]
    fn message_new_starts_with_empty_keys() {
        let msg = Message::new("Foo");
        assert!(msg.keys.is_empty());
    }

    #[test]
    fn message_with_payload_starts_with_empty_keys() {
        let msg = Message::with_payload("Foo", vec![1, 2, 3]);
        assert!(msg.keys.is_empty());
    }
}
```

- [ ] **Step 3: Run test to verify failure**

```bash
cargo test -p sola-bus --lib message::tests 2>&1 | head -40
```

Expected: FAIL with `no field 'keys'`.

- [ ] **Step 4: Add `keys` field and update constructors**

Edit `crates/sola-bus/src/message.rs` — add `keys: Vec<String>` to the struct (after `source`), update both `Message::new` and `Message::with_payload` to initialize `keys: Vec::new()`. Field order matters because `postcard` is positional: `keys` MUST be the last field so older serialized buffers (none yet, but defensively) wouldn't conflict. Drop any `#[serde(default)]` on `source` if present — the spec says "no Option, no serde(default)" for the new wire format, but keep `#[serde(default)]` on `keys` is unnecessary since this is the first version.

Actual struct shape after edit:

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    pub id: Uuid,
    pub topic: String,
    pub payload: Option<Vec<u8>>,
    pub sticky: bool,
    pub source: String,
    pub keys: Vec<String>,
}
```

Update `Message::new`:

```rust
impl Message {
    pub fn new(topic: impl Into<String>) -> Self {
        Self {
            id: Uuid::now_v7(),
            topic: topic.into(),
            payload: None,
            sticky: false,
            source: String::new(),
            keys: Vec::new(),
        }
    }

    pub fn with_payload(topic: impl Into<String>, payload: Vec<u8>) -> Self {
        Self {
            id: Uuid::now_v7(),
            topic: topic.into(),
            payload: Some(payload),
            sticky: false,
            source: String::new(),
            keys: Vec::new(),
        }
    }
}
```

Note: keep `Message::timestamp_ms` and any other existing methods untouched.

- [ ] **Step 5: Run test to verify passes**

```bash
cargo test -p sola-bus --lib message::tests 2>&1 | head -40
```

Expected: 3 passed.

- [ ] **Step 6: Build whole bus crate**

```bash
cargo build -p sola-bus 2>&1 | tail -30
```

Expected: builds clean (Message field changes propagate; nothing else uses `keys` yet).

- [ ] **Step 7: Commit**

```bash
git add crates/sola-bus/src/message.rs
git commit -m "feat(sola-bus): add keys field to Message"
```

---

## Task 2: Extend `define_topics!` macro to accept `keys = [...]`

**Files:**
- Modify: `crates/sola-bus/src/topic.rs`

This task threads an optional list of key-field idents through the macro's bucket accumulators and generates a `Topic::keys_for(&self) -> Vec<String>` method plus `TopicKind::has_keys(self) -> bool`. Only sticky and persistent payload variants accept keys; unit variants and ephemerals don't.

- [ ] **Step 1: Read the current macro**

```bash
sed -n '93,250p' crates/sola-bus/src/topic.rs
```

The bucket layout is six accumulators: ephemeral units `[$($eu)*]`, ephemeral payloads `[$($ep:ident($ept:ty))*]`, sticky units, sticky payloads, persistent units, persistent payloads.

- [ ] **Step 2: Write failing test**

Append to `crates/sola-bus/src/topic.rs` (under `#[cfg(test)]`):

```rust
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
        #[sticky(keys = ["id"])]
        Single(StickyKeyed),
        #[persistent(keys = ["window_id", "menu_id"])]
        Multi(StickyMultiKey),
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
        let t = Topic::Single(StickyKeyed { id: "abc".into(), other: 7 });
        assert_eq!(t.keys_for(), vec!["abc".to_string()]);
    }

    #[test]
    fn keys_for_multi_key_extracts_in_declaration_order() {
        let t = Topic::Multi(StickyMultiKey { window_id: 42, menu_id: "file".into() });
        assert_eq!(t.keys_for(), vec!["42".to_string(), "file".to_string()]);
    }

    #[test]
    fn topic_kind_has_keys_reflects_declaration() {
        assert!(!TopicKind::Plain.has_keys());
        assert!(!TopicKind::Bare.has_keys());
        assert!(TopicKind::Single.has_keys());
        assert!(TopicKind::Multi.has_keys());
    }
}
```

- [ ] **Step 3: Run test to verify failure**

```bash
cargo test -p sola-bus --lib topic::tests 2>&1 | head -50
```

Expected: macro fails to parse `#[sticky(keys = …)]` form.

- [ ] **Step 4: Extend macro accumulators to carry keys**

Replace the entire body of `_define_topics_inner!` in `crates/sola-bus/src/topic.rs` with the version below. The shape change: sticky-payload and persistent-payload buckets become `[$($name:ident($type:ty) keys=[$($k:ident)*])* ]`. Existing arms that match `#[sticky] $name($type)` push with empty `keys=[]`; new arms that match `#[sticky(keys = [...])] $name($type)` push with the captured idents. Ephemeral and unit variants are unchanged.

```rust
#[macro_export]
#[doc(hidden)]
macro_rules! _define_topics_inner {
    // --- #[persistent(keys = [...])] payload variants ---
    ( [ $($eu:ident)* ] [ $($ep:ident($ept:ty))* ]
      [ $($su:ident)* ] [ $($sp:ident($spt:ty) keys=[$($spk:ident)*])* ]
      [ $($pu:ident)* ] [ $($pp:ident($ppt:ty) keys=[$($ppk:ident)*])* ]
      #[persistent(keys = [ $($k:ident),+ $(,)? ])] $name:ident ( $payload:ty ), $($rest:tt)* ) => {
        $crate::_define_topics_inner!{
            [ $($eu)* ] [ $($ep($ept))* ]
            [ $($su)* ] [ $($sp($spt) keys=[$($spk)*])* ]
            [ $($pu)* ] [ $($pp($ppt) keys=[$($ppk)*])* $name($payload) keys=[$($k)*] ]
            $($rest)*
        }
    };
    ( [ $($eu:ident)* ] [ $($ep:ident($ept:ty))* ]
      [ $($su:ident)* ] [ $($sp:ident($spt:ty) keys=[$($spk:ident)*])* ]
      [ $($pu:ident)* ] [ $($pp:ident($ppt:ty) keys=[$($ppk:ident)*])* ]
      #[persistent(keys = [ $($k:ident),+ $(,)? ])] $name:ident ( $payload:ty ) ) => {
        $crate::_define_topics_inner!{
            [ $($eu)* ] [ $($ep($ept))* ]
            [ $($su)* ] [ $($sp($spt) keys=[$($spk)*])* ]
            [ $($pu)* ] [ $($pp($ppt) keys=[$($ppk)*])* $name($payload) keys=[$($k)*] ]
        }
    };

    // --- #[persistent] payload variants (no keys) ---
    ( [ $($eu:ident)* ] [ $($ep:ident($ept:ty))* ]
      [ $($su:ident)* ] [ $($sp:ident($spt:ty) keys=[$($spk:ident)*])* ]
      [ $($pu:ident)* ] [ $($pp:ident($ppt:ty) keys=[$($ppk:ident)*])* ]
      #[persistent] $name:ident ( $payload:ty ), $($rest:tt)* ) => {
        $crate::_define_topics_inner!{
            [ $($eu)* ] [ $($ep($ept))* ]
            [ $($su)* ] [ $($sp($spt) keys=[$($spk)*])* ]
            [ $($pu)* ] [ $($pp($ppt) keys=[$($ppk)*])* $name($payload) keys=[] ]
            $($rest)*
        }
    };
    ( [ $($eu:ident)* ] [ $($ep:ident($ept:ty))* ]
      [ $($su:ident)* ] [ $($sp:ident($spt:ty) keys=[$($spk:ident)*])* ]
      [ $($pu:ident)* ] [ $($pp:ident($ppt:ty) keys=[$($ppk:ident)*])* ]
      #[persistent] $name:ident ( $payload:ty ) ) => {
        $crate::_define_topics_inner!{
            [ $($eu)* ] [ $($ep($ept))* ]
            [ $($su)* ] [ $($sp($spt) keys=[$($spk)*])* ]
            [ $($pu)* ] [ $($pp($ppt) keys=[$($ppk)*])* $name($payload) keys=[] ]
        }
    };

    // --- #[persistent] unit variants (keys not allowed on units) ---
    ( [ $($eu:ident)* ] [ $($ep:ident($ept:ty))* ]
      [ $($su:ident)* ] [ $($sp:ident($spt:ty) keys=[$($spk:ident)*])* ]
      [ $($pu:ident)* ] [ $($pp:ident($ppt:ty) keys=[$($ppk:ident)*])* ]
      #[persistent] $name:ident, $($rest:tt)* ) => {
        $crate::_define_topics_inner!{
            [ $($eu)* ] [ $($ep($ept))* ]
            [ $($su)* ] [ $($sp($spt) keys=[$($spk)*])* ]
            [ $($pu)* $name ] [ $($pp($ppt) keys=[$($ppk)*])* ]
            $($rest)*
        }
    };
    ( [ $($eu:ident)* ] [ $($ep:ident($ept:ty))* ]
      [ $($su:ident)* ] [ $($sp:ident($spt:ty) keys=[$($spk:ident)*])* ]
      [ $($pu:ident)* ] [ $($pp:ident($ppt:ty) keys=[$($ppk:ident)*])* ]
      #[persistent] $name:ident ) => {
        $crate::_define_topics_inner!{
            [ $($eu)* ] [ $($ep($ept))* ]
            [ $($su)* ] [ $($sp($spt) keys=[$($spk)*])* ]
            [ $($pu)* $name ] [ $($pp($ppt) keys=[$($ppk)*])* ]
        }
    };

    // --- #[sticky(keys = [...])] payload variants ---
    ( [ $($eu:ident)* ] [ $($ep:ident($ept:ty))* ]
      [ $($su:ident)* ] [ $($sp:ident($spt:ty) keys=[$($spk:ident)*])* ]
      [ $($pu:ident)* ] [ $($pp:ident($ppt:ty) keys=[$($ppk:ident)*])* ]
      #[sticky(keys = [ $($k:ident),+ $(,)? ])] $name:ident ( $payload:ty ), $($rest:tt)* ) => {
        $crate::_define_topics_inner!{
            [ $($eu)* ] [ $($ep($ept))* ]
            [ $($su)* ] [ $($sp($spt) keys=[$($spk)*])* $name($payload) keys=[$($k)*] ]
            [ $($pu)* ] [ $($pp($ppt) keys=[$($ppk)*])* ]
            $($rest)*
        }
    };
    ( [ $($eu:ident)* ] [ $($ep:ident($ept:ty))* ]
      [ $($su:ident)* ] [ $($sp:ident($spt:ty) keys=[$($spk:ident)*])* ]
      [ $($pu:ident)* ] [ $($pp:ident($ppt:ty) keys=[$($ppk:ident)*])* ]
      #[sticky(keys = [ $($k:ident),+ $(,)? ])] $name:ident ( $payload:ty ) ) => {
        $crate::_define_topics_inner!{
            [ $($eu)* ] [ $($ep($ept))* ]
            [ $($su)* ] [ $($sp($spt) keys=[$($spk)*])* $name($payload) keys=[$($k)*] ]
            [ $($pu)* ] [ $($pp($ppt) keys=[$($ppk)*])* ]
        }
    };

    // --- #[sticky] payload variants (no keys) ---
    ( [ $($eu:ident)* ] [ $($ep:ident($ept:ty))* ]
      [ $($su:ident)* ] [ $($sp:ident($spt:ty) keys=[$($spk:ident)*])* ]
      [ $($pu:ident)* ] [ $($pp:ident($ppt:ty) keys=[$($ppk:ident)*])* ]
      #[sticky] $name:ident ( $payload:ty ), $($rest:tt)* ) => {
        $crate::_define_topics_inner!{
            [ $($eu)* ] [ $($ep($ept))* ]
            [ $($su)* ] [ $($sp($spt) keys=[$($spk)*])* $name($payload) keys=[] ]
            [ $($pu)* ] [ $($pp($ppt) keys=[$($ppk)*])* ]
            $($rest)*
        }
    };
    ( [ $($eu:ident)* ] [ $($ep:ident($ept:ty))* ]
      [ $($su:ident)* ] [ $($sp:ident($spt:ty) keys=[$($spk:ident)*])* ]
      [ $($pu:ident)* ] [ $($pp:ident($ppt:ty) keys=[$($ppk:ident)*])* ]
      #[sticky] $name:ident ( $payload:ty ) ) => {
        $crate::_define_topics_inner!{
            [ $($eu)* ] [ $($ep($ept))* ]
            [ $($su)* ] [ $($sp($spt) keys=[$($spk)*])* $name($payload) keys=[] ]
            [ $($pu)* ] [ $($pp($ppt) keys=[$($ppk)*])* ]
        }
    };

    // --- #[sticky] unit variants ---
    ( [ $($eu:ident)* ] [ $($ep:ident($ept:ty))* ]
      [ $($su:ident)* ] [ $($sp:ident($spt:ty) keys=[$($spk:ident)*])* ]
      [ $($pu:ident)* ] [ $($pp:ident($ppt:ty) keys=[$($ppk:ident)*])* ]
      #[sticky] $name:ident, $($rest:tt)* ) => {
        $crate::_define_topics_inner!{
            [ $($eu)* ] [ $($ep($ept))* ]
            [ $($su)* $name ] [ $($sp($spt) keys=[$($spk)*])* ]
            [ $($pu)* ] [ $($pp($ppt) keys=[$($ppk)*])* ]
            $($rest)*
        }
    };
    ( [ $($eu:ident)* ] [ $($ep:ident($ept:ty))* ]
      [ $($su:ident)* ] [ $($sp:ident($spt:ty) keys=[$($spk:ident)*])* ]
      [ $($pu:ident)* ] [ $($pp:ident($ppt:ty) keys=[$($ppk:ident)*])* ]
      #[sticky] $name:ident ) => {
        $crate::_define_topics_inner!{
            [ $($eu)* ] [ $($ep($ept))* ]
            [ $($su)* $name ] [ $($sp($spt) keys=[$($spk)*])* ]
            [ $($pu)* ] [ $($pp($ppt) keys=[$($ppk)*])* ]
        }
    };

    // --- Ephemeral payload variants ---
    ( [ $($eu:ident)* ] [ $($ep:ident($ept:ty))* ]
      [ $($su:ident)* ] [ $($sp:ident($spt:ty) keys=[$($spk:ident)*])* ]
      [ $($pu:ident)* ] [ $($pp:ident($ppt:ty) keys=[$($ppk:ident)*])* ]
      $name:ident ( $payload:ty ), $($rest:tt)* ) => {
        $crate::_define_topics_inner!{
            [ $($eu)* ] [ $($ep($ept))* $name($payload) ]
            [ $($su)* ] [ $($sp($spt) keys=[$($spk)*])* ]
            [ $($pu)* ] [ $($pp($ppt) keys=[$($ppk)*])* ]
            $($rest)*
        }
    };
    ( [ $($eu:ident)* ] [ $($ep:ident($ept:ty))* ]
      [ $($su:ident)* ] [ $($sp:ident($spt:ty) keys=[$($spk:ident)*])* ]
      [ $($pu:ident)* ] [ $($pp:ident($ppt:ty) keys=[$($ppk:ident)*])* ]
      $name:ident ( $payload:ty ) ) => {
        $crate::_define_topics_inner!{
            [ $($eu)* ] [ $($ep($ept))* $name($payload) ]
            [ $($su)* ] [ $($sp($spt) keys=[$($spk)*])* ]
            [ $($pu)* ] [ $($pp($ppt) keys=[$($ppk)*])* ]
        }
    };

    // --- Ephemeral unit variants ---
    ( [ $($eu:ident)* ] [ $($ep:ident($ept:ty))* ]
      [ $($su:ident)* ] [ $($sp:ident($spt:ty) keys=[$($spk:ident)*])* ]
      [ $($pu:ident)* ] [ $($pp:ident($ppt:ty) keys=[$($ppk:ident)*])* ]
      $name:ident, $($rest:tt)* ) => {
        $crate::_define_topics_inner!{
            [ $($eu)* $name ] [ $($ep($ept))* ]
            [ $($su)* ] [ $($sp($spt) keys=[$($spk)*])* ]
            [ $($pu)* ] [ $($pp($ppt) keys=[$($ppk)*])* ]
            $($rest)*
        }
    };
    ( [ $($eu:ident)* ] [ $($ep:ident($ept:ty))* ]
      [ $($su:ident)* ] [ $($sp:ident($spt:ty) keys=[$($spk:ident)*])* ]
      [ $($pu:ident)* ] [ $($pp:ident($ppt:ty) keys=[$($ppk:ident)*])* ]
      $name:ident ) => {
        $crate::_define_topics_inner!{
            [ $($eu)* $name ] [ $($ep($ept))* ]
            [ $($su)* ] [ $($sp($spt) keys=[$($spk)*])* ]
            [ $($pu)* ] [ $($pp($ppt) keys=[$($ppk)*])* ]
        }
    };

    // --- Terminal: generate Topic, TopicKind, Behavior wiring ---
    ( [ $($eu:ident)* ] [ $($ep:ident($ept:ty))* ]
      [ $($su:ident)* ] [ $($sp:ident($spt:ty) keys=[$($spk:ident)*])* ]
      [ $($pu:ident)* ] [ $($pp:ident($ppt:ty) keys=[$($ppk:ident)*])* ]
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

            pub fn from_json_kind(kind: TopicKind, value: serde_json::Value) -> Option<Topic> {
                let _ = &value;
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

            #[allow(unreachable_patterns, unused_variables)]
            pub fn to_toml_value(&self) -> Option<toml::Value> {
                match self {
                    $( Topic::$pp(payload) => $crate::topic::payload_to_toml(payload), )*
                    $( Topic::$pu => Some($crate::topic::empty_toml_section()), )*
                    _ => None,
                }
            }

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
```

Note on `has_keys`: the cute `stringify!($($spk)*).is_empty()` check at compile-evaluated codegen time avoids a separate runtime predicate. For variants with no keys, `stringify!()` produces `""`; for variants with keys, it produces e.g. `"id"` or `"window_id menu_id"`. It runs at runtime but the strings are static, so the optimizer collapses it.

- [ ] **Step 5: Run tests to verify pass**

```bash
cargo test -p sola-bus --lib topic::tests 2>&1 | head -60
```

Expected: 4 passed.

- [ ] **Step 6: Build whole bus crate to confirm existing topics still parse**

```bash
cargo build -p sola-bus 2>&1 | tail -30
```

Expected: builds clean — every existing `#[sticky]` / `#[persistent]` declaration in `topics.rs` matches the no-keys arm.

- [ ] **Step 7: Commit**

```bash
git add crates/sola-bus/src/topic.rs
git commit -m "feat(sola-bus): macro accepts keys = [...] and generates keys_for"
```

---

## Task 3: Add `Delivery` struct and migrate `BusHandler` signature

**Files:**
- Modify: `crates/sola-bus/src/registry.rs`
- Modify: `crates/sola-bus/src/lib.rs`

This task introduces `Delivery<'a> { topic: &'a Topic, retracted: bool }` and changes the `BusHandler` type alias from `fn(&mut A, &Topic, &mut C)` to `fn(&mut A, &Delivery, &mut C)`. Dispatch must still wrap the topic; we'll feed `retracted=false` for now and let later tasks compute the real value.

- [ ] **Step 1: Read current registry**

```bash
cat crates/sola-bus/src/registry.rs
```

- [ ] **Step 2: Add Delivery struct + change handler signature + change dispatch**

Replace the file content of `crates/sola-bus/src/registry.rs` (preserving non-affected code). Concretely:

```rust
use std::collections::HashMap;

use crate::topics::{Topic, TopicKind};

/// A bus delivery to a subscriber. Wraps the parsed `Topic` with a
/// `retracted` flag so handlers for `#[sticky]` / `#[persistent]` topic
/// kinds can branch on add/remove. For ephemeral topic kinds,
/// `retracted` is always `false`.
#[derive(Debug)]
pub struct Delivery<'a> {
    pub topic: &'a Topic,
    pub retracted: bool,
}

pub type BusHandler<A, C> = fn(&mut A, &Delivery, &mut C);

pub struct BusRegistry<A, C> {
    handlers: HashMap<TopicKind, BusHandler<A, C>>,
    subscribe_all: bool,
}

impl<A, C> BusRegistry<A, C> {
    pub fn new() -> Self {
        Self {
            handlers: HashMap::new(),
            subscribe_all: false,
        }
    }

    pub fn on(&mut self, kind: TopicKind, handler: BusHandler<A, C>) {
        if self.handlers.insert(kind, handler).is_some() {
            if cfg!(debug_assertions) {
                panic!("duplicate bus handler for {:?}", kind);
            } else {
                tracing::warn!(?kind, "duplicate bus handler; last registration wins");
            }
        }
    }

    pub fn subscribe_all(&mut self) {
        self.subscribe_all = true;
    }

    pub fn is_subscribe_all(&self) -> bool {
        self.subscribe_all
    }

    pub fn kinds(&self) -> impl Iterator<Item = TopicKind> + '_ {
        self.handlers.keys().copied()
    }

    pub fn dispatch(&self, delivery: &Delivery, app: &mut A, ctx: &mut C) {
        if let Some(handler) = self.handlers.get(&delivery.topic.kind()) {
            handler(app, delivery, ctx);
        }
    }
}

impl<A, C> Default for BusRegistry<A, C> {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct TestApp {
        last: Option<TopicKind>,
        last_retracted: bool,
    }
    struct TestCtx;

    fn handle(app: &mut TestApp, delivery: &Delivery, _: &mut TestCtx) {
        app.last = Some(delivery.topic.kind());
        app.last_retracted = delivery.retracted;
    }

    #[test]
    fn dispatch_routes_to_registered_handler() {
        let mut reg: BusRegistry<TestApp, TestCtx> = BusRegistry::new();
        reg.on(TopicKind::Shutdown, handle);
        let mut app = TestApp { last: None, last_retracted: false };
        let mut ctx = TestCtx;
        let topic = Topic::Shutdown;
        let delivery = Delivery { topic: &topic, retracted: false };
        reg.dispatch(&delivery, &mut app, &mut ctx);
        assert_eq!(app.last, Some(TopicKind::Shutdown));
        assert!(!app.last_retracted);
    }

    #[test]
    fn dispatch_passes_retracted_flag() {
        let mut reg: BusRegistry<TestApp, TestCtx> = BusRegistry::new();
        reg.on(TopicKind::Shutdown, handle);
        let mut app = TestApp { last: None, last_retracted: false };
        let mut ctx = TestCtx;
        let topic = Topic::Shutdown;
        let delivery = Delivery { topic: &topic, retracted: true };
        reg.dispatch(&delivery, &mut app, &mut ctx);
        assert!(app.last_retracted);
    }
}
```

Verify the file already had the `kinds()` and `subscribe_all()` methods (it did — preserve them exactly as before).

- [ ] **Step 3: Re-export `Delivery` from `sola-bus`**

Edit `crates/sola-bus/src/lib.rs`. Find the line `pub use registry::{BusHandler, BusRegistry};` and change it to:

```rust
pub use registry::{BusHandler, BusRegistry, Delivery};
```

- [ ] **Step 4: Run registry tests**

```bash
cargo test -p sola-bus --lib registry::tests 2>&1 | head -40
```

Expected: 2 passed.

- [ ] **Step 5: Build sola-bus to confirm**

```bash
cargo build -p sola-bus 2>&1 | tail -30
```

Expected: clean.

- [ ] **Step 6: Commit**

```bash
git add crates/sola-bus/src/registry.rs crates/sola-bus/src/lib.rs
git commit -m "feat(sola-bus): add Delivery wrapper; change BusHandler signature"
```

---

## Task 4: Update `sola-app` dispatch loop to construct `Delivery`

**Files:**
- Modify: `crates/sola-app/src/lib.rs`

The dispatch site at line 366 calls `registry.dispatch(&topic, app, ctx)`. We need to wrap `&topic` in `Delivery` and compute `retracted = topic_kind.behavior().is_sticky() && !msg.sticky`.

- [ ] **Step 1: Read the dispatch loop area**

```bash
sed -n '290,375p' crates/sola-app/src/lib.rs
```

- [ ] **Step 2: Update the dispatch call**

In `crates/sola-app/src/lib.rs`, find the line `registry.dispatch(&topic, app, ctx);` (around line 366) and replace it with:

```rust
let retracted = topic.kind().behavior().is_sticky() && !msg.sticky;
let delivery = sola_bus::Delivery { topic: &topic, retracted };
registry.dispatch(&delivery, app, ctx);
```

Also update the framework's default `on_close_app` handler at lines 70–80 of `crates/sola-app/src/lib.rs`. The current code is:

```rust
fn on_close_app(&mut self, topic: &Topic, ctx: &mut AppCtx)
where
    Self: Sized,
{
    if let Topic::CloseApp(app_id) = topic {
        if app_id == Self::APP_ID {
            self.on_shutdown(ctx);
            ctx.shutdown();
        }
    }
}
```

Replace with:

```rust
fn on_close_app(&mut self, delivery: &sola_bus::Delivery, ctx: &mut AppCtx)
where
    Self: Sized,
{
    if let Topic::CloseApp(app_id) = delivery.topic {
        if app_id == Self::APP_ID {
            self.on_shutdown(ctx);
            ctx.shutdown();
        }
    }
}
```

- [ ] **Step 3: Update `BusHandler` type alias usage if needed**

The `BusHandler<A>` type alias re-exports already inherits the new signature from sola-bus. No change to `pub type BusHandler<A> = sola_bus::BusHandler<A, AppCtx>;`. Confirm it still compiles.

- [ ] **Step 4: Build sola-app**

```bash
cargo build -p sola-app 2>&1 | tail -40
```

Expected: builds clean if the `on_close_app` signature is correct. Errors here indicate the trait default still has the old signature — fix and rebuild.

- [ ] **Step 5: Commit**

```bash
git add crates/sola-app/src/lib.rs
git commit -m "feat(sola-app): wrap Delivery in dispatch loop; update CloseApp handler"
```

---

## Task 5: Migrate all in-tree `bus.on` handlers to `&Delivery`

**Files:**
- Modify: `crates/sola-shell/src/app.rs` (11 handlers)
- Modify: `crates/sola-settings/src/main.rs` (4 handlers)
- Modify: `crates/sola-monitor/src/main.rs` (1 handler)

Mechanical signature migration: every `fn on_xxx(&mut self, topic: &Topic, ctx: &mut AppCtx)` becomes `fn on_xxx(&mut self, delivery: &sola_bus::Delivery, ctx: &mut AppCtx)`, and the function body's first line that destructures the topic gets the `delivery.topic` indirection.

- [ ] **Step 1: Find all candidate handlers in each crate**

```bash
grep -n "fn on_[a-z_]*(&mut self, .*&Topic" crates/sola-shell/src/app.rs
grep -n "fn on_[a-z_]*(&mut self, .*&Topic" crates/sola-settings/src/main.rs
grep -n "fn on_[a-z_]*(&mut self, .*&Topic" crates/sola-monitor/src/main.rs
```

- [ ] **Step 2: Migrate sola-shell handlers**

For every handler in `crates/sola-shell/src/app.rs`:

Before:
```rust
fn on_windows(&mut self, topic: &Topic, ctx: &mut AppCtx) {
    let Topic::Windows(payload) = topic else { return };
    // ... uses payload
}
```

After:
```rust
fn on_windows(&mut self, delivery: &sola_bus::Delivery, ctx: &mut AppCtx) {
    let Topic::Windows(payload) = delivery.topic else { return };
    // ... uses payload
}
```

For unit-variant handlers (no destructure), just change the parameter.

If a handler needs to ignore retractions (most won't, since today's topics aren't keyed yet), it doesn't need to add a guard — `retracted` stays false for them.

- [ ] **Step 3: Migrate sola-settings handlers**

Same mechanical edit in `crates/sola-settings/src/main.rs` for the 4 handlers (`on_close_app`, `on_windows`, `on_menu_action`, `on_mail_config`).

- [ ] **Step 4: Migrate sola-monitor handler**

Same in `crates/sola-monitor/src/main.rs` for `on_menu_action`. Note this crate also calls `bus.subscribe_all()` — that path doesn't change.

- [ ] **Step 5: Build the workspace**

```bash
cargo build 2>&1 | tail -50
```

Expected: builds clean. Any failures point to a missed handler.

- [ ] **Step 6: Commit**

```bash
git add crates/sola-shell/src/app.rs crates/sola-settings/src/main.rs crates/sola-monitor/src/main.rs
git commit -m "refactor: migrate bus handlers to &Delivery signature"
```

---

## Task 6: Bus host — sticky map keyed by `(topic, keys)`

**Files:**
- Modify: `crates/sola-bus/src/main.rs`

Change the sticky map type from `HashMap<(String, String), Message>` to `HashMap<(String, Vec<String>), Message>`. Update insert + bootstrap-eviction paths. Persistent stickies restored from disk now land under their real key — the `BUS_SOURCE`-keyed bootstrap entry goes away.

- [ ] **Step 1: Read current main.rs sticky paths**

```bash
sed -n '20,80p' crates/sola-bus/src/main.rs
sed -n '155,170p' crates/sola-bus/src/main.rs
sed -n '250,266p' crates/sola-bus/src/main.rs
```

- [ ] **Step 2: Change `BusState.sticky` type**

In `crates/sola-bus/src/main.rs`, change line 28:

```rust
sticky: HashMap<(String, String), sola_bus::Message>,
```

to:

```rust
sticky: HashMap<(String, Vec<String>), sola_bus::Message>,
```

Also update the comment above to read:

```rust
/// Latest sticky message per (topic, keys), replayed to newly connected clients.
/// `keys` is empty for unkeyed topics — at most one such sticky exists per topic kind.
/// Keyed topics may have many concurrent stickies, addressed by their key values.
```

- [ ] **Step 3: Update bootstrap insert**

In `main()` (around line 61–64), the current code is:

```rust
let mut sticky = HashMap::new();
for msg in restored {
    sticky.insert((msg.topic.clone(), msg.source.clone()), msg);
}
```

Replace with:

```rust
let mut sticky = HashMap::new();
for msg in restored {
    sticky.insert((msg.topic.clone(), msg.keys.clone()), msg);
}
```

- [ ] **Step 4: Update sticky-emit path**

In `handle_client`, around line 156, the current code is:

```rust
if event.sticky {
    let key = (event.topic.clone(), event.source.clone());
    bus.sticky.insert(key, event.clone());
}
```

Replace with:

```rust
if event.sticky {
    let key = (event.topic.clone(), event.keys.clone());
    bus.sticky.insert(key, event.clone());
}
```

- [ ] **Step 5: Drop bootstrap-eviction in `persist_if_needed`**

In `persist_if_needed`, lines 251–266 currently include:

```rust
if event.source != state::BUS_SOURCE {
    let bootstrap = (kind.as_str().to_string(), state::BUS_SOURCE.to_string());
    bus.sticky.remove(&bootstrap);
}
```

Delete those four lines entirely. The new keying means restored stickies live under `(topic, keys)`, and a client emit with the same `(topic, keys)` overwrites them naturally.

After the edit, `persist_if_needed` reads:

```rust
fn persist_if_needed(event: &sola_bus::Message, bus: &mut BusState) {
    let Some(topic) = Topic::parse(event) else {
        return;
    };
    let kind = topic.kind();
    if !kind.behavior().is_persistent() {
        return;
    }
    if let Err(e) = state::write_section(&bus.state_path, &topic) {
        warn!(topic = kind.as_str(), %e, "persistent write failed");
    }
}
```

- [ ] **Step 6: Build sola-bus and run all bus tests**

```bash
cargo test -p sola-bus 2>&1 | tail -40
```

Expected: state.rs test `zones_round_trip_through_disk` still passes; everything else builds.

- [ ] **Step 7: Commit**

```bash
git add crates/sola-bus/src/main.rs
git commit -m "feat(sola-bus): key sticky map by (topic, keys); drop bootstrap eviction"
```

---

## Task 7: Bus host — handle retract path

**Files:**
- Modify: `crates/sola-bus/src/main.rs`

When the bus receives `sticky=false` for a topic kind that's `#[sticky]` or `#[persistent]`, it must evict the entry under `(topic, keys)` and broadcast the message anyway so subscribers can reconcile. For ephemeral kinds, `sticky=false` keeps today's behavior (transient broadcast, no map mutation).

- [ ] **Step 1: Write a smoke test for the keys plumbing**

Create `crates/sola-bus/tests/retract.rs`:

```rust
//! Smoke test: the message produced by Topic::to_message carries
//! an empty `keys` vec for an unkeyed persistent topic. The end-to-end
//! retract behavior (with a real keyed topic) is verified at Task 11.

use sola_bus::topics::Topic;

#[test]
fn unkeyed_topic_emits_empty_keys() {
    let topic = Topic::Zones(std::collections::HashMap::new());
    let msg = topic.to_message();
    assert!(msg.keys.is_empty());
}
```

- [ ] **Step 2: Run the test to verify pass against current code**

```bash
cargo test -p sola-bus --test retract 2>&1 | head -20
```

Expected: passes (Topic::to_message wires `keys` from Task 2).

- [ ] **Step 3: Implement retract in `handle_client`**

In `crates/sola-bus/src/main.rs`, the current default arm (around lines 154–165) reads:

```rust
_ => {
    let mut bus = state.lock().unwrap();
    if event.sticky {
        let key = (event.topic.clone(), event.keys.clone());
        bus.sticky.insert(key, event.clone());
    }
    broadcast(id, &event, &mut bus);
    if event.sticky {
        persist_if_needed(&event, &mut bus);
    }
}
```

Replace with:

```rust
_ => {
    let mut bus = state.lock().unwrap();
    let kind = sola_bus::topics::Topic::parse(&event).map(|t| t.kind());
    let is_sticky_kind = kind.is_some_and(|k| k.behavior().is_sticky());
    if event.sticky {
        let key = (event.topic.clone(), event.keys.clone());
        bus.sticky.insert(key, event.clone());
    } else if is_sticky_kind {
        // Retract: client signals removal by sending sticky=false on a
        // sticky/persistent topic kind. Evict from in-memory map and,
        // if persistent, from disk.
        let key = (event.topic.clone(), event.keys.clone());
        bus.sticky.remove(&key);
        if let Some(k) = kind {
            if k.behavior().is_persistent() {
                if let Err(e) = state::retract_section(&bus.state_path, &event) {
                    warn!(topic = %event.topic, %e, "persistent retract failed");
                }
            }
        }
    }
    broadcast(id, &event, &mut bus);
    if event.sticky {
        persist_if_needed(&event, &mut bus);
    }
}
```

This calls `state::retract_section`, which doesn't exist yet — Task 9 adds it. For now, the build will fail at this step. Defer the build-check; Step 6 verifies after Task 9.

Actually — to keep this task self-contained, add a stub of `retract_section` to state.rs now and make Task 9 fill in the real implementation:

In `crates/sola-bus/src/state.rs`, append:

```rust
/// Remove the matching record from the persistent topic's section in
/// state.toml. For keyed kinds, removes the entry whose key fields
/// match `event.keys`; if it was the last entry, drops the section.
/// For unkeyed kinds, drops the section.
///
/// Filled in by Task 9 of the keyed-stickies plan; this stub keeps the
/// build green.
pub fn retract_section(path: &Path, event: &Message) -> io::Result<()> {
    let _ = (path, event);
    Ok(())
}
```

- [ ] **Step 4: Build sola-bus**

```bash
cargo build -p sola-bus 2>&1 | tail -30
```

Expected: clean.

- [ ] **Step 5: Run integration test**

```bash
cargo test -p sola-bus --test retract 2>&1 | head -20
```

Expected: passes.

- [ ] **Step 6: Commit**

```bash
git add crates/sola-bus/src/main.rs crates/sola-bus/src/state.rs crates/sola-bus/tests/retract.rs
git commit -m "feat(sola-bus): handle retract path; stub state::retract_section"
```

---

## Task 8: Client — `BusClient::retract(topic)` API

**Files:**
- Modify: `crates/sola-bus/src/client.rs`

Mirror the existing `BusClient::emit(topic)`: build the message, set `sticky=false`, populate `source` and `keys`, ship it. Reject (with a tracing warning, no panic) if the topic kind isn't `#[sticky]` / `#[persistent]`.

- [ ] **Step 1: Read the existing `emit` method**

```bash
grep -n "pub fn emit\|fn emit_" crates/sola-bus/src/client.rs
sed -n '195,215p' crates/sola-bus/src/client.rs
```

- [ ] **Step 2: Add `retract` method**

In `crates/sola-bus/src/client.rs`, immediately after the `emit` method, add:

```rust
/// Retract a sticky topic. Symmetric to `emit`: the bus removes the
/// entry under `(topic, keys)` from its sticky map (and from disk if
/// persistent) and broadcasts the message so subscribers can drop
/// their local copy. No-op on the wire if the topic kind is
/// ephemeral (logs a warning).
pub fn retract(&mut self, topic: crate::topics::Topic) -> std::io::Result<()> {
    let kind = topic.kind();
    if !kind.behavior().is_sticky() {
        tracing::warn!(
            ?kind,
            "retract on ephemeral topic kind; ignoring"
        );
        return Ok(());
    }
    let mut message = topic.to_message();
    message.sticky = false;
    message.source = self.app_id.clone();
    self.send(&message)
}
```

- [ ] **Step 3: Build sola-bus**

```bash
cargo build -p sola-bus 2>&1 | tail -20
```

Expected: clean.

- [ ] **Step 4: Add a unit test for the warning path**

Append to the `tests` module at the bottom of `crates/sola-bus/src/client.rs` (or create one if absent):

Skip this test for now — `BusClient::send` requires a real socket. The behavior is straightforward and will be verified end-to-end at Task 14.

- [ ] **Step 5: Commit**

```bash
git add crates/sola-bus/src/client.rs
git commit -m "feat(sola-bus): add BusClient::retract(topic)"
```

Also surface `retract` on `BusProxy` and `AppCtx` if those wrappers exist:

- [ ] **Step 6: Find BusProxy / AppCtx and mirror retract**

```bash
grep -rn "fn emit\b\|pub fn emit\b" crates/sola-bus/src crates/sola-app/src | head -20
```

For each `emit` wrapper, add a parallel `retract` method that forwards to the underlying client. For example, in `crates/sola-app/src/ctx.rs` (or wherever `AppCtx` lives), if `emit` is:

```rust
pub fn emit(&self, topic: Topic) {
    if let Err(e) = self.bus.borrow_mut().emit(topic) {
        tracing::warn!(%e, "emit failed");
    }
}
```

Add directly below:

```rust
pub fn retract(&self, topic: Topic) {
    if let Err(e) = self.bus.borrow_mut().retract(topic) {
        tracing::warn!(%e, "retract failed");
    }
}
```

Likewise for any `BusProxy`-like wrapper. Mirror the exact error-handling style used by `emit` in that file.

- [ ] **Step 7: Build the workspace**

```bash
cargo build 2>&1 | tail -30
```

Expected: clean.

- [ ] **Step 8: Commit**

```bash
git add crates/sola-app/src/ crates/sola-bus/src/
git commit -m "feat(sola-app): expose retract on AppCtx and BusProxy"
```

---

## Task 9: Disk persistence — load `[[Section]]` for keyed kinds

**Files:**
- Modify: `crates/sola-bus/src/state.rs`

Update `load()` to choose between `[Section]` (single-record) and `[[Section]]` (array of tables) based on `TopicKind::has_keys()`. For keyed kinds, iterate the array and emit one Message per entry, with `keys` extracted from the parsed payload.

- [ ] **Step 1: Read current load()**

```bash
sed -n '39,82p' crates/sola-bus/src/state.rs
```

- [ ] **Step 2: No new test in this task**

There's no real persistent-keyed topic in master yet (the first one — `TerminalSession` — arrives via the dependent terminal-port branch), and the only keyed sticky in this plan, `SetAppMenu` (added in Task 11), is in-memory only. So we can't write a keyed `load()` test from in-tree fixtures without inventing a test topic. Rely on existing tests (`zones_round_trip_through_disk`, etc.) to confirm the unkeyed path still works, and on the Task 12 build to confirm the keyed code path compiles.

- [ ] **Step 3: Update load() to dispatch by `has_keys`**

Replace the body of `load()` in `crates/sola-bus/src/state.rs` with:

```rust
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
```

- [ ] **Step 4: Build and run state tests**

```bash
cargo test -p sola-bus --lib state::tests 2>&1 | head -40
```

Expected: existing tests still pass (Zones path unchanged for unkeyed kinds).

- [ ] **Step 5: Commit**

```bash
git add crates/sola-bus/src/state.rs
git commit -m "feat(sola-bus): load [[Section]] arrays for keyed persistent topics"
```

---

## Task 10: Disk persistence — write `[[Section]]` for keyed kinds + real `retract_section`

**Files:**
- Modify: `crates/sola-bus/src/state.rs`

Update `write_section()` to either replace a single `[Section]` (unkeyed) or upsert into a `[[Section]]` array by key match (keyed). Implement `retract_section()` to remove the matching array entry; if the array becomes empty, drop the section entirely.

- [ ] **Step 1: Update `write_section`**

Replace the body of `write_section` in `crates/sola-bus/src/state.rs` with:

```rust
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

/// Implement `retract_section` to remove the matching record from a
/// keyed persistent topic's section in state.toml. For unkeyed kinds,
/// removes the section entirely.
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
```

- [ ] **Step 2: Add tests for keyed write + retract**

Since master has no real persistent-keyed topic yet (TerminalSession arrives via the terminal-port branch), write a test that constructs the TOML by hand and verifies the round-trip behavior, plus the unkeyed delete path:

Append to the `tests` module in `crates/sola-bus/src/state.rs`:

```rust
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
```

- [ ] **Step 3: Run state tests**

```bash
cargo test -p sola-bus --lib state::tests 2>&1 | tail -40
```

Expected: all pass.

- [ ] **Step 4: Build full workspace**

```bash
cargo build 2>&1 | tail -30
```

Expected: clean.

- [ ] **Step 5: Commit**

```bash
git add crates/sola-bus/src/state.rs
git commit -m "feat(sola-bus): write [[Section]] for keyed topics; implement retract_section"
```

---

## Task 11: Audit & convert `SetAppMenu` to keyed sticky

**Files:**
- Modify: `crates/sola-bus/src/topics.rs`

`SetAppMenu` today is `#[sticky]` with multiple emitters (4+ apps each pushing their own menu). Under the new `(topic, keys)` map, those emitters would clobber each other. Convert it to `#[sticky(keys = [app_id])]` so per-app menus remain independent. The payload `AppMenuPayload` already carries `app_id` (verified during exploration).

Other sticky topics (`Windows`, `OutputGeometry`, `RegisteredChords`) have a single emitter each (sola-river or sola-shell), so they stay unkeyed.

- [ ] **Step 1: Find the SetAppMenu declaration**

```bash
grep -n "SetAppMenu\|AppMenuPayload" crates/sola-bus/src/topics.rs | head -10
```

- [ ] **Step 2: Update the declaration**

In `crates/sola-bus/src/topics.rs`, change the line declaring `SetAppMenu` from:

```rust
#[sticky]
SetAppMenu(AppMenuPayload),
```

to:

```rust
#[sticky(keys = [app_id])]
SetAppMenu(AppMenuPayload),
```

The macro's `keys_for` will extract `payload.app_id.to_string()` and ship it as the key. Existing emitters need no change — they already include `app_id` in the payload.

- [ ] **Step 3: Build the workspace**

```bash
cargo build 2>&1 | tail -30
```

Expected: clean. The macro accepts the new declaration and `app_id` (a `String` field on `AppMenuPayload`) implements `Display`.

- [ ] **Step 4: Verify multi-app menu behavior with a quick test**

Append to `crates/sola-bus/tests/retract.rs`:

```rust
#[test]
fn set_app_menu_emits_app_id_as_key() {
    use sola_bus::topics::{AppMenuPayload, Topic};
    let topic = Topic::SetAppMenu(AppMenuPayload {
        app_id: "sola-browser".into(),
        menus: vec![],
    });
    let msg = topic.to_message();
    assert_eq!(msg.keys, vec!["sola-browser".to_string()]);
}

#[test]
fn two_apps_set_app_menu_have_independent_keys() {
    use sola_bus::topics::{AppMenuPayload, Topic};
    let a = Topic::SetAppMenu(AppMenuPayload {
        app_id: "sola-browser".into(),
        menus: vec![],
    })
    .to_message();
    let b = Topic::SetAppMenu(AppMenuPayload {
        app_id: "sola-terminal".into(),
        menus: vec![],
    })
    .to_message();
    assert_ne!(a.keys, b.keys);
}
```

- [ ] **Step 5: Run integration tests**

```bash
cargo test -p sola-bus --test retract 2>&1 | head -30
```

Expected: all pass.

- [ ] **Step 6: Build full workspace one more time**

```bash
cargo build 2>&1 | tail -20
```

Expected: clean.

- [ ] **Step 7: Commit**

```bash
git add crates/sola-bus/src/topics.rs crates/sola-bus/tests/retract.rs
git commit -m "feat(sola-bus): make SetAppMenu keyed by app_id"
```

---

## Task 12: Final smoke check & sticky replay sanity

**Files:**
- None (verification only)

- [ ] **Step 1: Run the full test suite**

```bash
cargo test 2>&1 | tail -40
```

Expected: all tests pass across the workspace.

- [ ] **Step 2: Build the full workspace via `cargo make`**

```bash
cargo make build 2>&1 | tail -30
```

Expected: clean.

- [ ] **Step 3: Inspect git log for the branch**

```bash
git log --oneline master..HEAD
```

Expected: ~12 commits matching the tasks above, in order.

- [ ] **Step 4: Final spec/plan cross-check**

Verify each spec section has corresponding implementation:
- ✅ Message extension → Task 1
- ✅ Macro extension → Task 2
- ✅ Bus host changes (sticky map + retract path) → Tasks 6, 7
- ✅ Client API → Task 8
- ✅ Subscriber callback shape → Tasks 3, 4, 5
- ✅ Disk persistence (load + write + retract) → Tasks 9, 10
- ✅ Wire format / postcard → Task 1
- ✅ Replay barrier (explicitly absent) → no code; subscribers handle each delivery
- ✅ Multi-emitter audit → Task 11

- [ ] **Step 5: Done — branch ready for merge to terminal-port worktree**

```bash
git status
```

Expected: clean. Notify user that `feature/keyed-stickies` is ready to merge into `feature/terminal-port` for the terminal refactor consumer.
