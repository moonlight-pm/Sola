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

/// Define a Topic enum with typed variants, a parse function, and to_message.
///
/// Two variant forms:
/// - Unit: `Shutdown,` — no payload
/// - Payload: `GrabInput(String),` — carries typed data
///
/// # Example
/// ```ignore
/// define_topics! {
///     Shutdown,
///     GrabInput(String),
///     Apps(Vec<App>),
/// }
///
/// // Sending:
/// bus.emit(Topic::GrabInput("sola-switcher".into()))?;
///
/// // Receiving:
/// let Some(topic) = Topic::parse(&msg) else { continue };
/// match topic {
///     Topic::GrabInput(target) => { ... }
///     Topic::Shutdown => { ... }
///     _ => {}
/// }
/// ```
#[macro_export]
macro_rules! define_topics {
    // Entry: separate unit variants from payload variants using a tt muncher
    ( $($tt:tt)* ) => {
        $crate::_define_topics_inner!{ [] [] $($tt)* }
    };
}

#[macro_export]
#[doc(hidden)]
macro_rules! _define_topics_inner {
    // Payload variant: Name(Type),
    ( [ $($unit:ident)* ] [ $( $pname:ident($pty:ty) )* ] $name:ident ( $payload:ty ), $($rest:tt)* ) => {
        $crate::_define_topics_inner!{ [ $($unit)* ] [ $($pname($pty))* $name($payload) ] $($rest)* }
    };
    // Payload variant: Name(Type) (trailing, no comma)
    ( [ $($unit:ident)* ] [ $( $pname:ident($pty:ty) )* ] $name:ident ( $payload:ty ) ) => {
        $crate::_define_topics_inner!{ [ $($unit)* ] [ $($pname($pty))* $name($payload) ] }
    };
    // Unit variant: Name,
    ( [ $($unit:ident)* ] [ $( $pname:ident($pty:ty) )* ] $name:ident, $($rest:tt)* ) => {
        $crate::_define_topics_inner!{ [ $($unit)* $name ] [ $($pname($pty))* ] $($rest)* }
    };
    // Unit variant: Name (trailing, no comma)
    ( [ $($unit:ident)* ] [ $( $pname:ident($pty:ty) )* ] $name:ident ) => {
        $crate::_define_topics_inner!{ [ $($unit)* $name ] [ $($pname($pty))* ] }
    };
    // Terminal: generate everything
    ( [ $($unit:ident)* ] [ $( $pname:ident($pty:ty) )* ] ) => {
        #[derive(Debug, Clone)]
        pub enum Topic {
            $( $unit, )*
            $( $pname($pty), )*
        }

        impl Topic {
            pub fn parse(msg: &$crate::Message) -> Option<Self> {
                match msg.topic.as_str() {
                    $( stringify!($unit) => Some(Topic::$unit), )*
                    $( stringify!($pname) => {
                        $crate::topic::decode_payload::<$pty>(msg).ok().map(Topic::$pname)
                    }, )*
                    _ => None,
                }
            }

            pub fn to_message(&self) -> $crate::Message {
                match self {
                    $( Topic::$unit => $crate::Message::new(stringify!($unit)), )*
                    $( Topic::$pname(payload) => $crate::Message::with_payload(
                        stringify!($pname),
                        $crate::topic::encode_payload(payload),
                    ), )*
                }
            }
        }
    };
}
