//! Tokio worker: PKCE, Web API, librespot engine, artwork, MPRIS.

use std::sync::Arc;
use std::time::{Duration, Instant};

use librespot_core::authentication::Credentials;
use serde::{Deserialize, Serialize};
use tokio::sync::{mpsc, watch};

use crate::cache::{self, Skipped};

use crate::api::models::*;
use crate::api::{ApiClient, ApiError, ApiSource, NetActivity, PlayRequest, TokenProvider, WebTokens};
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
    Tracks {
        title: String,
        subtitle: String,
        art: Option<String>,
        context_uri: Option<String>,
        items: Vec<Track>,
        total: u32,
        offset: u32,
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
    SetSkipped {
        uri: String,
        skipped: bool,
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
    MaybeAdvance { uri: String, generation: u64 },
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
    let api = Arc::new(ApiClient::new(http.clone(), activity, 20, 50, ApiSource::Shared));
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

    let mut mpris = MediaService::spawn(|| {});

    let mut worker = Worker {
        skipped: Skipped::load(&dirs),
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
        cancel_signin: None,
        reconnects: Vec::new(),
        resume: None,
        pending_play: None,
        play_uris: Vec::new(),
        play_index: 0,
        last_playback: Playback::Stopped,
        advance_gen: 0,
        hold_uri: std::sync::Arc::new(std::sync::Mutex::new(None)),
        now: NowPlaying::default(),
        devices: Vec::new(),
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
            _ = tick.tick() => {
                worker.poll().await;
                for media in mpris.drain_commands() {
                    worker.handle(Cmd::Media(media)).await;
                }
                mpris.update(worker.media_state());
            }
        }
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
    cancel_signin: Option<watch::Sender<bool>>,
    reconnects: Vec<Instant>,
    resume: Option<LoadSpec>,
    /// Play the user asked for before local playback was up (no Connect device).
    pending_play: Option<PlayRequest>,
    play_uris: Vec<String>,
    play_index: usize,
    last_playback: Playback,
    advance_gen: u64,
    skipped: Skipped,
    /// Drop engine snapshots for any other track while a click/skip settles.
    hold_uri: std::sync::Arc<std::sync::Mutex<Option<String>>>,
    now: NowPlaying,
    devices: Vec<Device>,
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
        self.api.set_token_provider(Some(TokenProvider::Web(tokens)));
        let api = Arc::clone(&self.api);
        tokio::spawn(async move {
            match api.me().await {
                Ok(user) => {
                    let premium = user.product.as_deref().map(|p| p == "premium");
                    bridge::emit(Event::User(user.clone()));
                    bridge::emit(Event::Premium(premium));
                    bridge::emit(Event::Auth(AuthStatus::Connected {
                        username: user.name().to_string(),
                    }));
                }
                Err(error) => {
                    bridge::emit(Event::Auth(AuthStatus::Failed(error.to_string())));
                }
            }
        });
        self.signed_in = true;
        self.resume_engine();
        self.open(Page::Home);
        let last = Page::decode(&self.settings.last_page).unwrap_or(Page::Home);
        if last != Page::Home {
            self.open(last);
        }
        self.refresh_devices();
        self.refresh_playback();
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
            Cmd::Play {
                request,
                device_id,
            } => self.play(request, device_id).await,
            Cmd::Transfer { device_id, play } => self.transfer(device_id, play).await,
            Cmd::Open(page) => self.open(page),
            Cmd::Search(query) => {
                self.search_query = query;
                self.open(Page::Search);
            }
            Cmd::LoadMore => self.load_more(),
            Cmd::FetchArt(url) => self.fetch_art(url),
            Cmd::SetSaved { uri, saved } => self.set_saved(uri, saved),
            Cmd::SetSkipped { uri, skipped } => self.set_skipped(uri, skipped),
            Cmd::RefreshPlayback => self.refresh_playback(),
            Cmd::RefreshDevices => self.refresh_devices(),
            Cmd::SaveSettings(settings) => {
                settings.save(&self.dirs);
                self.settings = settings.clone();
                bridge::emit(Event::Settings(settings));
                if self.engine.is_some() {
                    self.reconnect_engine();
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
                        let pending = self.pending_play.take();
                        if let Some(request) = pending {
                            self.hold_play(&request);
                            self.remember_queue(&request);
                            let _ = engine.command(PlayerCommand::Load(load_spec(&request)));
                        } else if let Some(spec) = self.resume.take() {
                            let _ = engine.command(PlayerCommand::Load(spec));
                        } else {
                            let _ = engine.command(PlayerCommand::Activate);
                        }
                        self.engine = Some(engine);
                        bridge::emit(Event::LocalPlayback(LocalPlayback::Ready { device_id }));
                    }
                    (None, Some(error)) => {
                        bridge::emit(Event::LocalPlayback(LocalPlayback::Failed(error)));
                    }
                    _ => {
                        bridge::emit(Event::LocalPlayback(LocalPlayback::Unavailable));
                    }
                }
            }
            Internal::Reconnect => self.reconnect_engine(),
            Internal::EngineState(state) => self.on_engine_state(state),
            Internal::MaybeAdvance { uri, generation } => self.maybe_advance(uri, generation),
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

    fn reconnect_engine(&mut self) {
        if !self.signed_in {
            return;
        }
        if let Some(engine) = self.engine.take() {
            self.resume = engine.interrupted().map(|interrupted| LoadSpec {
                uris: vec![interrupted.uri],
                position_ms: interrupted.position_ms,
                play: interrupted.playing,
                ..LoadSpec::default()
            });
            engine.shutdown();
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
            let hold_uri = Arc::clone(&self.hold_uri);
            Arc::new(move |event| match event {
                EngineEvent::State(state) => {
                    if !hold_allows(&hold_uri, &state) {
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
            if let Err(error) = engine.command(PlayerCommand::Load(load_spec(&request))) {
                bridge::emit(Event::Error(format!("Playback error: {error}")));
            }
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
            let recent = match api.recently_played(20, None, None).await {
                Ok(page) => page.items.into_iter().map(|h| h.track).collect(),
                Err(error) => {
                    bridge::emit(Event::Error(error.to_string()));
                    Vec::new()
                }
            };
            let playlists = match api.my_playlists(0, 30).await {
                Ok(page) => page.items,
                Err(_) => Vec::new(),
            };
            let top_artists = match api.top_artists("medium_term", 12).await {
                Ok(page) => page.items,
                Err(_) => Vec::new(),
            };
            let top_tracks = match api.top_tracks("medium_term", 10, 0).await {
                Ok(page) => page.items,
                Err(_) => Vec::new(),
            };
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
                    playlists,
                    top_artists,
                    top_tracks,
                },
            });
            Worker::emit_contains(api, like_uris);
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
                Ok(results) => bridge::emit(Event::Page {
                    page: Page::Search,
                    body: PageBody::Search(results),
                }),
                Err(error) => bridge::emit(Event::Error(error.to_string())),
            }
        });
    }

    fn load_liked(&mut self, offset: u32) {
        let api = Arc::clone(&self.api);
        tokio::spawn(async move {
            match api.saved_tracks(offset, PAGE_SIZE).await {
                Ok(page) => {
                    let items: Vec<Track> = page.items.into_iter().map(|s| s.track).collect();
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
                            subtitle: format!("{} songs", page.total),
                            art: None,
                            context_uri: Some("spotify:collection:tracks".into()),
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
            let playlist = match api.playlist(&id).await {
                Ok(p) => p,
                Err(error) => {
                    bridge::emit(Event::Error(error.to_string()));
                    return;
                }
            };
            match api.playlist_items(&id, offset, PAGE_SIZE).await {
                Ok(page) => {
                    let items: Vec<Track> = page
                        .items
                        .iter()
                        .filter_map(|item| match item.playable() {
                            Some(PlayableItem::Track(track)) => Some(track.clone()),
                            _ => None,
                        })
                        .collect();
                    let art = crate::api::models::pick_image(&playlist.images, 300).map(str::to_string);
                    let title = playlist.name.clone();
                    let subtitle = format!(
                        "{} · {} songs",
                        playlist.owner_name(),
                        playlist.track_total()
                    );
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
                    let art = crate::api::models::pick_image(&album.images, 300).map(str::to_string);
                    let year = album.year().unwrap_or("").to_string();
                    let artists = crate::api::models::join_names(
                        album.artists.iter().map(|a| a.name.as_str()),
                    );
                    let like_uris: Vec<String> = items
                        .iter()
                        .filter(|t| !t.uri.is_empty())
                        .map(|t| t.uri.clone())
                        .collect();
                    bridge::emit(Event::Page {
                        page: Page::Album(id),
                        body: PageBody::Tracks {
                            title: album.name,
                            subtitle: format!("{artists} · {year}"),
                            art,
                            context_uri: Some(album.uri),
                            total: items.len() as u32,
                            offset: 0,
                            items,
                        },
                    });
                    Worker::emit_contains(api, like_uris);
                }
                Err(error) => bridge::emit(Event::Error(error.to_string())),
            }
        });
    }

    fn load_artist(&self, id: String) {
        let api = Arc::clone(&self.api);
        tokio::spawn(async move {
            let artist = match api.artist(&id).await {
                Ok(a) => a,
                Err(error) => {
                    bridge::emit(Event::Error(error.to_string()));
                    return;
                }
            };
            let top = api.artist_top_tracks(&id).await.unwrap_or_default();
            let albums = api
                .artist_albums(&id, "album,single", 0, 20)
                .await
                .map(|p| p.items)
                .unwrap_or_default();
            bridge::emit(Event::Page {
                page: Page::Artist(id),
                body: PageBody::Artist {
                    artist,
                    top,
                    albums,
                },
            });
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
        if skipped
            && matches!(self.last_playback, Playback::Playing | Playback::Loading)
            && self.play_uris.get(self.play_index) == Some(&uri)
        {
            self.skip_to_next_after(uri);
        }
    }

    fn set_saved(&self, uri: String, saved: bool) {
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
                    Err(error) => tracing::debug!("contains: {error}"),
                }
            }
        });
    }

    fn refresh_devices(&self) {
        let api = Arc::clone(&self.api);
        tokio::spawn(async move {
            match api.devices().await {
                Ok(devices) => bridge::emit(Event::Devices(devices)),
                Err(ApiError::NotSignedIn) => {}
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
        self.refresh_playback();
        self.refresh_devices();
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
                *self.hold_uri.lock().unwrap_or_else(|p| p.into_inner()) = None;
                self.advance_gen = self.advance_gen.wrapping_add(1);
                self.transport(PlayerCommand::Next, |api, id| async move {
                    api.next(id.as_deref()).await
                })
                .await
            }
            MediaCommand::Previous => {
                *self.hold_uri.lock().unwrap_or_else(|p| p.into_inner()) = None;
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
        let uri = request
            .offset_uri
            .clone()
            .or_else(|| request.uris.first().cloned());
        *self.hold_uri.lock().unwrap_or_else(|p| p.into_inner()) = uri;
        self.advance_gen = self.advance_gen.wrapping_add(1);
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
        let was_going = matches!(
            self.last_playback,
            Playback::Playing | Playback::Loading
        );
        let now_stopped = state.playback == Playback::Stopped;
        let uri = state.track.as_ref().map(|t| t.uri.clone());
        self.last_playback = state.playback;
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
                let hold = self
                    .hold_uri
                    .lock()
                    .unwrap_or_else(|p| p.into_inner())
                    .clone();
                if hold.as_deref() != Some(uri.as_str()) {
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
        let Some(idx) = self.play_uris.iter().position(|u| u == &uri) else {
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
            let _ = engine.command(PlayerCommand::Load(load_spec(&request)));
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

fn hold_allows(
    hold_uri: &std::sync::Arc<std::sync::Mutex<Option<String>>>,
    state: &crate::player::LocalState,
) -> bool {
    let hold = hold_uri
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .clone();
    let Some(hold) = hold else {
        return true;
    };
    match state.track.as_ref().map(|t| t.uri.as_str()) {
        Some(uri) if uri == hold || uri_ids_match(uri, &hold) => true,
        Some(_) => false,
        None => false,
    }
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

fn apply_local_state(state: LocalState) {
    let volume_percent = ((state.volume as u32) * 100 / 65535) as u8;
    let (title, artists, album, uri, art_url, duration_ms) = match &state.track {
        Some(track) => (
            track.title.clone(),
            track.artist_names(),
            track.album.clone(),
            track.uri.clone(),
            track.art_url.clone().or(track.art_small_url.clone()),
            track.duration_ms,
        ),
        None => (
            String::new(),
            String::new(),
            String::new(),
            String::new(),
            None,
            0,
        ),
    };
    bridge::emit(Event::NowPlaying(NowPlaying {
        playback: state.playback,
        title,
        artists,
        album,
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
    }));
    if let Some(error) = state.error {
        bridge::emit(Event::Error(error));
    }
}

fn now_from_remote(state: &PlaybackState) -> NowPlaying {
    let item = state.item.as_ref();
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
        album: match item {
            Some(PlayableItem::Track(track)) => track
                .album
                .as_ref()
                .map(|a| a.name.clone())
                .unwrap_or_default(),
            Some(PlayableItem::Episode(ep)) => ep
                .show
                .as_ref()
                .map(|s| s.name.clone())
                .unwrap_or_default(),
            None => String::new(),
        },
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
