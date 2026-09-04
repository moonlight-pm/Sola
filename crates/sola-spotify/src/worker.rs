//! Tokio worker: PKCE, Web API, librespot engine, artwork, MPRIS.

use std::sync::Arc;
use std::time::{Duration, Instant};

use librespot_core::authentication::Credentials;
use serde::{Deserialize, Serialize};
use tokio::sync::{mpsc, watch};

use crate::cache::{self, Liked, Skipped};

use crate::api::models::*;
use crate::api::{
    ApiClient, ApiError, ApiSource, NetActivity, PlayRequest, TokenProvider, WebTokens,
};
use crate::auth::{self, Grant, StoredToken};
use crate::bridge;
use crate::images::ArtLoader;
use crate::media::{MediaCommand, MediaState, MediaTrack};
use crate::mpris::MediaService;
use crate::paths::AppDirs;
use crate::player::{
    Engine, EngineConfig, EngineEvent, LoadSpec, LocalState, Playback, PlayerCommand, RepeatMode,
};
use crate::settings::Settings;

const PREMIUM_NEEDED: &str = "Playing on this computer needs Spotify Premium.";
const PAGE_SIZE: u32 = 50;

#[derive(Clone, Debug, PartialEq)]
pub enum AuthStatus {
    Starting,
    SignedOut,
    WaitingForBrowser { url: String },
    Connecting,
    Connected { username: String },
    Failed(String),
}

#[derive(Clone, Debug, PartialEq)]
pub enum LocalPlayback {
    Unavailable,
    Authorizing,
    Connecting,
    Ready { device_id: String },
    Failed(String),
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Page {
    Home,
    Search,
    Liked,
    MadeForYou,
    Albums,
    Artists,
    Playlist(String),
    Album(String),
    Artist(String),
    Queue,
    Settings,
}

impl Page {
    pub fn encode(&self) -> String {
        match self {
            Page::Home => "home".into(),
            Page::Search => "search".into(),
            Page::Liked => "liked".into(),
            Page::MadeForYou => "made-for-you".into(),
            Page::Albums => "albums".into(),
            Page::Artists => "artists".into(),
            Page::Playlist(id) => format!("playlist:{id}"),
            Page::Album(id) => format!("album:{id}"),
            Page::Artist(id) => format!("artist:{id}"),
            Page::Queue => "queue".into(),
            Page::Settings => "settings".into(),
        }
    }

    pub fn decode(text: &str) -> Option<Self> {
        let text = text.trim();
        if text.is_empty() {
            return Some(Page::Home);
        }
        Some(match text {
            "home" => Page::Home,
            "search" => Page::Search,
            "liked" => Page::Liked,
            "made-for-you" => Page::MadeForYou,
            "albums" => Page::Albums,
            "artists" => Page::Artists,
            "queue" => Page::Queue,
            "settings" => Page::Settings,
            other => {
                if let Some(id) = other.strip_prefix("playlist:") {
                    Page::Playlist(id.to_string())
                } else if let Some(id) = other.strip_prefix("album:") {
                    Page::Album(id.to_string())
                } else if let Some(id) = other.strip_prefix("artist:") {
                    Page::Artist(id.to_string())
                } else {
                    return None;
                }
            }
        })
    }

    pub fn cache_key(&self) -> Option<String> {
        match self {
            Page::Search | Page::Queue | Page::Settings => None,
            Page::Home => Some("home".into()),
            Page::Liked => Some("liked".into()),
            Page::MadeForYou => Some("made-for-you".into()),
            Page::Albums => Some("albums".into()),
            Page::Artists => Some("artists".into()),
            Page::Playlist(id) => Some(format!("playlist-{id}")),
            Page::Album(id) => Some(format!("album-{id}")),
            Page::Artist(id) => Some(format!("artist-{id}")),
        }
    }

    pub fn persist_as_last(&self) -> bool {
        !matches!(self, Page::Search | Page::Queue | Page::Settings)
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum PageBody {
    Home {
        recent: Vec<Track>,
        playlists: Vec<Playlist>,
        top_artists: Vec<Artist>,
        top_tracks: Vec<Track>,
    },
    Search(SearchResults),
    Playlists {
        title: String,
        subtitle: String,
        items: Vec<Playlist>,
    },
    Tracks {
        title: String,
        subtitle: String,
        art: Option<String>,
        context_uri: Option<String>,
        items: Vec<Track>,
        total: u32,
        offset: u32,
        /// Set when this collection is an album page (Save / Follow chrome).
        #[serde(default)]
        album: Option<Album>,
    },
    Albums {
        items: Vec<Album>,
        total: u32,
        offset: u32,
    },
    Artists {
        items: Vec<Artist>,
    },
    Artist {
        artist: Artist,
        top: Vec<Track>,
        albums: Vec<Album>,
    },
    Queue {
        items: Vec<PlayableItem>,
    },
    Settings,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct NowPlaying {
    pub playback: Playback,
    pub title: String,
    pub artists: String,
    pub album: String,
    /// `(name, catalog id)` for each linked artist on the playing track.
    pub artist_links: Vec<(String, String)>,
    pub album_id: Option<String>,
    pub uri: String,
    pub art_url: Option<String>,
    pub duration_ms: u32,
    pub position_ms: u32,
    pub volume_percent: u8,
    pub shuffle: bool,
    pub repeat: RepeatMode,
    pub device_name: String,
    pub device_id: Option<String>,
    pub is_local: bool,
    pub saved: Option<bool>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FavoriteKind {
    Album,
    Artist,
}

#[derive(Debug, Clone)]
pub enum Cmd {
    SignIn,
    CancelSignIn,
    SignOut,
    AuthorizePlayback,
    Player(PlayerCommand),
    Play {
        request: PlayRequest,
        device_id: Option<String>,
    },
    Transfer {
        device_id: String,
        play: bool,
    },
    Open(Page),
    Search(String),
    LoadMore,
    FetchArt(String),
    SetSaved {
        uri: String,
        saved: bool,
    },
    SetFavorite {
        kind: FavoriteKind,
        id: String,
        saved: bool,
    },
    SetSkipped {
        uri: String,
        skipped: bool,
    },
    AddToPlaylist {
        playlist_id: String,
        name: String,
        uris: Vec<String>,
    },
    CreatePlaylist {
        name: String,
        uris: Vec<String>,
    },
    RefreshPlayback,
    RefreshDevices,
    SaveSettings(Settings),
    Media(MediaCommand),
    Shutdown,
}

#[derive(Debug, Clone)]
pub enum Event {
    Auth(AuthStatus),
    User(User),
    Premium(Option<bool>),
    LocalPlayback(LocalPlayback),
    Page {
        page: Page,
        body: PageBody,
    },
    NowPlaying(NowPlaying),
    Devices(Vec<Device>),
    Art {
        url: String,
        bytes: Arc<[u8]>,
    },
    Saved {
        uri: String,
        saved: bool,
    },
    Favorite {
        kind: FavoriteKind,
        id: String,
        saved: bool,
    },
    /// Full Liked Songs snapshot (uris). Replaces the UI's liked set.
    Liked(Vec<String>),
    /// Library playlists (sidebar + Home / Made for you), filled after first paint.
    Playlists(Vec<Playlist>),
    AddedToPlaylist {
        playlist_id: String,
        name: String,
        uris: Vec<String>,
    },
    Settings(Settings),
    Error(String),
    Raise,
    Quit,
}

enum Internal {
    WebSignedIn(StoredToken),
    PlaybackAuthorized(String),
    EngineConnected {
        engine: Option<Engine>,
        error: Option<String>,
    },
    Reconnect,
    EngineState(crate::player::LocalState),
    MaybeAdvance {
        uri: String,
        generation: u64,
    },
    /// `/me` succeeded; store it on the worker so poll stops asking.
    ProfileOk(User),
    /// `/me` 429/quota; keep the session, do not poll again immediately.
    ProfileSkipped,
    /// `/me` said the grant is dead; do not keep polling.
    ProfileFailed,
    /// Check that a Load after reconnect (or a click) actually started.
    VerifyResume,
}

pub fn start() {
    std::thread::Builder::new()
        .name("sola-spotify-worker".into())
        .spawn(|| {
            let rt = tokio::runtime::Builder::new_multi_thread()
                .enable_all()
                .thread_name("sola-spotify-rt")
                .build()
                .expect("spotify tokio runtime");
            rt.block_on(run());
        })
        .expect("spawn spotify worker");
}

async fn run() {
    let dirs = AppDirs::discover();
    if let Err(e) = dirs.ensure() {
        tracing::warn!("spotify dirs: {e}");
    }
    let settings = Settings::load(&dirs);
    bridge::emit(Event::Settings(settings.clone()));

    let http = reqwest::Client::builder()
        .user_agent("sola-spotify/0.1")
        .build()
        .expect("http client");
    let activity = Arc::new(NetActivity::default());
    let api = Arc::new(ApiClient::new(
        http.clone(),
        activity,
        20,
        50,
        ApiSource::Shared,
    ));
    let art = ArtLoader::new(http.clone(), dirs.art_cache_dir());

    let (int_tx, mut int_rx) = mpsc::unbounded_channel::<Internal>();
    let std_rx = bridge::take_cmd_rx();
    let (cmd_tx, mut cmd_rx) = mpsc::unbounded_channel::<Cmd>();
    std::thread::spawn(move || {
        while let Ok(cmd) = std_rx.recv() {
            if cmd_tx.send(cmd).is_err() {
                break;
            }
        }
    });

    let (mut mpris, mut media_rx) = MediaService::spawn();
    // Drop the MPRIS branch if the D-Bus thread dies — a closed channel
    // would otherwise ready-spin the worker loop.
    let mut mpris_commands = true;

    let mut worker = Worker {
        skipped: Skipped::load(&dirs),
        liked: Liked::load(&dirs),
        dirs,
        settings,
        http,
        api,
        art,
        int_tx,
        engine: None,
        engine_busy: false,
        signed_in: false,
        premium: None,
        user: None,
        profile_inflight: false,
        profile_retry_at: None,
        cancel_signin: None,
        reconnects: Vec::new(),
        resume: None,
        resume_verify: None,
        pending_play: None,
        play_uris: Vec::new(),
        play_index: 0,
        last_playback: Playback::Stopped,
        advance_gen: 0,
        hold: std::sync::Arc::new(std::sync::Mutex::new(EngineHold::default())),
        last_engine_uri: None,
        now: NowPlaying::default(),
        devices: Vec::new(),
        last_devices_at: Instant::now(),
        page: Page::Home,
        page_offset: 0,
        page_total: 0,
        search_query: String::new(),
    };

    worker.restore_session();

    let mut tick = tokio::time::interval(Duration::from_secs(2));
    tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

    loop {
        tokio::select! {
            cmd = cmd_rx.recv() => {
                let Some(cmd) = cmd else { break; };
                if matches!(cmd, Cmd::Shutdown) {
                    break;
                }
                worker.handle(cmd).await;
            }
            internal = int_rx.recv() => {
                let Some(internal) = internal else { break; };
                worker.handle_internal(internal).await;
            }
            // Media keys used to drain only on the 2s playback poll.
            media = media_rx.recv(), if mpris_commands => {
                let Some(media) = media else {
                    mpris_commands = false;
                    continue;
                };
                worker.handle(Cmd::Media(media)).await;
                while let Ok(more) = media_rx.try_recv() {
                    worker.handle(Cmd::Media(more)).await;
                }
            }
            _ = tick.tick() => {
                worker.poll().await;
            }
        }
        mpris.update(worker.media_state());
    }
    if let Some(engine) = worker.engine.take() {
        engine.shutdown();
    }
}

struct Worker {
    dirs: AppDirs,
    settings: Settings,
    http: reqwest::Client,
    api: Arc<ApiClient>,
    art: ArtLoader,
    int_tx: mpsc::UnboundedSender<Internal>,
    engine: Option<Engine>,
    engine_busy: bool,
    signed_in: bool,
    premium: Option<bool>,
    user: Option<User>,
    profile_inflight: bool,
    profile_retry_at: Option<Instant>,
    cancel_signin: Option<watch::Sender<bool>>,
    reconnects: Vec<Instant>,
    resume: Option<LoadSpec>,
    /// Load in flight after reconnect / a click; retry if it never starts.
    resume_verify: Option<(LoadSpec, u8)>,
    /// Play the user asked for before local playback was up (no Connect device).
    pending_play: Option<PlayRequest>,
    play_uris: Vec<String>,
    play_index: usize,
    last_playback: Playback,
    advance_gen: u64,
    skipped: Skipped,
    liked: Liked,
    /// Drop engine snapshots for any other track while a click/skip settles.
    hold: std::sync::Arc<std::sync::Mutex<EngineHold>>,
    last_engine_uri: Option<String>,
    now: NowPlaying,
    devices: Vec<Device>,
    last_devices_at: Instant,
    page: Page,
    page_offset: u32,
    page_total: u32,
    search_query: String,
}

impl Worker {
    fn engine_config(&self) -> EngineConfig {
        EngineConfig {
            device_name: self.settings.device_name.clone(),
            bitrate_kbps: self.settings.bitrate_kbps,
            normalisation: self.settings.normalisation,
            autoplay: self.settings.autoplay,
            gapless: self.settings.gapless,
            backend: None,
            audio_device: None,
            initial_volume: 40_000,
            credentials_dir: self.dirs.credentials_dir(),
            volume_dir: self.dirs.volume_dir(),
            audio_cache_dir: Some(self.dirs.audio_cache_dir()),
            audio_cache_limit: Some(2 * 1024 * 1024 * 1024),
        }
    }

    fn restore_session(&mut self) {
        match StoredToken::load(&self.dirs.shared_web_token_file()) {
            Some(token) if token.has_scopes(auth::WEB_SCOPES) => {
                bridge::emit(Event::Auth(AuthStatus::Connecting));
                self.on_web_signed_in(token);
            }
            Some(_) => bridge::emit(Event::Auth(AuthStatus::Failed(
                "Spotify permissions changed. Sign in again.".into(),
            ))),
            None => bridge::emit(Event::Auth(AuthStatus::SignedOut)),
        }
    }

    fn on_web_signed_in(&mut self, token: StoredToken) {
        let tokens = WebTokens::new(
            self.http.clone(),
            token,
            self.dirs.shared_web_token_file(),
            ApiSource::Shared,
        );
        self.api
            .set_token_provider(Some(TokenProvider::Web(tokens)));
        self.refresh_me();
        self.signed_in = true;
        self.resume_engine();
        if !self.liked.uris.is_empty() {
            bridge::emit(Event::Liked(self.liked.uris.iter().cloned().collect()));
        }
        let last = Page::decode(&self.settings.last_page).unwrap_or(Page::Home);
        self.open(last);
        self.refresh_library_playlists();
        self.refresh_devices();
        self.refresh_playback();
        self.refresh_liked();
    }

    async fn handle(&mut self, cmd: Cmd) {
        match cmd {
            Cmd::SignIn => self.sign_in(),
            Cmd::CancelSignIn => {
                if let Some(cancel) = self.cancel_signin.take() {
                    let _ = cancel.send(true);
                }
                if !self.signed_in {
                    bridge::emit(Event::Auth(AuthStatus::SignedOut));
                }
            }
            Cmd::SignOut => self.sign_out(),
            Cmd::AuthorizePlayback => self.authorize_playback(),
            Cmd::Player(command) => self.player(command),
            Cmd::Play { request, device_id } => self.play(request, device_id).await,
            Cmd::Transfer { device_id, play } => self.transfer(device_id, play).await,
            Cmd::Open(page) => self.open(page),
            Cmd::Search(query) => {
                self.search_query = query;
                self.open(Page::Search);
            }
            Cmd::LoadMore => self.load_more(),
            Cmd::FetchArt(url) => self.fetch_art(url),
            Cmd::SetSaved { uri, saved } => self.set_saved(uri, saved),
            Cmd::SetFavorite { kind, id, saved } => self.set_favorite(kind, id, saved),
            Cmd::SetSkipped { uri, skipped } => self.set_skipped(uri, skipped),
            Cmd::AddToPlaylist {
                playlist_id,
                name,
                uris,
            } => self.add_to_playlist(playlist_id, name, uris),
            Cmd::CreatePlaylist { name, uris } => self.create_playlist(name, uris),
            Cmd::RefreshPlayback => self.refresh_playback(),
            Cmd::RefreshDevices => self.refresh_devices(),
            Cmd::SaveSettings(settings) => {
                settings.save(&self.dirs);
                self.settings = settings.clone();
                bridge::emit(Event::Settings(settings));
                if self.engine.is_some() {
                    self.reconnect_engine().await;
                }
            }
            Cmd::Media(command) => self.media(command).await,
            Cmd::Shutdown => {}
        }
    }

    async fn handle_internal(&mut self, internal: Internal) {
        match internal {
            Internal::WebSignedIn(token) => {
                if let Err(error) = token.save(&self.dirs.shared_web_token_file()) {
                    tracing::warn!("unable to save Spotify sign-in: {error}");
                }
                self.cancel_signin = None;
                self.on_web_signed_in(token);
            }
            Internal::PlaybackAuthorized(access_token) => {
                self.cancel_signin = None;
                self.connect_engine(Credentials::with_access_token(access_token));
            }
            Internal::EngineConnected { engine, error } => {
                self.engine_busy = false;
                match (engine, error) {
                    (Some(engine), _) => {
                        let device_id = engine.device_id().to_string();
                        tracing::info!(device_id = %device_id, "local playback ready");
                        self.reconnects.clear();
                        let pending = self.pending_play.take();
                        let spec = if let Some(request) = pending {
                            self.hold_play(&request);
                            self.remember_queue(&request);
                            Some(load_spec(&request))
                        } else {
                            self.resume.take()
                        };
                        // An early Load while Spirc is still registering 400s
                        // and leaves the device inactive — later clicks then
                        // no-op. Wait, then verify it actually started.
                        if let Some(spec) = spec {
                            self.arm_resume(spec, Duration::from_millis(1_500));
                        }
                        self.engine = Some(engine);
                        bridge::emit(Event::LocalPlayback(LocalPlayback::Ready { device_id }));
                    }
                    (None, Some(error)) => {
                        tracing::warn!("local playback failed: {error}");
                        self.resume = None;
                        self.resume_verify = None;
                        bridge::emit(Event::LocalPlayback(LocalPlayback::Failed(error)));
                    }
                    _ => {
                        self.resume = None;
                        self.resume_verify = None;
                        bridge::emit(Event::LocalPlayback(LocalPlayback::Unavailable));
                    }
                }
            }
            Internal::Reconnect => self.reconnect_engine().await,
            Internal::EngineState(state) => self.on_engine_state(state),
            Internal::MaybeAdvance { uri, generation } => self.maybe_advance(uri, generation),
            Internal::ProfileOk(user) => self.on_profile(user),
            Internal::ProfileSkipped => self.on_profile_skipped(),
            Internal::ProfileFailed => {
                self.profile_inflight = false;
                self.profile_retry_at = Some(Instant::now() + Duration::from_secs(300));
            }
            Internal::VerifyResume => self.verify_resume(),
        }
    }

    fn sign_in(&mut self) {
        if self.cancel_signin.is_some() {
            return;
        }
        let grant = Grant::shared_web_api();
        let flow = auth::begin(grant.clone());
        let (cancel_tx, cancel_rx) = watch::channel(false);
        self.cancel_signin = Some(cancel_tx);
        bridge::emit(Event::Auth(AuthStatus::WaitingForBrowser {
            url: flow.url.clone(),
        }));
        let url = flow.url.clone();
        tokio::task::spawn_blocking(move || {
            if let Err(e) = sola_core::open_url::open(&url) {
                tracing::warn!("open Spotify sign-in: {e}");
            }
        });
        let http = self.http.clone();
        let int_tx = self.int_tx.clone();
        tokio::spawn(async move {
            let result = async {
                let code = auth::wait_for_code(grant.redirect_port, &flow.state, cancel_rx).await?;
                let response = auth::exchange_code(&http, &grant, &code, &flow.verifier).await?;
                StoredToken::from_response(&grant.client_id, response, None)
            }
            .await;
            match result {
                Ok(token) => {
                    let _ = int_tx.send(Internal::WebSignedIn(token));
                }
                Err(error) => {
                    let message = error.to_string();
                    if !message.contains("cancelled") {
                        bridge::emit(Event::Auth(AuthStatus::Failed(format!(
                            "Sign-in failed: {message}"
                        ))));
                    } else {
                        bridge::emit(Event::Auth(AuthStatus::SignedOut));
                    }
                }
            }
        });
    }

    fn sign_out(&mut self) {
        self.signed_in = false;
        self.user = None;
        self.premium = None;
        self.profile_inflight = false;
        self.profile_retry_at = None;
        self.resume_verify = None;
        if let Some(engine) = self.engine.take() {
            engine.shutdown();
        }
        if let Some(cancel) = self.cancel_signin.take() {
            let _ = cancel.send(true);
        }
        self.api.set_token_provider(None);
        StoredToken::remove(&self.dirs.shared_web_token_file());
        let _ = std::fs::remove_file(self.dirs.credentials_dir().join("credentials.json"));
        self.pending_play = None;
        bridge::emit(Event::LocalPlayback(LocalPlayback::Unavailable));
        bridge::emit(Event::Auth(AuthStatus::SignedOut));
        self.now = NowPlaying::default();
        bridge::emit(Event::NowPlaying(self.now.clone()));
    }

    fn authorize_playback(&mut self) {
        if self.engine_busy || self.cancel_signin.is_some() {
            return;
        }
        if self.premium == Some(false) {
            bridge::emit(Event::LocalPlayback(LocalPlayback::Failed(
                PREMIUM_NEEDED.into(),
            )));
            return;
        }
        let grant = Grant::playback();
        let flow = auth::begin(grant.clone());
        let (cancel_tx, cancel_rx) = watch::channel(false);
        self.cancel_signin = Some(cancel_tx);
        bridge::emit(Event::LocalPlayback(LocalPlayback::Authorizing));
        let url = flow.url.clone();
        tokio::task::spawn_blocking(move || {
            if let Err(e) = sola_core::open_url::open(&url) {
                tracing::warn!("open playback sign-in: {e}");
            }
        });
        let http = self.http.clone();
        let int_tx = self.int_tx.clone();
        tokio::spawn(async move {
            let result = async {
                let code = auth::wait_for_code(grant.redirect_port, &flow.state, cancel_rx).await?;
                auth::exchange_code(&http, &grant, &code, &flow.verifier).await
            }
            .await;
            match result {
                Ok(token) => {
                    let _ = int_tx.send(Internal::PlaybackAuthorized(token.access_token));
                }
                Err(error) => {
                    let message = error.to_string();
                    if message.contains("cancelled") {
                        bridge::emit(Event::LocalPlayback(LocalPlayback::Unavailable));
                    } else {
                        bridge::emit(Event::LocalPlayback(LocalPlayback::Failed(message)));
                    }
                }
            }
        });
    }

    fn resume_engine(&mut self) {
        if self.engine.is_some() || self.engine_busy || self.premium == Some(false) {
            return;
        }
        let credentials = self
            .engine_config()
            .open_cache()
            .ok()
            .and_then(|cache| cache.credentials());
        if let Some(credentials) = credentials {
            self.connect_engine(credentials);
        }
    }

    async fn reconnect_engine(&mut self) {
        if !self.signed_in {
            return;
        }
        self.resume_verify = None;
        if let Some(engine) = self.engine.take() {
            self.resume = engine.interrupted().map(|interrupted| LoadSpec {
                uris: vec![interrupted.uri],
                position_ms: interrupted.position_ms,
                play: interrupted.playing,
                ..LoadSpec::default()
            });
            engine.shutdown_wait().await;
        }
        let now = Instant::now();
        self.reconnects
            .retain(|attempt| now.duration_since(*attempt) < Duration::from_secs(600));
        if self.reconnects.len() >= 6 {
            self.resume = None;
            bridge::emit(Event::LocalPlayback(LocalPlayback::Failed(
                "Local playback keeps dropping. Set up playback again from Settings.".into(),
            )));
            return;
        }
        self.reconnects.push(now);
        tracing::info!(
            attempt = self.reconnects.len(),
            "local playback session ended; reconnecting"
        );
        self.resume_engine();
    }

    fn connect_engine(&mut self, credentials: Credentials) {
        if self.engine_busy {
            return;
        }
        if self.premium == Some(false) {
            bridge::emit(Event::LocalPlayback(LocalPlayback::Failed(
                PREMIUM_NEEDED.into(),
            )));
            return;
        }
        self.cancel_signin = None;
        self.engine_busy = true;
        bridge::emit(Event::LocalPlayback(LocalPlayback::Connecting));
        let config = self.engine_config();
        let int_tx = self.int_tx.clone();
        let notify: crate::player::Notify = {
            let int_tx = int_tx.clone();
            let hold = Arc::clone(&self.hold);
            Arc::new(move |event| match event {
                EngineEvent::State(state) => {
                    if !hold_allows(&hold, &state) {
                        return;
                    }
                    apply_local_state(state.clone());
                    let _ = int_tx.send(Internal::EngineState(state));
                }
                EngineEvent::SessionEnded => {
                    let _ = int_tx.send(Internal::Reconnect);
                }
            })
        };
        tokio::spawn(async move {
            let cache = match config.open_cache() {
                Ok(cache) => cache,
                Err(error) => {
                    let _ = int_tx.send(Internal::EngineConnected {
                        engine: None,
                        error: Some(error.to_string()),
                    });
                    return;
                }
            };
            let attempt = tokio::time::timeout(
                Duration::from_secs(45),
                Engine::connect(&config, credentials, cache, notify),
            )
            .await;
            let outcome = match attempt {
                Ok(Ok(engine)) => Internal::EngineConnected {
                    engine: Some(engine),
                    error: None,
                },
                Ok(Err(error)) => Internal::EngineConnected {
                    engine: None,
                    error: Some(error.to_string()),
                },
                Err(_) => Internal::EngineConnected {
                    engine: None,
                    error: Some("Connecting to Spotify timed out.".into()),
                },
            };
            let _ = int_tx.send(outcome);
        });
    }

    fn player(&self, command: PlayerCommand) {
        match &self.engine {
            Some(engine) => {
                if let Err(error) = engine.command(command) {
                    bridge::emit(Event::Error(format!("Playback error: {error}")));
                }
            }
            None => bridge::emit(Event::Error(
                "Local playback isn't set up on this computer yet.".into(),
            )),
        }
    }

    async fn play(&mut self, request: PlayRequest, device_id: Option<String>) {
        self.hold_play(&request);
        self.remember_queue(&request);
        if let Some(engine) = &self.engine {
            let spec = load_spec(&request);
            if let Err(error) = engine.command(PlayerCommand::Load(spec.clone())) {
                bridge::emit(Event::Error(format!("Playback error: {error}")));
            }
            self.arm_resume(spec, Duration::from_millis(2_000));
            return;
        }
        // An explicit Connect target (phone, speaker, Electron) — not the
        // fallback "whatever was active last". Playing here with no local
        // engine and no chosen device used to 404 NO_ACTIVE_DEVICE.
        if let Some(device) = device_id {
            let api = Arc::clone(&self.api);
            tokio::spawn(async move {
                if let Err(error) = api.play(Some(&device), Some(&request)).await {
                    bridge::emit(Event::Error(friendly_player_error(&error)));
                }
            });
            return;
        }
        self.play_here(request);
    }

    /// Bring up local playback, then play `request` once the engine is live.
    fn play_here(&mut self, request: PlayRequest) {
        if self.premium == Some(false) {
            bridge::emit(Event::Error(PREMIUM_NEEDED.into()));
            return;
        }
        if request.context_uri.is_some() || !request.uris.is_empty() {
            self.pending_play = Some(request);
        }
        if self.engine_busy {
            return;
        }
        let credentials = self
            .engine_config()
            .open_cache()
            .ok()
            .and_then(|cache| cache.credentials());
        if let Some(credentials) = credentials {
            self.connect_engine(credentials);
        } else {
            self.authorize_playback();
        }
    }

    async fn transfer(&self, device_id: String, play: bool) {
        if let Some(engine) = &self.engine
            && engine.device_id() == device_id
        {
            let _ = engine.command(PlayerCommand::Activate);
            if play {
                let _ = engine.command(PlayerCommand::Play);
            }
            return;
        }
        let api = Arc::clone(&self.api);
        tokio::spawn(async move {
            if let Err(error) = api.transfer(&device_id, play).await {
                bridge::emit(Event::Error(friendly_player_error(&error)));
            }
        });
    }

    fn open(&mut self, page: Page) {
        self.page = page.clone();
        self.page_offset = 0;
        self.emit_cached(&page);
        match &page {
            Page::Home => self.load_home(),
            Page::MadeForYou => self.load_made_for_you(),
            Page::Search => self.load_search(),
            Page::Liked => self.load_liked(0),
            Page::Albums => self.load_albums(0),
            Page::Artists => self.load_artists(),
            Page::Playlist(id) => self.load_playlist(id.clone(), 0),
            Page::Album(id) => self.load_album(id.clone()),
            Page::Artist(id) => self.load_artist(id.clone()),
            Page::Queue => self.load_queue(),
            Page::Settings => {
                bridge::emit(Event::Page {
                    page: Page::Settings,
                    body: PageBody::Settings,
                });
            }
        }
    }

    fn emit_cached(&self, page: &Page) {
        let Some(key) = page.cache_key() else {
            return;
        };
        let path = self.dirs.page_cache_dir().join(format!("{key}.json"));
        if let Some(body) = cache::read_json::<PageBody>(&path) {
            bridge::emit(Event::Page {
                page: page.clone(),
                body,
            });
        }
    }

    fn load_more(&mut self) {
        if self.page_offset + PAGE_SIZE >= self.page_total && self.page_total > 0 {
            return;
        }
        let next = self.page_offset + PAGE_SIZE;
        match self.page.clone() {
            Page::Liked => self.load_liked(next),
            Page::Albums => self.load_albums(next),
            Page::Playlist(id) => self.load_playlist(id, next),
            _ => {}
        }
    }

    fn load_home(&self) {
        let api = Arc::clone(&self.api);
        tokio::spawn(async move {
            let (recent, playlists, top_artists, top_tracks) = tokio::join!(
                api.recently_played(20, None, None),
                Worker::library_playlists_fast(&api),
                api.top_artists("medium_term", 12),
                api.top_tracks("medium_term", 10, 0),
            );
            let recent = match recent {
                Ok(page) => page.items.into_iter().map(|h| h.track).collect(),
                Err(error) => {
                    bridge::emit(Event::Error(error.to_string()));
                    Vec::new()
                }
            };
            let top_artists = top_artists.map(|p| p.items).unwrap_or_default();
            let top_tracks = top_tracks.map(|p| p.items).unwrap_or_default();
            let like_uris: Vec<String> = recent
                .iter()
                .chain(top_tracks.iter())
                .filter(|t| !t.uri.is_empty())
                .map(|t| t.uri.clone())
                .collect();
            bridge::emit(Event::Page {
                page: Page::Home,
                body: PageBody::Home {
                    recent,
                    playlists: playlists.clone(),
                    top_artists,
                    top_tracks,
                },
            });
            Worker::emit_contains(Arc::clone(&api), like_uris);
            Worker::library_playlists_rest(api, playlists).await;
        });
    }

    fn merge_playlists(mut into: Vec<Playlist>, extra: Vec<Playlist>) -> Vec<Playlist> {
        let mut seen: std::collections::HashSet<String> =
            into.iter().map(|p| p.id.clone()).collect();
        for playlist in extra {
            if seen.insert(playlist.id.clone()) {
                into.push(playlist);
            }
        }
        into
    }

    async fn library_playlists_fast(api: &ApiClient) -> Vec<Playlist> {
        let (mine, extra) =
            tokio::join!(api.my_playlists(0, 50), async { api.made_for_you().await });
        let mine = mine.map(|page| page.items).unwrap_or_default();
        Worker::merge_playlists(mine, extra)
    }

    async fn library_playlists_rest(api: Arc<ApiClient>, first: Vec<Playlist>) {
        let mut playlists = first;
        let mut offset = 50_u32;
        for _ in 0..3 {
            match api.my_playlists(offset, 50).await {
                Ok(page) => {
                    let n = page.items.len() as u32;
                    if n == 0 {
                        break;
                    }
                    playlists = Worker::merge_playlists(playlists, page.items);
                    offset += 50;
                    if n < 50 || page.next.is_none() {
                        break;
                    }
                }
                Err(_) => break,
            }
        }
        bridge::emit(Event::Playlists(playlists));
    }

    fn refresh_library_playlists(&self) {
        let api = Arc::clone(&self.api);
        tokio::spawn(async move {
            let first = Worker::library_playlists_fast(&api).await;
            bridge::emit(Event::Playlists(first.clone()));
            Worker::library_playlists_rest(api, first).await;
        });
    }

    fn load_made_for_you(&self) {
        let api = Arc::clone(&self.api);
        tokio::spawn(async move {
            let playlists = Worker::library_playlists_fast(&api).await;
            bridge::emit(Event::Page {
                page: Page::MadeForYou,
                body: PageBody::Playlists {
                    title: "Made for you".into(),
                    subtitle: "Mixes Spotify builds from what you play.".into(),
                    items: playlists.clone(),
                },
            });
            Worker::library_playlists_rest(api, playlists).await;
        });
    }

    fn load_search(&self) {
        let query = self.search_query.trim().to_string();
        if query.is_empty() {
            bridge::emit(Event::Page {
                page: Page::Search,
                body: PageBody::Search(SearchResults::default()),
            });
            return;
        }
        let api = Arc::clone(&self.api);
        tokio::spawn(async move {
            match api
                .search(
                    &query,
                    &["track", "artist", "album", "playlist", "show", "episode"],
                )
                .await
            {
                Ok(results) => {
                    let like_uris: Vec<String> = results
                        .tracks
                        .as_ref()
                        .map(|page| {
                            page.items
                                .iter()
                                .filter(|t| !t.uri.is_empty())
                                .map(|t| t.uri.clone())
                                .collect()
                        })
                        .unwrap_or_default();
                    bridge::emit(Event::Page {
                        page: Page::Search,
                        body: PageBody::Search(results),
                    });
                    Worker::emit_contains(api, like_uris);
                }
                Err(error) => bridge::emit(Event::Error(error.to_string())),
            }
        });
    }

    fn load_liked(&mut self, offset: u32) {
        let api = Arc::clone(&self.api);
        tokio::spawn(async move {
            match api.saved_tracks(offset, PAGE_SIZE).await {
                Ok(page) => {
                    let items: Vec<Track> = page
                        .items
                        .into_iter()
                        .map(|s| {
                            let mut track = s.track;
                            if track.added_at.is_none() {
                                track.added_at = s.added_at;
                            }
                            track
                        })
                        .collect();
                    for track in &items {
                        if !track.uri.is_empty() {
                            bridge::emit(Event::Saved {
                                uri: track.uri.clone(),
                                saved: true,
                            });
                        }
                    }
                    bridge::emit(Event::Page {
                        page: Page::Liked,
                        body: PageBody::Tracks {
                            title: "Liked Songs".into(),
                            subtitle: {
                                let mut line = format!("{} songs", page.total);
                                if let Some(span) = crate::api::models::added_span(&items) {
                                    line.push_str(" · ");
                                    line.push_str(&span);
                                }
                                line
                            },
                            art: None,
                            context_uri: Some("spotify:collection:tracks".into()),
                            items,
                            total: page.total,
                            offset: page.offset,
                            album: None,
                        },
                    });
                }
                Err(error) => bridge::emit(Event::Error(error.to_string())),
            }
        });
        self.page_offset = offset;
    }

    fn load_albums(&mut self, offset: u32) {
        let api = Arc::clone(&self.api);
        tokio::spawn(async move {
            match api.saved_albums(offset, PAGE_SIZE).await {
                Ok(page) => {
                    let items: Vec<Album> = page.items.into_iter().map(|s| s.album).collect();
                    bridge::emit(Event::Page {
                        page: Page::Albums,
                        body: PageBody::Albums {
                            items,
                            total: page.total,
                            offset: page.offset,
                        },
                    });
                }
                Err(error) => bridge::emit(Event::Error(error.to_string())),
            }
        });
        self.page_offset = offset;
    }

    fn load_artists(&self) {
        let api = Arc::clone(&self.api);
        tokio::spawn(async move {
            match api.followed_artists(None, 50).await {
                Ok(page) => bridge::emit(Event::Page {
                    page: Page::Artists,
                    body: PageBody::Artists { items: page.items },
                }),
                Err(error) => bridge::emit(Event::Error(error.to_string())),
            }
        });
    }

    fn load_playlist(&mut self, id: String, offset: u32) {
        let api = Arc::clone(&self.api);
        tokio::spawn(async move {
            let (playlist, items) = tokio::join!(
                api.playlist(&id),
                api.playlist_items(&id, offset, PAGE_SIZE),
            );
            let playlist = match playlist {
                Ok(p) => p,
                Err(error) => {
                    bridge::emit(Event::Error(error.to_string()));
                    return;
                }
            };
            match items {
                Ok(page) => {
                    let items: Vec<Track> = page
                        .items
                        .iter()
                        .filter_map(PlaylistItem::into_track)
                        .collect();
                    let art =
                        crate::api::models::pick_image(&playlist.images, 300).map(str::to_string);
                    let title = playlist.name.clone();
                    let mut subtitle = format!(
                        "{} · {} songs",
                        playlist.owner_name(),
                        playlist.track_total()
                    );
                    if playlist.is_generated() {
                        subtitle.push_str(" · Made for you");
                    }
                    if let Some(span) = crate::api::models::added_span(&items) {
                        subtitle.push_str(" · ");
                        subtitle.push_str(&span);
                    }
                    let total = page.total.max(playlist.track_total());
                    let context_uri = if playlist.uri.is_empty() {
                        format!("spotify:playlist:{id}")
                    } else {
                        playlist.uri.clone()
                    };
                    let like_uris: Vec<String> = items
                        .iter()
                        .filter(|t| !t.uri.is_empty())
                        .map(|t| t.uri.clone())
                        .collect();
                    bridge::emit(Event::Page {
                        page: Page::Playlist(id.clone()),
                        body: PageBody::Tracks {
                            title,
                            subtitle,
                            art,
                            context_uri: Some(context_uri),
                            items,
                            total,
                            offset: page.offset,
                            album: None,
                        },
                    });
                    Worker::emit_contains(api.clone(), like_uris);
                }
                Err(error) => bridge::emit(Event::Error(error.to_string())),
            }
        });
        self.page_offset = offset;
    }

    fn load_album(&self, id: String) {
        let api = Arc::clone(&self.api);
        tokio::spawn(async move {
            match api.album(&id).await {
                Ok(album) => {
                    let items = match &album.tracks {
                        Some(page) if !page.items.is_empty() => {
                            let mut tracks = page.items.clone();
                            for track in &mut tracks {
                                if track.album.is_none() {
                                    track.album = Some(album.clone());
                                }
                            }
                            tracks
                        }
                        _ => match api.album_tracks(&id, 0, 50).await {
                            Ok(page) => {
                                let mut tracks = page.items;
                                for track in &mut tracks {
                                    if track.album.is_none() {
                                        track.album = Some(album.clone());
                                    }
                                }
                                tracks
                            }
                            Err(error) => {
                                bridge::emit(Event::Error(error.to_string()));
                                return;
                            }
                        },
                    };
                    let art =
                        crate::api::models::pick_image(&album.images, 300).map(str::to_string);
                    let year = album.year().unwrap_or("").to_string();
                    let artists = crate::api::models::join_names(
                        album.artists.iter().map(|a| a.name.as_str()),
                    );
                    let like_uris: Vec<String> = items
                        .iter()
                        .filter(|t| !t.uri.is_empty())
                        .map(|t| t.uri.clone())
                        .collect();
                    let mut header = album.clone();
                    header.tracks = None;
                    let album_id = header
                        .catalog_id()
                        .map(str::to_string)
                        .unwrap_or_else(|| id.clone());
                    let artist_id = header
                        .artists
                        .iter()
                        .find_map(|artist| artist.catalog_id().map(str::to_string));
                    let context_uri = if album.uri.is_empty() {
                        format!("spotify:album:{id}")
                    } else {
                        album.uri.clone()
                    };
                    bridge::emit(Event::Page {
                        page: Page::Album(id),
                        body: PageBody::Tracks {
                            title: album.name,
                            subtitle: format!("{artists} · {year}"),
                            art,
                            context_uri: Some(context_uri),
                            total: items.len() as u32,
                            offset: 0,
                            items,
                            album: Some(header),
                        },
                    });
                    Worker::emit_contains(api.clone(), like_uris);
                    Worker::emit_library_state(api, Some(album_id), artist_id);
                }
                Err(error) => bridge::emit(Event::Error(error.to_string())),
            }
        });
    }

    fn load_artist(&self, id: String) {
        let api = Arc::clone(&self.api);
        tokio::spawn(async move {
            let (artist, top, albums) = tokio::join!(
                api.artist(&id),
                api.artist_top_tracks(&id),
                api.artist_albums(&id, "album,single", 0, 20),
            );
            let artist = match artist {
                Ok(a) => a,
                Err(error) => {
                    bridge::emit(Event::Error(error.to_string()));
                    return;
                }
            };
            let top = top.unwrap_or_default();
            let albums = albums.map(|p| p.items).unwrap_or_default();
            let like_uris: Vec<String> = top
                .iter()
                .filter(|t| !t.uri.is_empty())
                .map(|t| t.uri.clone())
                .collect();
            let artist_id = if artist.id.is_empty() {
                id.clone()
            } else {
                artist.id.clone()
            };
            bridge::emit(Event::Page {
                page: Page::Artist(id),
                body: PageBody::Artist {
                    artist,
                    top,
                    albums,
                },
            });
            Worker::emit_contains(api.clone(), like_uris);
            Worker::emit_library_state(api, None, Some(artist_id));
        });
    }

    fn load_queue(&self) {
        let api = Arc::clone(&self.api);
        tokio::spawn(async move {
            match api.queue().await {
                Ok(queue) => bridge::emit(Event::Page {
                    page: Page::Queue,
                    body: PageBody::Queue { items: queue.queue },
                }),
                Err(error) => bridge::emit(Event::Error(error.to_string())),
            }
        });
    }

    fn fetch_art(&self, url: String) {
        let art = self.art.clone();
        tokio::spawn(async move {
            match art.fetch(&url).await {
                Ok(bytes) => bridge::emit(Event::Art { url, bytes }),
                Err(error) => tracing::debug!("art {url}: {error}"),
            }
        });
    }

    fn set_skipped(&mut self, uri: String, skipped: bool) {
        if skipped {
            self.skipped.uris.insert(uri.clone());
        } else {
            self.skipped.uris.remove(&uri);
        }
        self.skipped.save(&self.dirs);
        if skipped && self.playing_uri(&uri) {
            if self
                .play_uris
                .iter()
                .any(|u| u == &uri || uri_ids_match(u, &uri))
            {
                self.skip_to_next_after(uri);
            } else {
                self.player(PlayerCommand::Next);
            }
        }
    }

    fn playing_uri(&self, uri: &str) -> bool {
        if !matches!(self.last_playback, Playback::Playing | Playback::Loading) {
            return false;
        }
        self.play_uris
            .get(self.play_index)
            .is_some_and(|u| u == uri || uri_ids_match(u, uri))
            || self
                .last_engine_uri
                .as_deref()
                .is_some_and(|u| u == uri || uri_ids_match(u, uri))
    }

    fn add_to_playlist(&self, playlist_id: String, name: String, uris: Vec<String>) {
        let api = Arc::clone(&self.api);
        tokio::spawn(async move {
            match api.add_playlist_items(&playlist_id, &uris, None).await {
                Ok(_) => bridge::emit(Event::AddedToPlaylist {
                    playlist_id,
                    name,
                    uris,
                }),
                Err(error) => bridge::emit(Event::Error(error.to_string())),
            }
        });
    }

    fn create_playlist(&self, name: String, uris: Vec<String>) {
        let api = Arc::clone(&self.api);
        tokio::spawn(async move {
            match api.create_playlist(&name, false, "").await {
                Ok(playlist) => {
                    let playlist_id = playlist.id.clone();
                    let playlist_name = if playlist.name.is_empty() {
                        name
                    } else {
                        playlist.name
                    };
                    if !uris.is_empty()
                        && let Err(error) = api.add_playlist_items(&playlist_id, &uris, None).await
                    {
                        bridge::emit(Event::Error(error.to_string()));
                        return;
                    }
                    let lists = api.my_playlists_all(400).await;
                    if !lists.is_empty() {
                        bridge::emit(Event::Playlists(lists));
                    }
                    bridge::emit(Event::AddedToPlaylist {
                        playlist_id,
                        name: playlist_name,
                        uris,
                    });
                }
                Err(error) => bridge::emit(Event::Error(error.to_string())),
            }
        });
    }

    fn set_favorite(&self, kind: FavoriteKind, id: String, saved: bool) {
        if id.is_empty() {
            return;
        }
        let api = Arc::clone(&self.api);
        tokio::spawn(async move {
            let result = match kind {
                FavoriteKind::Album => api.set_saved_album(&id, saved).await,
                FavoriteKind::Artist => api.set_followed_artist(&id, saved).await,
            };
            match result {
                Ok(()) => bridge::emit(Event::Favorite { kind, id, saved }),
                Err(error) => bridge::emit(Event::Error(error.to_string())),
            }
        });
    }

    fn emit_library_state(
        api: Arc<ApiClient>,
        album_id: Option<String>,
        artist_id: Option<String>,
    ) {
        tokio::spawn(async move {
            if let Some(id) = album_id {
                match api.album_is_saved(&id).await {
                    Ok(saved) => bridge::emit(Event::Favorite {
                        kind: FavoriteKind::Album,
                        id,
                        saved,
                    }),
                    Err(error) => tracing::debug!("album contains: {error}"),
                }
            }
            if let Some(id) = artist_id {
                match api.artist_is_followed(&id).await {
                    Ok(saved) => bridge::emit(Event::Favorite {
                        kind: FavoriteKind::Artist,
                        id,
                        saved,
                    }),
                    Err(error) => tracing::debug!("artist contains: {error}"),
                }
            }
        });
    }

    fn set_saved(&mut self, uri: String, saved: bool) {
        self.liked.set(uri.clone(), saved);
        self.liked.save(&self.dirs);
        let api = Arc::clone(&self.api);
        tokio::spawn(async move {
            let result = if saved {
                api.save(&[uri.clone()]).await
            } else {
                api.unsave(&[uri.clone()]).await
            };
            match result {
                Ok(()) => bridge::emit(Event::Saved { uri, saved }),
                Err(error) => bridge::emit(Event::Error(error.to_string())),
            }
        });
    }

    fn refresh_liked(&self) {
        let api = Arc::clone(&self.api);
        let dirs = self.dirs.clone();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(1200)).await;
            let mut uris = Vec::new();
            let mut offset = 0_u32;
            loop {
                if api.cooling_down().await {
                    break;
                }
                match api.saved_tracks(offset, PAGE_SIZE).await {
                    Ok(page) => {
                        if page.items.is_empty() {
                            break;
                        }
                        for saved in page.items {
                            if !saved.track.uri.is_empty() {
                                uris.push(saved.track.uri);
                            } else if let Some(id) = saved.track.id {
                                uris.push(format!("spotify:track:{id}"));
                            }
                        }
                        offset += PAGE_SIZE;
                        if offset >= page.total {
                            break;
                        }
                    }
                    Err(error) => {
                        tracing::warn!("liked library: {error}");
                        break;
                    }
                }
            }
            if uris.is_empty() {
                return;
            }
            let liked = Liked {
                uris: uris.iter().cloned().collect(),
            };
            liked.save(&dirs);
            bridge::emit(Event::Liked(uris));
        });
    }

    fn emit_contains(api: Arc<ApiClient>, uris: Vec<String>) {
        if uris.is_empty() {
            return;
        }
        tokio::spawn(async move {
            for chunk in uris.chunks(50) {
                match api.contains(chunk).await {
                    Ok(flags) => {
                        for (uri, saved) in chunk.iter().zip(flags) {
                            bridge::emit(Event::Saved {
                                uri: uri.clone(),
                                saved,
                            });
                        }
                    }
                    Err(error) => tracing::warn!("contains: {error}"),
                }
            }
        });
    }

    fn refresh_devices(&mut self) {
        self.last_devices_at = Instant::now();
        let api = Arc::clone(&self.api);
        tokio::spawn(async move {
            match api.devices().await {
                Ok(devices) => bridge::emit(Event::Devices(devices)),
                Err(ApiError::NotSignedIn | ApiError::RateLimited) => {}
                Err(error) => tracing::debug!("devices: {error}"),
            }
        });
    }

    fn refresh_playback(&self) {
        if self.engine.is_some() {
            return;
        }
        let api = Arc::clone(&self.api);
        tokio::spawn(async move {
            match api.playback_state().await {
                Ok(Some(state)) => {
                    let now = now_from_remote(&state);
                    if !now.uri.is_empty() {
                        let api = api.clone();
                        let uri = now.uri.clone();
                        tokio::spawn(async move {
                            if let Ok(flags) = api.contains(&[uri.clone()]).await
                                && let Some(saved) = flags.first().copied()
                            {
                                bridge::emit(Event::Saved { uri, saved });
                            }
                        });
                    }
                    bridge::emit(Event::NowPlaying(now));
                }
                Ok(None) => {}
                Err(ApiError::NotSignedIn) => {}
                Err(error) => tracing::debug!("playback: {error}"),
            }
        });
    }

    async fn poll(&mut self) {
        if !self.signed_in {
            return;
        }
        if self.api.cooling_down().await {
            return;
        }
        if profile_poll_due(
            self.user.is_some(),
            self.profile_inflight,
            self.profile_retry_at,
            Instant::now(),
        ) {
            self.refresh_me();
        }
        if self.engine.is_none() && !self.engine_busy {
            self.refresh_playback();
        }
        if self.last_devices_at.elapsed() >= Duration::from_secs(30) {
            self.refresh_devices();
        }
    }

    fn refresh_me(&mut self) {
        if self.profile_inflight {
            return;
        }
        self.profile_inflight = true;
        let api = Arc::clone(&self.api);
        let int_tx = self.int_tx.clone();
        tokio::spawn(async move {
            let msg = match api.me().await {
                Ok(user) => Internal::ProfileOk(user),
                Err(ApiError::RateLimited | ApiError::QuotaExhausted) => {
                    tracing::warn!("profile fetch delayed; keeping the saved session");
                    Internal::ProfileSkipped
                }
                Err(ApiError::NotSignedIn | ApiError::SignInExpired { .. }) => {
                    bridge::emit(Event::Auth(AuthStatus::Failed(
                        "Spotify sign-in expired. Sign in again.".into(),
                    )));
                    Internal::ProfileFailed
                }
                Err(error) => {
                    tracing::debug!("profile: {error}");
                    Internal::ProfileSkipped
                }
            };
            let _ = int_tx.send(msg);
        });
    }

    fn on_profile(&mut self, user: User) {
        self.profile_inflight = false;
        self.profile_retry_at = None;
        let premium = user.product.as_deref().map(|p| p == "premium");
        self.user = Some(user.clone());
        self.premium = premium;
        if premium == Some(false)
            && let Some(engine) = self.engine.take()
        {
            engine.shutdown();
            bridge::emit(Event::LocalPlayback(LocalPlayback::Failed(
                PREMIUM_NEEDED.into(),
            )));
        }
        bridge::emit(Event::User(user.clone()));
        bridge::emit(Event::Premium(premium));
        bridge::emit(Event::Auth(AuthStatus::Connected {
            username: user.name().to_string(),
        }));
    }

    fn on_profile_skipped(&mut self) {
        self.profile_inflight = false;
        self.profile_retry_at = Some(Instant::now() + Duration::from_secs(300));
        if !matches!(self.user, Some(_)) {
            bridge::emit(Event::Auth(AuthStatus::Connected {
                username: "Spotify".into(),
            }));
        }
    }

    fn arm_resume(&mut self, spec: LoadSpec, delay: Duration) {
        self.resume_verify = Some((spec, 0));
        let int_tx = self.int_tx.clone();
        tokio::spawn(async move {
            tokio::time::sleep(delay).await;
            let _ = int_tx.send(Internal::VerifyResume);
        });
    }

    fn verify_resume(&mut self) {
        let Some((spec, attempts)) = self.resume_verify.take() else {
            return;
        };
        let Some(engine) = &self.engine else {
            return;
        };
        let waiting = self
            .hold
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .want
            .is_some();
        let going = matches!(
            self.last_playback,
            Playback::Playing | Playback::Loading | Playback::Paused
        ) || engine.interrupted().is_some();
        // A click sets `want` until that URI is Playing. Stale Playing events
        // from the previous track must not count as success.
        if !waiting && going {
            return;
        }
        if attempts >= 3 {
            tracing::warn!("gave up starting playback after {attempts} tries");
            return;
        }
        tracing::info!(
            try_n = attempts + 1,
            "playback did not start; loading again"
        );
        if let Err(error) = engine.command(PlayerCommand::Load(spec.clone())) {
            tracing::warn!("unable to start playback: {error}");
        }
        self.resume_verify = Some((spec, attempts + 1));
        let int_tx = self.int_tx.clone();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_secs(4)).await;
            let _ = int_tx.send(Internal::VerifyResume);
        });
    }

    async fn media(&mut self, command: MediaCommand) {
        match command {
            MediaCommand::Play => {
                if self.engine.is_some() {
                    self.player(PlayerCommand::Play);
                } else {
                    self.play(self.now_as_play_request(), None).await;
                }
            }
            MediaCommand::Pause | MediaCommand::Stop => {
                self.transport(PlayerCommand::Pause, |api, id| async move {
                    api.pause(id.as_deref()).await
                })
                .await
            }
            MediaCommand::PlayPause => {
                if self.engine.is_some() {
                    self.player(PlayerCommand::Toggle);
                } else if self.now.playback == Playback::Playing {
                    self.transport(PlayerCommand::Pause, |api, id| async move {
                        api.pause(id.as_deref()).await
                    })
                    .await
                } else {
                    self.play(self.now_as_play_request(), None).await;
                }
            }
            MediaCommand::Next => {
                self.ignore_current_track();
                self.advance_gen = self.advance_gen.wrapping_add(1);
                self.transport(PlayerCommand::Next, |api, id| async move {
                    api.next(id.as_deref()).await
                })
                .await
            }
            MediaCommand::Previous => {
                self.ignore_current_track();
                self.advance_gen = self.advance_gen.wrapping_add(1);
                self.transport(PlayerCommand::Previous, |api, id| async move {
                    api.previous(id.as_deref()).await
                })
                .await
            }
            MediaCommand::SeekBy(delta) => {
                let pos = self.now.position_ms as i64 + delta;
                let pos = pos.max(0) as u32;
                self.seek(pos).await;
            }
            MediaCommand::SetPosition {
                track_uri,
                position_ms,
            } => {
                if self.now.uri == track_uri || self.now.uri.is_empty() {
                    self.seek(position_ms).await;
                }
            }
            MediaCommand::SetVolume(volume) => {
                let percent = (volume * 100.0).clamp(0.0, 100.0) as u8;
                let librespot = ((percent as u32) * 65535 / 100) as u16;
                if self.engine.is_some() {
                    self.player(PlayerCommand::Volume(librespot));
                } else {
                    let api = Arc::clone(&self.api);
                    let device = self.now.device_id.clone();
                    tokio::spawn(async move {
                        let _ = api.set_volume(percent, device.as_deref()).await;
                    });
                }
            }
            MediaCommand::SetShuffle(shuffle) => {
                if self.engine.is_some() {
                    self.player(PlayerCommand::Shuffle(shuffle));
                } else {
                    let api = Arc::clone(&self.api);
                    let device = self.now.device_id.clone();
                    tokio::spawn(async move {
                        let _ = api.set_shuffle(shuffle, device.as_deref()).await;
                    });
                }
            }
            MediaCommand::SetRepeat(mode) => {
                if self.engine.is_some() {
                    self.player(PlayerCommand::Repeat(mode));
                } else {
                    let api = Arc::clone(&self.api);
                    let device = self.now.device_id.clone();
                    tokio::spawn(async move {
                        let _ = api.set_repeat(mode.api_name(), device.as_deref()).await;
                    });
                }
            }
            MediaCommand::OpenUri(uri) => {
                let request = if uri.contains(":track:") || uri.contains(":episode:") {
                    PlayRequest::tracks(vec![uri])
                } else {
                    PlayRequest::context(uri)
                };
                self.play(request, None).await;
            }
            MediaCommand::Raise => bridge::emit(Event::Raise),
            MediaCommand::Quit => bridge::emit(Event::Quit),
        }
    }

    fn hold_play(&mut self, request: &PlayRequest) {
        let want = request
            .offset_uri
            .clone()
            .or_else(|| request.uris.first().cloned());
        let ignore = self.last_engine_uri.clone().filter(|prev| {
            !want
                .as_ref()
                .is_some_and(|w| w == prev || uri_ids_match(prev, w))
        });
        let mut hold = self.hold.lock().unwrap_or_else(|p| p.into_inner());
        hold.want = want;
        hold.ignore = ignore;
        self.advance_gen = self.advance_gen.wrapping_add(1);
    }

    fn ignore_current_track(&mut self) {
        let mut hold = self.hold.lock().unwrap_or_else(|p| p.into_inner());
        hold.want = None;
        hold.ignore = self.last_engine_uri.clone();
    }

    fn remember_queue(&mut self, request: &PlayRequest) {
        if request.uris.len() >= 2 {
            self.play_uris = request.uris.clone();
            self.play_index = request
                .offset_uri
                .as_ref()
                .and_then(|uri| request.uris.iter().position(|u| u == uri))
                .or_else(|| request.offset_position.map(|i| i as usize))
                .unwrap_or(0);
        } else {
            self.play_uris.clear();
            self.play_index = 0;
        }
    }

    fn on_engine_state(&mut self, state: crate::player::LocalState) {
        let was_going = matches!(self.last_playback, Playback::Playing | Playback::Loading);
        let now_stopped = state.playback == Playback::Stopped;
        let uri = state.track.as_ref().map(|t| t.uri.clone());
        if let Some(uri) = uri.as_ref() {
            self.last_engine_uri = Some(uri.clone());
        }
        self.last_playback = state.playback;
        self.now = now_playing_from_local(&state);
        if was_going && now_stopped {
            if let Some(uri) = uri {
                let generation = self.advance_gen.wrapping_add(1);
                self.advance_gen = generation;
                let int_tx = self.int_tx.clone();
                tokio::spawn(async move {
                    tokio::time::sleep(Duration::from_millis(400)).await;
                    let _ = int_tx.send(Internal::MaybeAdvance { uri, generation });
                });
            }
        } else if matches!(state.playback, Playback::Playing | Playback::Loading) {
            self.advance_gen = self.advance_gen.wrapping_add(1);
            if let Some(uri) = uri.clone()
                && let Some(idx) = self.play_uris.iter().position(|u| u == &uri)
            {
                self.play_index = idx;
            }
            if let Some(uri) = uri
                && self.skipped.contains(&uri)
            {
                let want = self
                    .hold
                    .lock()
                    .unwrap_or_else(|p| p.into_inner())
                    .want
                    .clone();
                if !want
                    .as_ref()
                    .is_some_and(|w| w == &uri || uri_ids_match(&uri, w))
                {
                    self.skip_to_next_after(uri);
                }
            }
        }
    }

    fn maybe_advance(&mut self, uri: String, generation: u64) {
        if generation != self.advance_gen || self.last_playback != Playback::Stopped {
            return;
        }
        self.skip_to_next_after(uri);
    }

    fn skip_to_next_after(&mut self, uri: String) {
        let Some(idx) = self
            .play_uris
            .iter()
            .position(|u| u == &uri || uri_ids_match(u, &uri))
        else {
            return;
        };
        let mut next = idx + 1;
        while next < self.play_uris.len() && self.skipped.contains(&self.play_uris[next]) {
            next += 1;
        }
        if next >= self.play_uris.len() {
            return;
        }
        self.play_index = next;
        let rest: Vec<String> = self.play_uris[next..]
            .iter()
            .filter(|u| !self.skipped.contains(u))
            .cloned()
            .collect();
        if rest.is_empty() {
            return;
        }
        let request = PlayRequest {
            uris: rest,
            ..PlayRequest::default()
        };
        self.hold_play(&request);
        if let Some(engine) = &self.engine {
            let spec = load_spec(&request);
            let _ = engine.command(PlayerCommand::Load(spec.clone()));
            self.arm_resume(spec, Duration::from_millis(2_000));
        }
    }

    fn now_as_play_request(&self) -> PlayRequest {
        if self.now.uri.is_empty() {
            PlayRequest::default()
        } else {
            PlayRequest::tracks(vec![self.now.uri.clone()])
        }
    }

    async fn transport<F, Fut>(&self, local: PlayerCommand, remote: F)
    where
        F: FnOnce(Arc<ApiClient>, Option<String>) -> Fut,
        Fut: std::future::Future<Output = Result<(), ApiError>>,
    {
        if self.engine.is_some() {
            self.player(local);
            return;
        }
        let api = Arc::clone(&self.api);
        let device = self.now.device_id.clone();
        if let Err(error) = remote(api, device).await {
            bridge::emit(Event::Error(friendly_player_error(&error)));
        }
    }

    async fn seek(&self, position_ms: u32) {
        if self.engine.is_some() {
            self.player(PlayerCommand::Seek(position_ms));
            return;
        }
        let api = Arc::clone(&self.api);
        let device = self.now.device_id.clone();
        tokio::spawn(async move {
            let _ = api.seek(position_ms, device.as_deref()).await;
        });
    }

    fn media_state(&self) -> MediaState {
        MediaState {
            playback: self.now.playback,
            track: if self.now.uri.is_empty() {
                None
            } else {
                Some(MediaTrack {
                    uri: self.now.uri.clone(),
                    title: self.now.title.clone(),
                    artists: self
                        .now
                        .artists
                        .split(", ")
                        .filter(|s| !s.is_empty())
                        .map(str::to_string)
                        .collect(),
                    album: self.now.album.clone(),
                    art_url: self.now.art_url.clone(),
                    duration_ms: self.now.duration_ms,
                })
            },
            position_ms: self.now.position_ms,
            volume: self.now.volume_percent as f64 / 100.0,
            shuffle: self.now.shuffle,
            repeat: self.now.repeat,
            can_control: self.signed_in,
        }
    }
}

#[derive(Clone, Debug, Default)]
struct EngineHold {
    /// URI we just asked to play. Drop other URIs until this is Playing.
    want: Option<String>,
    /// URI we left. Drop even after `want` is playing (stale engine events).
    ignore: Option<String>,
}

fn hold_allows(
    hold: &std::sync::Arc<std::sync::Mutex<EngineHold>>,
    state: &crate::player::LocalState,
) -> bool {
    let mut hold = hold.lock().unwrap_or_else(|p| p.into_inner());
    let Some(uri) = state.track.as_ref().map(|t| t.uri.as_str()) else {
        return hold.want.is_none();
    };
    if let Some(ignore) = hold.ignore.as_deref()
        && (uri == ignore || uri_ids_match(uri, ignore))
    {
        return false;
    }
    if let Some(want) = hold.want.as_deref() {
        if uri == want || uri_ids_match(uri, want) {
            if state.playback == Playback::Playing {
                hold.want = None;
            }
            return true;
        }
        return false;
    }
    true
}

fn uri_ids_match(a: &str, b: &str) -> bool {
    a.rsplit(':').next() == b.rsplit(':').next()
}

fn load_spec(request: &PlayRequest) -> LoadSpec {
    LoadSpec {
        context_uri: request.context_uri.clone(),
        uris: request.uris.clone(),
        offset_uri: request.offset_uri.clone(),
        offset_index: request.offset_position,
        position_ms: request.position_ms,
        play: true,
        shuffle: None,
        autoplay: false,
    }
}

fn friendly_player_error(error: &ApiError) -> String {
    let raw = error.to_string();
    let lower = raw.to_ascii_lowercase();
    if lower.contains("no active device") || lower.contains("no_active_device") {
        "Nothing is playing on a speaker. Choose This computer in Devices — you'll approve playback once in the browser.".into()
    } else if lower.contains("restriction") {
        "That speaker can't take this play command. Choose This computer in Devices.".into()
    } else {
        raw
    }
}

fn profile_poll_due(
    has_user: bool,
    inflight: bool,
    retry_at: Option<Instant>,
    now: Instant,
) -> bool {
    !has_user && !inflight && retry_at.is_none_or(|at| now >= at)
}

fn now_playing_from_local(state: &LocalState) -> NowPlaying {
    let volume_percent = ((state.volume as u32) * 100 / 65535) as u8;
    let (title, artists, album, artist_links, album_id, uri, art_url, duration_ms) =
        match &state.track {
            Some(track) => (
                track.title.clone(),
                track.artist_names(),
                track.album.clone(),
                track.artist_links.clone(),
                track.album_id.clone(),
                track.uri.clone(),
                track.art_url.clone().or(track.art_small_url.clone()),
                track.duration_ms,
            ),
            None => (
                String::new(),
                String::new(),
                String::new(),
                Vec::new(),
                None,
                String::new(),
                None,
                0,
            ),
        };
    NowPlaying {
        playback: state.playback,
        title,
        artists,
        album,
        artist_links,
        album_id,
        uri,
        art_url,
        duration_ms,
        position_ms: state.position_now(),
        volume_percent,
        shuffle: state.shuffle,
        repeat: state.repeat,
        device_name: "This computer".into(),
        device_id: None,
        is_local: true,
        saved: None,
    }
}

fn apply_local_state(state: LocalState) {
    bridge::emit(Event::NowPlaying(now_playing_from_local(&state)));
    if let Some(error) = state.error {
        bridge::emit(Event::Error(error));
    }
}

fn now_from_remote(state: &PlaybackState) -> NowPlaying {
    let item = state.item.as_ref();
    let (album, artist_links, album_id) = match item {
        Some(PlayableItem::Track(track)) => (
            track
                .album
                .as_ref()
                .map(|a| a.name.clone())
                .unwrap_or_default(),
            track.artist_links(),
            track.album_catalog_id().map(str::to_string),
        ),
        Some(PlayableItem::Episode(ep)) => (
            ep.show.as_ref().map(|s| s.name.clone()).unwrap_or_default(),
            Vec::new(),
            None,
        ),
        None => (String::new(), Vec::new(), None),
    };
    NowPlaying {
        playback: if state.is_playing {
            Playback::Playing
        } else if item.is_some() {
            Playback::Paused
        } else {
            Playback::Stopped
        },
        title: item.map(|i| i.name().to_string()).unwrap_or_default(),
        artists: item.map(|i| i.subtitle()).unwrap_or_default(),
        album,
        artist_links,
        album_id,
        uri: item.map(|i| i.uri().to_string()).unwrap_or_default(),
        art_url: item.and_then(|i| i.image(300)).map(str::to_string),
        duration_ms: item.map(|i| i.duration_ms()).unwrap_or(0),
        position_ms: state.progress_ms.unwrap_or(0),
        volume_percent: state
            .device
            .as_ref()
            .and_then(|d| d.volume_percent)
            .unwrap_or(50),
        shuffle: state.shuffle_state,
        repeat: RepeatMode::from_api(&state.repeat_state),
        device_name: state
            .device
            .as_ref()
            .map(|d| d.name.clone())
            .unwrap_or_default(),
        device_id: state.device.as_ref().and_then(|d| d.id.clone()),
        is_local: false,
        saved: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::player::{LocalState, LocalTrack};

    fn state(uri: &str, playback: Playback) -> LocalState {
        LocalState {
            playback,
            track: Some(LocalTrack {
                uri: uri.into(),
                title: "t".into(),
                ..LocalTrack::default()
            }),
            ..LocalState::default()
        }
    }

    fn hold(
        want: Option<&str>,
        ignore: Option<&str>,
    ) -> std::sync::Arc<std::sync::Mutex<EngineHold>> {
        std::sync::Arc::new(std::sync::Mutex::new(EngineHold {
            want: want.map(str::to_string),
            ignore: ignore.map(str::to_string),
        }))
    }

    #[test]
    fn hold_allows_context_advance_after_wanted_track_plays() {
        let h = hold(Some("spotify:track:a"), None);
        assert!(hold_allows(
            &h,
            &state("spotify:track:a", Playback::Loading)
        ));
        assert!(hold_allows(
            &h,
            &state("spotify:track:a", Playback::Playing)
        ));
        assert!(h.lock().unwrap().want.is_none());
        assert!(hold_allows(
            &h,
            &state("spotify:track:b", Playback::Playing)
        ));
    }

    #[test]
    fn hold_drops_previous_track_while_switching() {
        let h = hold(Some("spotify:track:b"), Some("spotify:track:a"));
        assert!(!hold_allows(
            &h,
            &state("spotify:track:a", Playback::Playing)
        ));
        assert!(!hold_allows(
            &h,
            &state("spotify:track:a", Playback::Stopped)
        ));
        assert!(hold_allows(
            &h,
            &state("spotify:track:b", Playback::Playing)
        ));
        assert!(!hold_allows(
            &h,
            &state("spotify:track:a", Playback::Playing)
        ));
        assert!(hold_allows(
            &h,
            &state("spotify:track:c", Playback::Playing)
        ));
    }

    #[test]
    fn hold_without_want_still_ignores_left_track() {
        let h = hold(None, Some("spotify:track:a"));
        assert!(!hold_allows(
            &h,
            &state("spotify:track:a", Playback::Stopped)
        ));
        assert!(hold_allows(
            &h,
            &state("spotify:track:b", Playback::Playing)
        ));
    }

    #[test]
    fn profile_poll_stops_once_we_have_a_user_or_are_backing_off() {
        let now = Instant::now();
        assert!(profile_poll_due(false, false, None, now));
        assert!(!profile_poll_due(true, false, None, now));
        assert!(!profile_poll_due(false, true, None, now));
        assert!(!profile_poll_due(
            false,
            false,
            Some(now + Duration::from_secs(60)),
            now
        ));
        assert!(profile_poll_due(
            false,
            false,
            Some(now - Duration::from_secs(1)),
            now
        ));
    }

    #[test]
    fn now_playing_from_local_copies_the_engine_track() {
        let now = now_playing_from_local(&state("spotify:track:a", Playback::Playing));
        assert_eq!(now.uri, "spotify:track:a");
        assert_eq!(now.playback, Playback::Playing);
        assert!(now.is_local);
    }

    #[test]
    fn album_page_cache_without_album_field_still_decodes() {
        let json = r#"{"Tracks":{"title":"P","subtitle":"s","items":[],"total":0,"offset":0}}"#;
        let body: PageBody = serde_json::from_str(json).unwrap();
        match body {
            PageBody::Tracks { album, title, .. } => {
                assert!(album.is_none());
                assert_eq!(title, "P");
            }
            other => panic!("unexpected {other:?}"),
        }
    }
}
