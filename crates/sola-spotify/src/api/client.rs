//! An authenticated, rate-limited transport for the Spotify Web API.
//!
//! Adapted from Fastpotify (MIT), https://github.com/crmne/fastpotify
//!
//! Typed calls share one `request` helper that adds the bearer token, limits
//! concurrency, honors `Retry-After`, and formats API errors. Background
//! work (polls, syncs, liked checks) runs inside [`background`] and gets a
//! smaller budget, so it can never hold every slot a click needs.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use reqwest::{Method, StatusCode};
use serde::de::DeserializeOwned;
use serde_json::{Value, json};
use thiserror::Error;
use tokio::sync::Semaphore;

use super::models::*;

const BASE_URL: &str = "https://api.spotify.com/v1";
const MAX_IN_FLIGHT: usize = 6;
/// Background requests share at most this many of the in-flight slots.
const MAX_BACKGROUND_IN_FLIGHT: usize = 4;
const RATE_LIMIT_RETRIES: u32 = 3;
const MAX_RETRY_AFTER: Duration = Duration::from_secs(30);
/// Development Mode quota exhaustion lasts longer than a rate limit.
const MAX_QUOTA_WAIT: Duration = Duration::from_secs(600);

tokio::task_local! {
    static BACKGROUND: ();
}

/// Runs `future` with the background request budget: requests nothing on
/// screen is waiting for.
pub async fn background<F: std::future::Future>(future: F) -> F::Output {
    BACKGROUND.scope((), future).await
}

fn in_background() -> bool {
    BACKGROUND.try_with(|_| ()).is_ok()
}

#[derive(Clone, Debug, Error)]
pub enum ApiError {
    #[error("not signed in")]
    NotSignedIn,
    #[error("{message}")]
    Status { status: u16, message: String },
    #[error("Spotify is rate limiting requests; try again in a moment")]
    RateLimited,
    #[error("Spotify's Development Mode quota is exhausted; try again after the quota resets")]
    QuotaExhausted,
    #[error("your Spotify sign-in expired; please sign in again")]
    SignInExpired,
    #[error("network error: {0}")]
    Network(String),
    #[error("unexpected response from Spotify: {0}")]
    Decode(String),
}

impl ApiError {
    pub fn status(&self) -> Option<u16> {
        match self {
            Self::Status { status, .. } => Some(*status),
            _ => None,
        }
    }
}

impl From<reqwest::Error> for ApiError {
    fn from(error: reqwest::Error) -> Self {
        if error.is_decode() {
            Self::Decode(error.to_string())
        } else {
            Self::Network(error.to_string())
        }
    }
}

pub type Result<T> = std::result::Result<T, ApiError>;

fn retry_after(header: Option<&str>) -> Duration {
    header
        .and_then(|value| value.parse::<u64>().ok())
        .map_or(Duration::from_secs(5), Duration::from_secs)
        .max(Duration::from_secs(5))
}

fn retry_after_wait(header: Option<&str>) -> Duration {
    retry_after(header).min(MAX_RETRY_AFTER)
}

fn is_quota_exhausted(body: &str) -> bool {
    serde_json::from_str::<ApiErrorBody>(body)
        .ok()
        .and_then(|body| body.error.reason)
        .is_some_and(|reason| reason == "QUOTA_EXCEEDED")
}

/// Where bearer tokens come from.
///
/// The Web API is driven by a registered application's PKCE grant, refreshed
/// on demand and persisted so the browser is needed once per machine. Tokens
/// minted for Spotify's own desktop client are throttled on the Web API, so
/// they are never used here. `Fixed` exists only for tests.
#[derive(Clone)]
pub enum TokenProvider {
    Web(std::sync::Arc<WebTokens>),
}

impl TokenProvider {
    async fn access_token(&self) -> Result<String> {
        match self {
            Self::Web(tokens) => tokens.access_token(false).await,
        }
    }

    async fn invalidate(&self) {
        let Self::Web(tokens) = self;
        let _ = tokens.access_token(true).await;
    }
}

/// The Web API grant, refreshed and persisted as it ages.
pub struct WebTokens {
    http: reqwest::Client,
    token: tokio::sync::Mutex<crate::auth::StoredToken>,
    path: std::path::PathBuf,
}

impl WebTokens {
    pub fn new(
        http: reqwest::Client,
        token: crate::auth::StoredToken,
        path: std::path::PathBuf,
    ) -> std::sync::Arc<Self> {
        std::sync::Arc::new(Self {
            http,
            token: tokio::sync::Mutex::new(token),
            path,
        })
    }

    /// A valid access token, refreshing first when it is close to expiry or
    /// `force` asks for a fresh one after a 401.
    async fn access_token(&self, force: bool) -> Result<String> {
        let mut guard = self.token.lock().await;
        if force || guard.needs_refresh() {
            let client_id = guard.client_id.clone();
            let refresh_token = guard.refresh_token.clone();
            match crate::auth::refresh(&self.http, &client_id, &refresh_token).await {
                Ok(response) => match crate::auth::StoredToken::from_response(
                    &client_id,
                    response,
                    Some(&refresh_token),
                ) {
                    Ok(updated) => {
                        let _ = updated.save(&self.path);
                        *guard = updated;
                    }
                    Err(error) => {
                        tracing::warn!("token refresh returned an unusable response: {error}")
                    }
                },
                Err(crate::auth::TokenEndpointError::Rejected { .. }) => {
                    return Err(ApiError::SignInExpired);
                }
                Err(crate::auth::TokenEndpointError::Unreachable(detail)) => {
                    if force || guard.expired() {
                        return Err(ApiError::Network(detail));
                    }
                    tracing::warn!("token refresh failed, using the current token: {detail}");
                }
            }
        }
        Ok(guard.access_token.clone())
    }
}

/// What to start playing.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PlayRequest {
    pub context_uri: Option<String>,
    pub uris: Vec<String>,
    pub offset_uri: Option<String>,
    pub offset_position: Option<u32>,
    pub position_ms: u32,
}

impl PlayRequest {
    pub fn context(uri: impl Into<String>) -> Self {
        Self {
            context_uri: Some(uri.into()),
            ..Self::default()
        }
    }

    pub fn tracks(uris: Vec<String>) -> Self {
        Self {
            uris,
            ..Self::default()
        }
    }

    pub fn starting_at_uri(mut self, uri: impl Into<String>) -> Self {
        self.offset_uri = Some(uri.into());
        self
    }

    fn body(&self) -> Value {
        let mut body = serde_json::Map::new();
        if let Some(context) = &self.context_uri {
            body.insert("context_uri".into(), json!(context));
        } else if !self.uris.is_empty() {
            body.insert("uris".into(), json!(self.uris));
        }
        if let Some(uri) = &self.offset_uri {
            body.insert("offset".into(), json!({ "uri": uri }));
        } else if let Some(position) = self.offset_position {
            body.insert("offset".into(), json!({ "position": position }));
        }
        if self.position_ms > 0 {
            body.insert("position_ms".into(), json!(self.position_ms));
        }
        Value::Object(body)
    }
}

pub struct ApiClient {
    http: reqwest::Client,
    tokens: Mutex<Option<TokenProvider>>,
    limiter: Semaphore,
    background_limiter: Semaphore,
    cooldown_until: tokio::sync::Mutex<Instant>,
    /// Set by a 429 and cleared by the next answer that is not one. While
    /// set, requests leave the cooldown one at a time behind `probe`, so a
    /// queue of them does not fire together into a still-empty bucket.
    recovering: AtomicBool,
    probe: tokio::sync::Mutex<()>,
    search_limit: u32,
    artist_albums_limit: u32,
}

impl ApiClient {
    pub fn new(http: reqwest::Client, search_limit: u32, artist_albums_limit: u32) -> Self {
        Self {
            http,
            tokens: Mutex::new(None),
            limiter: Semaphore::new(MAX_IN_FLIGHT),
            background_limiter: Semaphore::new(MAX_BACKGROUND_IN_FLIGHT),
            cooldown_until: tokio::sync::Mutex::new(Instant::now()),
            recovering: AtomicBool::new(false),
            probe: tokio::sync::Mutex::new(()),
            search_limit,
            artist_albums_limit,
        }
    }

    pub fn set_token_provider(&self, provider: Option<TokenProvider>) {
        *self.tokens.lock().unwrap_or_else(|p| p.into_inner()) = provider;
    }

    fn provider(&self) -> Result<TokenProvider> {
        self.tokens
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .clone()
            .ok_or(ApiError::NotSignedIn)
    }

    pub async fn cooling_down(&self) -> bool {
        *self.cooldown_until.lock().await > Instant::now()
    }

    /// Waits out a `Retry-After`. Only quota exhaustion cools down longer
    /// than a rate limit; nothing should sit on that, so it fails at once.
    /// A player command (`urgent`) fails during any cooldown: a pause that
    /// lands half a minute late can stop music the listener resumed since.
    async fn wait_for_cooldown(&self, urgent: bool) -> Result<()> {
        loop {
            let until = *self.cooldown_until.lock().await;
            let Some(wait) = until.checked_duration_since(Instant::now()) else {
                return Ok(());
            };
            if wait > MAX_RETRY_AFTER {
                return Err(ApiError::QuotaExhausted);
            }
            if urgent {
                return Err(ApiError::RateLimited);
            }
            tokio::time::sleep(wait).await;
        }
    }

    async fn extend_cooldown(&self, wait: Duration) {
        let mut until = self.cooldown_until.lock().await;
        *until = (*until).max(Instant::now() + wait);
    }

    /// While recovering from a 429, the probe lock lets one request out of
    /// the cooldown at a time; the rest follow once an answer is not a 429.
    async fn probe_turn(&self, urgent: bool) -> Result<Option<tokio::sync::MutexGuard<'_, ()>>> {
        if !self.recovering.load(Ordering::SeqCst) {
            return Ok(None);
        }
        let guard = self.probe.lock().await;
        // The previous probe may have earned a fresh cooldown.
        self.wait_for_cooldown(urgent).await?;
        Ok(self.recovering.load(Ordering::SeqCst).then_some(guard))
    }

    // ---- transport -------------------------------------------------------

    async fn send(
        &self,
        method: Method,
        path: &str,
        query: &[(&str, String)],
        body: Option<&Value>,
    ) -> Result<String> {
        let url = if path.starts_with("http") {
            path.to_string()
        } else {
            format!("{BASE_URL}{path}")
        };
        let provider = self.provider()?;
        let started = Instant::now();
        let background = in_background();
        let urgent = method != Method::GET && path.starts_with("/me/player");

        let mut attempt = 0;
        loop {
            attempt += 1;
            let background_permit = if background {
                Some(
                    self.background_limiter
                        .acquire()
                        .await
                        .map_err(|_| ApiError::NotSignedIn)?,
                )
            } else {
                None
            };
            let permit = self
                .limiter
                .acquire()
                .await
                .map_err(|_| ApiError::NotSignedIn)?;
            // After the permit: a 429 answered while this request queued has
            // extended the cooldown, and sending now would only earn another.
            self.wait_for_cooldown(urgent).await?;
            let mut probe = self.probe_turn(urgent).await?;
            let token = provider.access_token().await?;
            let mut request = self
                .http
                .request(method.clone(), &url)
                .bearer_auth(&token)
                .query(query);
            if let Some(body) = body {
                request = request.json(body);
            } else if matches!(method, Method::PUT | Method::POST | Method::DELETE) {
                request = request.header(reqwest::header::CONTENT_LENGTH, "0");
            }
            let response = request.send().await?;
            let status = response.status();

            let rate_limited = status == StatusCode::TOO_MANY_REQUESTS;
            self.recovering.store(rate_limited, Ordering::SeqCst);
            // After a 429 the probe is held until the new cooldown is
            // recorded, so the next request in line cannot slip out first.
            if !rate_limited {
                drop(probe.take());
            }
            if status == StatusCode::UNAUTHORIZED && attempt == 1 {
                drop(permit);
                drop(background_permit);
                provider.invalidate().await;
                continue;
            }
            if status == StatusCode::TOO_MANY_REQUESTS {
                let header = response
                    .headers()
                    .get(reqwest::header::RETRY_AFTER)
                    .and_then(|value| value.to_str().ok())
                    .map(str::to_string);
                let text = response.text().await.unwrap_or_default();
                if is_quota_exhausted(&text) {
                    let wait = retry_after(header.as_deref()).min(MAX_QUOTA_WAIT);
                    tracing::warn!("Spotify quota exhausted path={path} wait={wait:?}");
                    self.extend_cooldown(wait).await;
                    return Err(ApiError::QuotaExhausted);
                }
                let wait = retry_after_wait(header.as_deref());
                tracing::warn!(
                    "Spotify rate limit method={} path={path} wait={wait:?}",
                    method
                );
                tracing::info!(
                    "Spotify cooldown path={path} duration_ms={}",
                    wait.as_millis()
                );
                drop(permit);
                drop(background_permit);
                self.extend_cooldown(wait).await;
                drop(probe.take());
                if urgent || attempt > RATE_LIMIT_RETRIES {
                    return Err(ApiError::RateLimited);
                }
                continue;
            }
            if status.is_server_error() && method == Method::GET && attempt == 1 {
                drop(permit);
                drop(background_permit);
                tokio::time::sleep(Duration::from_millis(800)).await;
                continue;
            }
            let text = response.text().await?;
            tracing::debug!(
                "Spotify request method={} status={} duration_ms={}",
                method,
                status.as_u16(),
                started.elapsed().as_millis()
            );
            if status.is_success() {
                return Ok(text);
            }
            let message = serde_json::from_str::<ApiErrorBody>(&text)
                .ok()
                .map(|body| body.error.message)
                .filter(|message| !message.is_empty())
                .unwrap_or_else(|| {
                    status
                        .canonical_reason()
                        .unwrap_or("request failed")
                        .to_string()
                });
            return Err(ApiError::Status {
                status: status.as_u16(),
                message,
            });
        }
    }

    async fn get<T: DeserializeOwned>(&self, path: &str, query: &[(&str, String)]) -> Result<T> {
        let text = self.send(Method::GET, path, query, None).await?;
        if text.trim().is_empty() {
            return serde_json::from_value(Value::Null)
                .map_err(|error| ApiError::Decode(error.to_string()));
        }
        serde_json::from_str(&text).map_err(|error| ApiError::Decode(error.to_string()))
    }

    async fn get_optional<T: DeserializeOwned>(
        &self,
        path: &str,
        query: &[(&str, String)],
    ) -> Result<Option<T>> {
        // Spotify answers 204 with no body when nothing is playing.
        let text = self.send(Method::GET, path, query, None).await?;
        if text.trim().is_empty() || text.trim() == "null" {
            return Ok(None);
        }
        serde_json::from_str(&text)
            .map(Some)
            .map_err(|error| ApiError::Decode(error.to_string()))
    }

    /// Performs a change. The status decides success; the body is only
    /// consulted where a caller needs something from it, because Spotify's
    /// replies to player commands are not reliably JSON and contain nothing
    /// this client uses. Treating an unparseable body as failure told people
    /// their music had not started while it was already playing.
    async fn write(
        &self,
        method: Method,
        path: &str,
        query: &[(&str, String)],
        body: Option<&Value>,
    ) -> Result<Option<Value>> {
        let text = self.send(method, path, query, body).await?;
        if text.trim().is_empty() {
            return Ok(None);
        }
        match serde_json::from_str(&text) {
            Ok(value) => Ok(Some(value)),
            Err(error) => {
                tracing::debug!("Spotify write returned a non-JSON success body: {error}");
                Ok(None)
            }
        }
    }

    // ---- identity and player ---------------------------------------------

    pub async fn me(&self) -> Result<User> {
        self.get("/me", &[]).await
    }

    pub async fn devices(&self) -> Result<Vec<Device>> {
        let list: DeviceList = self.get("/me/player/devices", &[]).await?;
        Ok(list.devices)
    }

    pub async fn playback_state(&self) -> Result<Option<PlaybackState>> {
        self.get_optional(
            "/me/player",
            &[("additional_types", "track,episode".to_string())],
        )
        .await
    }

    pub async fn queue(&self) -> Result<Queue> {
        self.get("/me/player/queue", &[]).await
    }

    pub async fn recently_played(
        &self,
        limit: u32,
        after: Option<&str>,
        before: Option<&str>,
    ) -> Result<CursorPage<PlayHistory>> {
        let mut query = vec![("limit", limit.to_string())];
        if let Some(after) = after {
            query.push(("after", after.to_string()));
        }
        if let Some(before) = before {
            query.push(("before", before.to_string()));
        }
        self.get("/me/player/recently-played", &query).await
    }

    fn device_query(device_id: Option<&str>) -> Vec<(&'static str, String)> {
        device_id
            .map(|id| vec![("device_id", id.to_string())])
            .unwrap_or_default()
    }

    pub async fn play(&self, device_id: Option<&str>, request: Option<&PlayRequest>) -> Result<()> {
        let body = request.map(PlayRequest::body);
        self.write(
            Method::PUT,
            "/me/player/play",
            &Self::device_query(device_id),
            body.as_ref(),
        )
        .await?;
        Ok(())
    }

    pub async fn pause(&self, device_id: Option<&str>) -> Result<()> {
        self.write(
            Method::PUT,
            "/me/player/pause",
            &Self::device_query(device_id),
            None,
        )
        .await?;
        Ok(())
    }

    pub async fn next(&self, device_id: Option<&str>) -> Result<()> {
        self.write(
            Method::POST,
            "/me/player/next",
            &Self::device_query(device_id),
            None,
        )
        .await?;
        Ok(())
    }

    pub async fn previous(&self, device_id: Option<&str>) -> Result<()> {
        self.write(
            Method::POST,
            "/me/player/previous",
            &Self::device_query(device_id),
            None,
        )
        .await?;
        Ok(())
    }

    pub async fn seek(&self, position_ms: u32, device_id: Option<&str>) -> Result<()> {
        let mut query = Self::device_query(device_id);
        query.push(("position_ms", position_ms.to_string()));
        self.write(Method::PUT, "/me/player/seek", &query, None)
            .await?;
        Ok(())
    }

    pub async fn set_volume(&self, percent: u8, device_id: Option<&str>) -> Result<()> {
        let mut query = Self::device_query(device_id);
        query.push(("volume_percent", percent.min(100).to_string()));
        self.write(Method::PUT, "/me/player/volume", &query, None)
            .await?;
        Ok(())
    }

    pub async fn set_shuffle(&self, state: bool, device_id: Option<&str>) -> Result<()> {
        let mut query = Self::device_query(device_id);
        query.push(("state", state.to_string()));
        self.write(Method::PUT, "/me/player/shuffle", &query, None)
            .await?;
        Ok(())
    }

    /// `state` is `off`, `context`, or `track`.
    pub async fn set_repeat(&self, state: &str, device_id: Option<&str>) -> Result<()> {
        let mut query = Self::device_query(device_id);
        query.push(("state", state.to_string()));
        self.write(Method::PUT, "/me/player/repeat", &query, None)
            .await?;
        Ok(())
    }

    pub async fn transfer(&self, device_id: &str, play: bool) -> Result<()> {
        let body = json!({ "device_ids": [device_id], "play": play });
        self.write(Method::PUT, "/me/player", &[], Some(&body))
            .await?;
        Ok(())
    }

    // ---- playlists ---------------------------------------------------------

    pub async fn my_playlists(&self, offset: u32, limit: u32) -> Result<Page<Playlist>> {
        self.get(
            "/me/playlists",
            &[("limit", limit.to_string()), ("offset", offset.to_string())],
        )
        .await
    }

    /// Spotify's "Made For You" category (Daily Mix, mixes, …). Apps that
    /// cannot read Spotify-owned lists get a 403 or 404.
    pub async fn made_for_you(&self) -> Result<Vec<Playlist>> {
        const CATEGORY: &str = "0JQ5DAt0tbjZptfcdMSKl3";
        self.get::<CategoryPlaylists>(
            &format!("/browse/categories/{CATEGORY}/playlists"),
            &[("limit", "50".into())],
        )
        .await
        .map(|wrap| wrap.playlists.items)
    }

    /// The playlist header and the first page of rows Spotify embeds in it
    /// (up to 100), in one request instead of two.
    pub async fn playlist_with_items(
        &self,
        id: &str,
    ) -> Result<(Playlist, Option<Page<PlaylistItem>>)> {
        let text = self
            .send(
                Method::GET,
                &format!("/playlists/{id}"),
                &[("additional_types", "track,episode".to_string())],
                None,
            )
            .await?;
        let playlist: Playlist =
            serde_json::from_str(&text).map_err(|error| ApiError::Decode(error.to_string()))?;
        let embedded = serde_json::from_str::<EmbeddedItems>(&text)
            .ok()
            .and_then(EmbeddedItems::page);
        Ok((playlist, embedded))
    }

    pub async fn playlist_items(
        &self,
        id: &str,
        offset: u32,
        limit: u32,
    ) -> Result<Page<PlaylistItem>> {
        self.get(
            &format!("/playlists/{id}/items"),
            &[
                ("limit", limit.to_string()),
                ("offset", offset.to_string()),
                ("additional_types", "track,episode".to_string()),
            ],
        )
        .await
    }

    pub async fn create_playlist(
        &self,
        name: &str,
        public: bool,
        description: &str,
    ) -> Result<Playlist> {
        let body = json!({ "name": name, "public": public, "description": description });
        let value = self
            .write(Method::POST, "/me/playlists", &[], Some(&body))
            .await?
            .unwrap_or(Value::Null);
        serde_json::from_value(value).map_err(|error| ApiError::Decode(error.to_string()))
    }

    pub async fn add_playlist_items(
        &self,
        id: &str,
        uris: &[String],
        position: Option<u32>,
    ) -> Result<Option<String>> {
        let mut body = json!({ "uris": uris });
        if let Some(position) = position {
            body["position"] = json!(position);
        }
        let value = self
            .write(
                Method::POST,
                &format!("/playlists/{id}/items"),
                &[],
                Some(&body),
            )
            .await?;
        Ok(Self::snapshot(value))
    }

    fn snapshot(value: Option<Value>) -> Option<String> {
        value
            .and_then(|value| serde_json::from_value::<SnapshotId>(value).ok())
            .and_then(|snapshot| snapshot.snapshot_id)
    }

    // ---- library -----------------------------------------------------------

    pub async fn saved_tracks(&self, offset: u32, limit: u32) -> Result<Page<SavedTrack>> {
        self.get(
            "/me/tracks",
            &[("limit", limit.to_string()), ("offset", offset.to_string())],
        )
        .await
    }

    pub async fn saved_albums(&self, offset: u32, limit: u32) -> Result<Page<SavedAlbum>> {
        self.get(
            "/me/albums",
            &[("limit", limit.to_string()), ("offset", offset.to_string())],
        )
        .await
    }

    pub async fn followed_artists(
        &self,
        after: Option<&str>,
        limit: u32,
    ) -> Result<CursorPage<Artist>> {
        let mut query = vec![("type", "artist".to_string()), ("limit", limit.to_string())];
        if let Some(after) = after {
            query.push(("after", after.to_string()));
        }
        let followed: FollowedArtists = self.get("/me/following", &query).await?;
        Ok(followed.artists)
    }

    pub async fn top_tracks(
        &self,
        time_range: &str,
        limit: u32,
        offset: u32,
    ) -> Result<Page<Track>> {
        self.get(
            "/me/top/tracks",
            &[
                ("limit", limit.to_string()),
                ("offset", offset.to_string()),
                ("time_range", time_range.to_string()),
            ],
        )
        .await
    }

    pub async fn top_artists(&self, time_range: &str, limit: u32) -> Result<Page<Artist>> {
        self.get(
            "/me/top/artists",
            &[
                ("limit", limit.to_string()),
                ("time_range", time_range.to_string()),
            ],
        )
        .await
    }

    async fn library_write(&self, method: Method, uris: &[String]) -> Result<()> {
        self.write(method, "/me/library", &[("uris", uris.join(","))], None)
            .await?;
        Ok(())
    }

    /// Saves tracks, albums, artists, shows, episodes, or playlists.
    pub async fn save(&self, uris: &[String]) -> Result<()> {
        self.tracks_library(Method::PUT, uris).await
    }

    pub async fn unsave(&self, uris: &[String]) -> Result<()> {
        self.tracks_library(Method::DELETE, uris).await
    }

    pub async fn set_saved_album(&self, id: &str, saved: bool) -> Result<()> {
        let method = if saved { Method::PUT } else { Method::DELETE };
        self.write(method, "/me/albums", &[("ids", id.to_string())], None)
            .await?;
        Ok(())
    }

    pub async fn album_is_saved(&self, id: &str) -> Result<bool> {
        let flags: Vec<bool> = self
            .get("/me/albums/contains", &[("ids", id.to_string())])
            .await?;
        Ok(flags.first().copied().unwrap_or(false))
    }

    pub async fn set_followed_artist(&self, id: &str, saved: bool) -> Result<()> {
        let method = if saved { Method::PUT } else { Method::DELETE };
        self.write(
            method,
            "/me/following",
            &[("type", "artist".to_string()), ("ids", id.to_string())],
            None,
        )
        .await?;
        Ok(())
    }

    pub async fn artist_is_followed(&self, id: &str) -> Result<bool> {
        let flags: Vec<bool> = self
            .get(
                "/me/following/contains",
                &[("type", "artist".to_string()), ("ids", id.to_string())],
            )
            .await?;
        Ok(flags.first().copied().unwrap_or(false))
    }

    async fn tracks_library(&self, method: Method, uris: &[String]) -> Result<()> {
        let ids: Vec<String> = uris
            .iter()
            .filter_map(|uri| spotify_id(uri, "track").map(str::to_string))
            .collect();
        if ids.is_empty() {
            return self.library_write(method, uris).await;
        }
        self.write(method, "/me/tracks", &[("ids", ids.join(","))], None)
            .await?;
        Ok(())
    }

    /// Whether each URI is in Liked Songs, in the same order as `uris`.
    ///
    /// Official likes live at `/me/tracks/contains` (50 ids). The unified
    /// `/me/library/contains` path is capped at 40 and 400s a 50-track page.
    pub async fn contains(&self, uris: &[String]) -> Result<Vec<bool>> {
        let mut flags = vec![false; uris.len()];
        let mut slots = Vec::new();
        let mut ids = Vec::new();
        for (index, uri) in uris.iter().enumerate() {
            if let Some(id) = spotify_id(uri, "track") {
                slots.push(index);
                ids.push(id.to_string());
            }
        }
        if ids.is_empty() {
            return Ok(flags);
        }
        let got: Vec<bool> = self
            .get("/me/tracks/contains", &[("ids", ids.join(","))])
            .await?;
        for (slot, flag) in slots.into_iter().zip(got) {
            flags[slot] = flag;
        }
        Ok(flags)
    }

    // ---- catalog -----------------------------------------------------------

    pub async fn search(&self, query: &str, types: &[&str]) -> Result<SearchResults> {
        self.get(
            "/search",
            &[
                ("q", query.to_string()),
                ("type", types.join(",")),
                ("limit", self.search_limit.to_string()),
            ],
        )
        .await
    }

    pub async fn artist(&self, id: &str) -> Result<Artist> {
        self.get(&format!("/artists/{id}"), &[]).await
    }

    pub async fn artist_top_tracks(&self, id: &str) -> Result<Vec<Track>> {
        self.get::<TopTracks>(&format!("/artists/{id}/top-tracks"), &[])
            .await
            .map(|top| top.tracks)
    }

    pub async fn artist_albums(
        &self,
        id: &str,
        include_groups: &str,
        offset: u32,
        limit: u32,
    ) -> Result<Page<Album>> {
        let limit = limit.min(self.artist_albums_limit);
        self.get(
            &format!("/artists/{id}/albums"),
            &[
                ("include_groups", include_groups.to_string()),
                ("limit", limit.to_string()),
                ("offset", offset.to_string()),
            ],
        )
        .await
    }

    pub async fn album(&self, id: &str) -> Result<Album> {
        self.get(&format!("/albums/{id}"), &[]).await
    }

    pub async fn album_tracks(&self, id: &str, offset: u32, limit: u32) -> Result<Page<Track>> {
        self.get(
            &format!("/albums/{id}/tracks"),
            &[("limit", limit.to_string()), ("offset", offset.to_string())],
        )
        .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn play_request_body_shapes() {
        let context = PlayRequest::context("spotify:album:x").starting_at_uri("spotify:track:y");
        assert_eq!(
            context.body(),
            json!({ "context_uri": "spotify:album:x", "offset": { "uri": "spotify:track:y" } })
        );
        let tracks = PlayRequest {
            offset_position: Some(0),
            ..PlayRequest::tracks(vec!["spotify:track:a".into()])
        };
        assert_eq!(
            tracks.body(),
            json!({ "uris": ["spotify:track:a"], "offset": { "position": 0 } })
        );
    }

    #[test]
    fn retry_after_never_waits_less_than_five_seconds() {
        assert_eq!(retry_after_wait(None), Duration::from_secs(5));
        assert_eq!(retry_after_wait(Some("1")), Duration::from_secs(5));
        assert_eq!(retry_after_wait(Some("19")), Duration::from_secs(19));
        assert_eq!(retry_after_wait(Some("999")), MAX_RETRY_AFTER);
    }

    #[test]
    fn quota_exhaustion_is_distinct_from_an_ordinary_rate_limit() {
        assert!(is_quota_exhausted(
            r#"{"error":{"status":429,"reason":"QUOTA_EXCEEDED"}}"#
        ));
        assert!(!is_quota_exhausted(
            r#"{"error":{"status":429,"message":"Too many requests"}}"#
        ));
    }

    #[tokio::test]
    async fn after_a_rate_limit_requests_leave_the_cooldown_one_at_a_time() {
        let client = ApiClient::new(reqwest::Client::new(), 20, 50);
        assert!(client.probe_turn(false).await.unwrap().is_none());
        client.recovering.store(true, Ordering::SeqCst);
        let first = client.probe_turn(false).await.unwrap();
        assert!(first.is_some());
        // The next request waits for the probe's answer.
        assert!(client.probe.try_lock().is_err());
        client.recovering.store(false, Ordering::SeqCst);
        drop(first);
        assert!(client.probe_turn(false).await.unwrap().is_none());
    }

    #[tokio::test]
    async fn long_or_player_cooldowns_fail_instead_of_holding_requests() {
        let client = ApiClient::new(reqwest::Client::new(), 20, 50);
        client.extend_cooldown(Duration::from_secs(5)).await;
        assert!(matches!(
            client.wait_for_cooldown(true).await,
            Err(ApiError::RateLimited)
        ));
        client.extend_cooldown(Duration::from_secs(300)).await;
        assert!(matches!(
            client.wait_for_cooldown(false).await,
            Err(ApiError::QuotaExhausted)
        ));
    }

    #[tokio::test]
    async fn background_work_is_marked_for_its_own_budget() {
        assert!(!in_background());
        assert!(background(async { in_background() }).await);
    }
}
