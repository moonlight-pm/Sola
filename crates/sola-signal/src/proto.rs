//! Signal's wire messages, generated from `proto/` (copied from Signal-Android).

#![allow(clippy::all, dead_code)]

include!(concat!(env!("OUT_DIR"), "/signalservice.rs"));

/// The group server's messages (`Groups.proto`, package `signal`).
pub mod groups {
    include!(concat!(env!("OUT_DIR"), "/signal.rs"));
}
