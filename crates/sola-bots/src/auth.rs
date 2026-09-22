//! Shared secret for the phone HTTP API. Sideload-only; not a public product.

pub const HTTP_BIND: &str = "0.0.0.0:27419";
pub const HTTP_PORT: u16 = 27419;
pub const HTTP_HOST: &str = "bot.sola.computer";

/// Bearer token. Same string is compiled into the iOS client.
pub const HTTP_TOKEN: &str = "36bdd2069df7e0f6a3cebeac6c7c4cafe3a086b69c2d7f27a6478631afa5e91c";
