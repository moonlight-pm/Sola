//! Spotify Web API client.

pub mod client;
pub mod models;

pub use client::{ApiClient, ApiError, PlayRequest, TokenProvider, WebTokens, background};
