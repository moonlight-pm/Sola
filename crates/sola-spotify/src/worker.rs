//! Tokio worker: PKCE, Web API, librespot engine, artwork, MPRIS.
//!
//! Web API traffic is budgeted: the shared app's quota is split among every
//! user of its client id, so pages are reused while fresh, catalogue reads
//! go through the playback session when it is up (`session_reads`), liked
//! state comes from the synced `liked.json`, and playback on another device
//! is polled slowly unless a command or a Connect push says it changed.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::{Duration, Instant};

use iced::futures::StreamExt;
use librespot_core::Session;
use librespot_core::authentication::Credentials;
use serde::{Deserialize, Serialize};
use tokio::sync::{mpsc, watch};

use crate::cache::{Liked, Skipped};

use crate::api::models::*;
use crate::api::{ApiClient, ApiError, PlayRequest, TokenProvider, WebTokens, background};
use crate::auth::{self, Grant, StoredToken};
use crate::bridge;
use crate::images::ArtLoader;
use crate::media::{MediaCommand, MediaState, MediaTrack};
use crate::mpris::MediaService;
use crate::paths::AppDirs;
use crate::player::{
    Engine, EngineConfig, EngineEvent, LoadSpec, LocalState, Playback, PlayerCommand, RepeatMode,
};
use crate::session_reads::{self, Failure};
use crate::settings::Settings;

const PREMIUM_NEEDED: &str = "Playing on this computer needs Spotify Premium.";
const PAGE_SIZE: u32 = 50;
/// Playlist rows per page when the playback session reads them.
const SESSION_PAGE_SIZE: u32 = 100;
/// Playback on another device is polled this often without a session...
const REMOTE_POLL: Duration = Duration::from_secs(15);
/// ...and this often with one, whose Connect pushes say when to look sooner.
const REMOTE_POLL_PUSHED: Duration = Duration::from_secs(60);
/// After a command to another device, poll quickly for this long so the
/// result is confirmed.
const REMOTE_POLL_ACTIVE: Duration = Duration::from_secs(4);
const REMOTE_COMMAND_WINDOW: Duration = Duration::from_secs(10);
/// A Connect push is followed by a poll no sooner than this after the last.
const REMOTE_PUSH_GAP: Duration = Duration::from_secs(2);
/// Liked Songs are re-synced this often (usually one request).
const LIKED_REFRESH: Duration = Duration::from_secs(15 * 60);
/// Library playlists (sidebar, Home, Made for you) stay fresh this long...
const LIBRARY_FRESH: Duration = Duration::from_secs(10 * 60);
/// ...or this long when part of the read failed.
const LIBRARY_RETRY: Duration = Duration::from_secs(60);
/// Playlists read from the Web API when the playback session cannot answer.
const LIBRARY_MAX: u32 = 200;
/// A read that prefers the playback session waits this long for it to
/// finish connecting before asking the Web API.
const SESSION_WAIT: Duration = Duration::from_secs(6);
/// Cached page files untouched this long are pruned at startup.
const PAGE_CACHE_LIMIT: u64 = 64 * 1024 * 1024;
const PAGE_CACHE_TARGET: u64 = 48 * 1024 * 1024;

#[derive(Clone, Debug, PartialEq)]
pub enum AuthStatus {
    Starting,
    SignedOut,
    WaitingForBrowser { url: String },
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

    /// How long a fetched page is shown again without asking Spotify.
    /// Back/Forward and revisits inside this window cost no request.
    fn fresh_for(&self) -> Duration {
        const MINUTE: u64 = 60;
        Duration::from_secs(match self {
            Page::Home | Page::MadeForYou | Page::Albums | Page::Artists => 10 * MINUTE,
            Page::Liked | Page::Playlist(_) | Page::Search => 5 * MINUTE,
            Page::Artist(_) => 60 * MINUTE,
            Page::Album(_) => 6 * 60 * MINUTE,
            Page::Queue | Page::Settings => 0,
        })
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
        /// Where this chunk of rows starts.
        offset: u32,
        /// Where the next chunk starts, while rows remain. Rows Spotify
        /// returns empty are dropped, so this is not `offset + items.len()`.
        #[serde(default)]
        next: Option<u32>,
        /// Set when this collection is an album page (Save / Follow chrome).
        #[serde(default)]
        album: Option<Album>,
    },
    Albums {
        items: Vec<Album>,
        total: u32,
        offset: u32,
        #[serde(default)]
        next: Option<u32>,
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

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
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
    Play {
        request: PlayRequest,
        device_id: Option<String>,
    },
    Transfer {
        device_id: String,
        play: bool,
    },
    Open(Page),
    /// Open `page` asking Spotify again even while it is fresh.
    Refresh(Page),
    Search(String),
    /// The next page of `page`, starting at `offset` (the rows already shown).
    LoadMore {
        page: Page,
        offset: u32,
    },
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
    RefreshDevices,
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
    /// The cover could not be loaded; the UI may ask again later.
    ArtFailed(String),
    Saved {
        uri: String,
        saved: bool,
    },
    /// Liked state for a batch of tracks, as one update.
    SavedMany(Vec<(String, bool)>),
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
    /// Only the latest arm's generation counts.
    VerifyResume(u64),
    /// A page loaded; reuse it while fresh.
    PageFetched(Page),
    /// The playback poll answered: what plays on another device, if anything.
    RemoteState(Option<NowPlaying>),
    /// The playback poll failed; poll again on schedule.
    RemoteFailed,
    /// Spotify Connect pushed a cluster change: something played, paused, or
    /// moved on some device.
    ClusterChanged,
    /// Liked Songs as Spotify has them now.
    LikedSynced(HashSet<String>),
    LikedSyncEnded,
}

/// The playback session, shared with spawned reads. Reads that prefer it
/// wait briefly while the engine is still connecting.
#[derive(Clone)]
struct SessionSlot {
    session: Arc<watch::Sender<Option<Session>>>,
    connecting: Arc<AtomicBool>,
    /// The session's Connect cluster pushes are subscribed and flowing.
    pushes: Arc<AtomicBool>,
}

impl SessionSlot {
    fn new() -> Self {
        Self {
            session: Arc::new(watch::channel(None).0),
            connecting: Arc::new(AtomicBool::new(false)),
            pushes: Arc::new(AtomicBool::new(false)),
        }
    }

    fn set(&self, session: Option<Session>) {
        if session.is_none() {
            self.pushes.store(false, Ordering::SeqCst);
        }
        self.session.send_replace(session);
    }

    fn set_connecting(&self, connecting: bool) {
        self.connecting.store(connecting, Ordering::SeqCst);
        if !connecting {
            // Wake waiters so a failed connect does not hold them.
            self.session.send_modify(|_| {});
        }
    }

    fn pushes(&self) -> bool {
        self.pushes.load(Ordering::SeqCst)
    }

    async fn wait(&self) -> Option<Session> {
        let mut changes = self.session.subscribe();
        let ready = async {
            loop {
                if let Some(session) = changes.borrow_and_update().clone() {
                    return Some(session);
                }
                if !self.connecting.load(Ordering::SeqCst) {
                    return None;
                }
                if changes.changed().await.is_err() {
                    return None;
                }
            }
        };
        tokio::time::timeout(SESSION_WAIT, ready)
            .await
            .ok()
            .flatten()
    }
}

/// Library playlists, read at most once at a time and reused while fresh,
/// so Home, Made for you, and the sidebar share one fetch.
#[derive(Default)]
struct Library {
    lists: tokio::sync::Mutex<Option<HeldLibrary>>,
    /// Spotify's Made For You category refused this app (403/404); stop asking.
    no_made_for_you: AtomicBool,
}

struct HeldLibrary {
    at: Instant,
    lists: Vec<Playlist>,
    /// Every part of the read answered. A partial list is held only until a
    /// retry, and never replaces a complete one.
    complete: bool,
}

impl HeldLibrary {
    fn fresh(&self) -> bool {
        let keep = if self.complete {
            LIBRARY_FRESH
        } else {
            LIBRARY_RETRY
        };
        self.at.elapsed() < keep
    }
}

impl Library {
    async fn playlists(
        &self,
        api: &ApiClient,
        session: &SessionSlot,
        force: bool,
    ) -> Vec<Playlist> {
        let mut held = self.lists.lock().await;
        if !force
            && let Some(library) = held.as_ref()
            && library.fresh()
        {
            return library.lists.clone();
        }
        let (lists, complete) = self.fetch(api, session).await;
        let keep_old = held
            .as_ref()
            .is_some_and(|old| lists.is_empty() || (!complete && old.complete));
        // A failed or partial read (rate limit, no session) must not replace
        // a good list; a stale one is read again on the next visit.
        if keep_old && let Some(old) = held.as_ref() {
            return old.lists.clone();
        }
        if !lists.is_empty() {
            *held = Some(HeldLibrary {
                at: Instant::now(),
                lists: lists.clone(),
                complete,
            });
        }
        lists
    }

    /// The library and whether every part of the read answered.
    async fn fetch(&self, api: &ApiClient, session: &SessionSlot) -> (Vec<Playlist>, bool) {
        let from_session = async {
            let session = session.wait().await?;
            match session_reads::within(session_reads::library(&session)).await {
                Ok(lists) => Some(lists),
                Err(failure) => {
                    tracing::info!("library over the playback session: {failure}");
                    None
                }
            }
        };
        let made = async {
            if self.no_made_for_you.load(Ordering::SeqCst) {
                return (Vec::new(), true);
            }
            match background(api.made_for_you()).await {
                Ok(lists) => (lists, true),
                Err(error) => {
                    let refused = matches!(error.status(), Some(403 | 404));
                    if refused {
                        self.no_made_for_you.store(true, Ordering::SeqCst);
                    }
                    tracing::debug!("made for you: {error}");
                    (Vec::new(), refused)
                }
            }
        };
        let (from_session, (made, made_complete)) = tokio::join!(from_session, made);
        let (mine, mine_complete) = match from_session {
            Some(lists) => (lists, true),
            None => web_library(api).await,
        };
        (
            Worker::merge_playlists(mine, made),
            mine_complete && made_complete,
        )
    }

    /// Puts a playlist this app just made at the top without a refetch.
    async fn add_front(&self, playlist: Playlist) -> Option<Vec<Playlist>> {
        let mut held = self.lists.lock().await;
        let library = held.as_mut()?;
        library.lists.retain(|existing| existing.id != playlist.id);
        library.lists.insert(0, playlist);
        Some(library.lists.clone())
    }

    async fn clear(&self) {
        *self.lists.lock().await = None;
    }
}

/// The account's playlists from the Web API, a page at a time, and whether
/// every page answered.
async fn web_library(api: &ApiClient) -> (Vec<Playlist>, bool) {
    let mut lists = Vec::new();
    let mut offset = 0_u32;
    while offset < LIBRARY_MAX {
        match api.my_playlists(offset, PAGE_SIZE).await {
            Ok(page) => {
                let next = page.next_offset();
                lists.extend(page.items);
                match next {
                    Some(next) if next > offset => offset = next,
                    _ => return (lists, true),
                }
            }
            Err(error) => {
                tracing::debug!("library playlists: {error}");
                return (lists, false);
            }
        }
    }
    (lists, true)
}

/// When the playback poll is next due. `command_at` is the last command to
/// another device; `pushed` means a Connect push arrived since the last
/// poll; `pushes` means the session delivers them at all; `playing` is the
/// remote song's `(position, duration)` at the last poll.
fn remote_poll_due(
    polled_at: Option<Instant>,
    command_at: Option<Instant>,
    pushed: bool,
    pushes: bool,
    playing: Option<(u32, u32)>,
    now: Instant,
) -> bool {
    let Some(polled_at) = polled_at else {
        return true;
    };
    let elapsed = now.saturating_duration_since(polled_at);
    let commanded =
        command_at.is_some_and(|at| now.saturating_duration_since(at) < REMOTE_COMMAND_WINDOW);
    let interval = if commanded {
        REMOTE_POLL_ACTIVE
    } else if pushes {
        REMOTE_POLL_PUSHED
    } else {
        REMOTE_POLL
    };
    if elapsed >= interval || (pushed && elapsed >= REMOTE_PUSH_GAP) {
        return true;
    }
    // Without pushes, look once just after the remote song should end.
    if !pushes && let Some((position, duration)) = playing.filter(|(_, duration)| *duration > 0) {
        let left = Duration::from_millis(u64::from(duration.saturating_sub(position)) + 1_000);
        return elapsed >= left.max(REMOTE_POLL_ACTIVE);
    }
    false
}

/// Whether an incremental Liked Songs read has seen enough. Spotify lists
/// the newest likes first: once a whole page is songs already known and the
/// known songs plus the new ones add up to Spotify's total, nothing older
/// can have changed. Returns the settled set.
fn liked_settled(
    known: &HashSet<String>,
    seen: &[String],
    page_all_known: bool,
    total: u32,
) -> Option<HashSet<String>> {
    if known.is_empty() || !page_all_known {
        return None;
    }
    let merged: HashSet<String> = known.iter().chain(seen).cloned().collect();
    (merged.len() == total as usize).then_some(merged)
}

/// Starts the worker thread. The receiver hears once the worker has shut
/// down (local playback unregistered), so `main` can wait for it on quit.
pub fn start() -> std::sync::mpsc::Receiver<()> {
    let (done_tx, done_rx) = std::sync::mpsc::channel();
    std::thread::Builder::new()
        .name("sola-spotify-worker".into())
        .spawn(move || {
            let rt = tokio::runtime::Builder::new_multi_thread()
                .enable_all()
                .thread_name("sola-spotify-rt")
                .build()
                .expect("spotify tokio runtime");
            rt.block_on(run());
            let _ = done_tx.send(());
        })
        .expect("spawn spotify worker");
    done_rx
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
    let api = Arc::new(ApiClient::new(http.clone(), 20, 50));
    let art = ArtLoader::new(http.clone(), dirs.art_cache_dir());
    {
        let art = art.clone();
        let pages = dirs.page_cache_dir();
        tokio::task::spawn_blocking(move || {
            art.prune_disk();
            crate::images::prune_dir(&pages, PAGE_CACHE_LIMIT, PAGE_CACHE_TARGET);
        });
    }

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

    let liked = Liked::load(&dirs);
    let mut worker = Worker {
        skipped: Skipped::load(&dirs),
        // A liked set from the last run is good enough to mark rows at once;
        // the sync shortly after start corrects it in (usually) one request.
        liked_ready: Arc::new(AtomicBool::new(!liked.uris.is_empty())),
        liked,
        liked_synced_at: None,
        liked_syncing: false,
        session: SessionSlot::new(),
        library: Arc::new(Library::default()),
        fetched: HashMap::new(),
        favorites: Arc::default(),
        search_gen: Arc::new(AtomicU64::new(0)),
        last_search: None,
        remote: None,
        remote_polled_at: None,
        remote_inflight: false,
        remote_command_at: None,
        remote_pushed: false,
        resume_gen: 0,
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
        profile_fresh: false,
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
                // A Connect push or a command can make a poll due at once.
                worker.poll_remote_if_due().await;
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
    // Unregister this Connect device before the process goes, so it does
    // not linger in other apps' device lists.
    if let Some(engine) = worker.engine.take() {
        engine.shutdown_wait().await;
    }
}

struct Worker {
    dirs: AppDirs,
    settings: Settings,
    http: reqwest::Client,
    api: Arc<ApiClient>,
    art: ArtLoader,
    session: SessionSlot,
    library: Arc<Library>,
    /// When each page last loaded; reused while `Page::fresh_for` allows.
    fetched: HashMap<Page, Instant>,
    /// Album saved / artist followed answers, kept for the session.
    favorites: Arc<std::sync::Mutex<HashMap<(FavoriteKind, String), bool>>>,
    /// Bumped per search, so a slow older answer never replaces a newer one.
    search_gen: Arc<AtomicU64>,
    last_search: Option<(String, Instant)>,
    /// Playback on another device, from the last poll.
    remote: Option<NowPlaying>,
    remote_polled_at: Option<Instant>,
    remote_inflight: bool,
    remote_command_at: Option<Instant>,
    /// A Connect push arrived since the last poll.
    remote_pushed: bool,
    /// Generation of the latest `arm_resume`; older verify timers are ignored.
    resume_gen: u64,
    /// The liked set is complete enough to mark rows without asking Spotify.
    liked_ready: Arc<AtomicBool>,
    liked_synced_at: Option<Instant>,
    liked_syncing: bool,
    int_tx: mpsc::UnboundedSender<Internal>,
    engine: Option<Engine>,
    engine_busy: bool,
    signed_in: bool,
    premium: Option<bool>,
    user: Option<User>,
    /// A profile arrived since sign-in; `user` may only be last run's.
    profile_fresh: bool,
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
            Some(token) if token.has_scopes(auth::WEB_SCOPES) => self.on_web_signed_in(token),
            Some(_) => bridge::emit(Event::Auth(AuthStatus::Failed(
                "Spotify permissions changed. Sign in again.".into(),
            ))),
            None => bridge::emit(Event::Auth(AuthStatus::SignedOut)),
        }
    }

    fn on_web_signed_in(&mut self, token: StoredToken) {
        let tokens = WebTokens::new(self.http.clone(), token, self.dirs.shared_web_token_file());
        self.api
            .set_token_provider(Some(TokenProvider::Web(tokens)));
        self.signed_in = true;
        // The app opens on the saved token and last run's profile: waiting
        // on `/me` held sign-in for a minute while the shared app was 429ing.
        self.profile_fresh = false;
        if self.user.is_none() {
            self.user = crate::cache::read_json(&self.dirs.profile_file());
        }
        if let Some(user) = &self.user {
            self.premium = user.product.as_deref().map(|p| p == "premium");
            bridge::emit(Event::User(user.clone()));
            bridge::emit(Event::Premium(self.premium));
        }
        bridge::emit(Event::Auth(AuthStatus::Connected {
            username: self.user.as_ref().map_or("Spotify", User::name).to_string(),
        }));
        self.resume_engine();
        // A starting session brings the profile itself.
        if !self.engine_busy {
            self.refresh_me();
        }
        if !self.liked.uris.is_empty() {
            bridge::emit(Event::Liked(self.liked.uris.iter().cloned().collect()));
        }
        let last = Page::decode(&self.settings.last_page).unwrap_or(Page::Home);
        self.open(last);
        self.refresh_library_playlists(false);
        self.refresh_playback();
        self.sync_liked(Duration::from_millis(1_200));
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
            Cmd::Play { request, device_id } => self.play(request, device_id).await,
            Cmd::Transfer { device_id, play } => self.transfer(device_id, play).await,
            Cmd::Open(page) => self.open(page),
            Cmd::Refresh(page) => {
                self.fetched.remove(&page);
                match page {
                    // The library is re-read in the spawned load; taking its
                    // lock here would hold the loop for the whole fetch.
                    Page::Home => self.load_home(true),
                    Page::MadeForYou => self.load_made_for_you(true),
                    Page::Search => {
                        self.last_search = None;
                        self.open(page);
                    }
                    page => self.open(page),
                }
            }
            Cmd::Search(query) => {
                self.search_query = query;
                self.open(Page::Search);
            }
            Cmd::LoadMore { page, offset } => self.load_more(page, offset),
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
            Cmd::RefreshDevices => self.refresh_devices(),
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
                self.session
                    .set(engine.as_ref().map(crate::player::Engine::session));
                self.session.set_connecting(false);
                match (engine, error) {
                    (Some(engine), _) => {
                        let device_id = engine.device_id().to_string();
                        tracing::info!(device_id = %device_id, "local playback ready");
                        self.watch_cluster(engine.session());
                        if !self.profile_fresh {
                            self.read_profile(engine.session());
                        }
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
                        self.refresh_me();
                    }
                    _ => {
                        self.resume = None;
                        self.resume_verify = None;
                        bridge::emit(Event::LocalPlayback(LocalPlayback::Unavailable));
                        self.refresh_me();
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
            Internal::VerifyResume(generation) => {
                if generation == self.resume_gen {
                    self.verify_resume();
                }
            }
            Internal::PageFetched(page) => self.on_page_fetched(page),
            Internal::RemoteState(now) => self.on_remote_state(now),
            Internal::RemoteFailed => self.remote_inflight = false,
            Internal::ClusterChanged => self.remote_pushed = true,
            Internal::LikedSynced(uris) => {
                self.liked_syncing = false;
                self.liked_ready.store(true, Ordering::SeqCst);
                if uris != self.liked.uris {
                    self.liked.uris = uris;
                    self.liked.save(&self.dirs);
                    self.fetched.remove(&Page::Liked);
                    bridge::emit(Event::Liked(self.liked.uris.iter().cloned().collect()));
                }
            }
            Internal::LikedSyncEnded => self.liked_syncing = false,
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
        self.profile_fresh = false;
        self.profile_inflight = false;
        self.profile_retry_at = None;
        let _ = std::fs::remove_file(self.dirs.profile_file());
        self.resume_verify = None;
        if let Some(engine) = self.engine.take() {
            engine.shutdown();
        }
        if let Some(cancel) = self.cancel_signin.take() {
            let _ = cancel.send(true);
        }
        self.api.set_token_provider(None);
        self.session.set(None);
        self.session.set_connecting(false);
        self.fetched.clear();
        self.favorites
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .clear();
        self.remote = None;
        self.last_search = None;
        let library = Arc::clone(&self.library);
        tokio::spawn(async move { library.clear().await });
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
        self.session.set(None);
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
        self.session.set_connecting(true);
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
                    // While another device has playback, the poll of that
                    // device owns the player bar.
                    if state.connect_active {
                        apply_local_state(state.clone());
                    } else if let Some(error) = state.error.clone() {
                        bridge::emit(Event::Error(error));
                    }
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

    async fn transfer(&mut self, device_id: String, play: bool) {
        if let Some(engine) = &self.engine
            && engine.device_id() == device_id
        {
            let _ = engine.command(PlayerCommand::Activate);
            if play {
                let _ = engine.command(PlayerCommand::Play);
            }
            return;
        }
        self.remote_command_at = Some(Instant::now());
        let api = Arc::clone(&self.api);
        tokio::spawn(async move {
            if let Err(error) = api.transfer(&device_id, play).await {
                bridge::emit(Event::Error(friendly_player_error(&error)));
            }
        });
    }

    /// Loads `page` unless it loaded recently: the UI already shows what it
    /// holds, so a revisit inside `Page::fresh_for` costs no request.
    fn open(&mut self, page: Page) {
        if self.is_fresh(&page) {
            return;
        }
        match &page {
            Page::Home => self.load_home(false),
            Page::MadeForYou => self.load_made_for_you(false),
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

    fn is_fresh(&self, page: &Page) -> bool {
        if *page == Page::Search {
            return self.last_search.as_ref().is_some_and(|(query, at)| {
                query == self.search_query.trim() && at.elapsed() < page.fresh_for()
            });
        }
        self.fetched
            .get(page)
            .is_some_and(|at| at.elapsed() < page.fresh_for())
    }

    fn on_page_fetched(&mut self, page: Page) {
        if page == Page::Search {
            self.last_search = Some((self.search_query.trim().to_string(), Instant::now()));
        } else {
            self.fetched.insert(page, Instant::now());
        }
    }

    fn load_more(&mut self, page: Page, offset: u32) {
        match page {
            Page::Liked => self.load_liked(offset),
            Page::Albums => self.load_albums(offset),
            Page::Playlist(id) => self.load_playlist(id, offset),
            _ => {}
        }
    }

    fn load_home(&self, fresh_library: bool) {
        let api = Arc::clone(&self.api);
        let session = self.session.clone();
        let library = Arc::clone(&self.library);
        let liked_ready = Arc::clone(&self.liked_ready);
        let int_tx = self.int_tx.clone();
        tokio::spawn(async move {
            let (recent, playlists, top_artists, top_tracks) = tokio::join!(
                api.recently_played(20, None, None),
                library.playlists(&api, &session, fresh_library),
                api.top_artists("medium_term", 12),
                api.top_tracks("medium_term", 10, 0),
            );
            let complete = recent.is_ok() && top_artists.is_ok() && top_tracks.is_ok();
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
            if !playlists.is_empty() {
                bridge::emit(Event::Playlists(playlists));
            }
            if complete {
                let _ = int_tx.send(Internal::PageFetched(Page::Home));
            }
            Worker::emit_contains(api, &liked_ready, like_uris);
        });
    }

    fn merge_playlists(mut into: Vec<Playlist>, extra: Vec<Playlist>) -> Vec<Playlist> {
        let mut seen: HashSet<String> = into.iter().map(|p| p.id.clone()).collect();
        for playlist in extra {
            if seen.insert(playlist.id.clone()) {
                into.push(playlist);
            }
        }
        into
    }

    fn refresh_library_playlists(&self, force: bool) {
        let api = Arc::clone(&self.api);
        let session = self.session.clone();
        let library = Arc::clone(&self.library);
        tokio::spawn(async move {
            let playlists = library.playlists(&api, &session, force).await;
            if !playlists.is_empty() {
                bridge::emit(Event::Playlists(playlists));
            }
        });
    }

    fn load_made_for_you(&self, fresh_library: bool) {
        let api = Arc::clone(&self.api);
        let session = self.session.clone();
        let library = Arc::clone(&self.library);
        let int_tx = self.int_tx.clone();
        tokio::spawn(async move {
            let playlists = library.playlists(&api, &session, fresh_library).await;
            let found = !playlists.is_empty();
            bridge::emit(Event::Page {
                page: Page::MadeForYou,
                body: PageBody::Playlists {
                    title: "Made for you".into(),
                    subtitle: "Mixes Spotify builds from what you play.".into(),
                    items: playlists.clone(),
                },
            });
            // An empty answer may be a failed read: show it, ask again next time.
            if found {
                bridge::emit(Event::Playlists(playlists));
                let _ = int_tx.send(Internal::PageFetched(Page::MadeForYou));
            }
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
        let generation = self.search_gen.fetch_add(1, Ordering::SeqCst) + 1;
        let search_gen = Arc::clone(&self.search_gen);
        let api = Arc::clone(&self.api);
        let liked_ready = Arc::clone(&self.liked_ready);
        let int_tx = self.int_tx.clone();
        tokio::spawn(async move {
            let result = api
                .search(
                    &query,
                    &["track", "artist", "album", "playlist", "show", "episode"],
                )
                .await;
            // A newer search started while this one waited: its answer wins.
            if search_gen.load(Ordering::SeqCst) != generation {
                return;
            }
            match result {
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
                    let _ = int_tx.send(Internal::PageFetched(Page::Search));
                    Worker::emit_contains(api, &liked_ready, like_uris);
                }
                Err(error) => bridge::emit(Event::Error(error.to_string())),
            }
        });
    }

    fn load_liked(&self, offset: u32) {
        let api = Arc::clone(&self.api);
        let int_tx = self.int_tx.clone();
        tokio::spawn(async move {
            match api.saved_tracks(offset, PAGE_SIZE).await {
                Ok(page) => {
                    let next = page.next_offset();
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
                    bridge::emit(Event::SavedMany(
                        items
                            .iter()
                            .filter(|track| !track.uri.is_empty())
                            .map(|track| (track.uri.clone(), true))
                            .collect(),
                    ));
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
                            next,
                            album: None,
                        },
                    });
                    if offset == 0 {
                        let _ = int_tx.send(Internal::PageFetched(Page::Liked));
                    }
                }
                Err(error) => bridge::emit(Event::Error(error.to_string())),
            }
        });
    }

    fn load_albums(&self, offset: u32) {
        let api = Arc::clone(&self.api);
        let favorites = Arc::clone(&self.favorites);
        let int_tx = self.int_tx.clone();
        tokio::spawn(async move {
            match api.saved_albums(offset, PAGE_SIZE).await {
                Ok(page) => {
                    let next = page.next_offset();
                    let items: Vec<Album> = page.items.into_iter().map(|s| s.album).collect();
                    remember_favorites(
                        &favorites,
                        FavoriteKind::Album,
                        items.iter().filter_map(Album::catalog_id),
                    );
                    bridge::emit(Event::Page {
                        page: Page::Albums,
                        body: PageBody::Albums {
                            items,
                            total: page.total,
                            offset: page.offset,
                            next,
                        },
                    });
                    if offset == 0 {
                        let _ = int_tx.send(Internal::PageFetched(Page::Albums));
                    }
                }
                Err(error) => bridge::emit(Event::Error(error.to_string())),
            }
        });
    }

    fn load_artists(&self) {
        let api = Arc::clone(&self.api);
        let favorites = Arc::clone(&self.favorites);
        let int_tx = self.int_tx.clone();
        tokio::spawn(async move {
            match api.followed_artists(None, 50).await {
                Ok(page) => {
                    remember_favorites(
                        &favorites,
                        FavoriteKind::Artist,
                        page.items.iter().map(|artist| artist.id.as_str()),
                    );
                    bridge::emit(Event::Page {
                        page: Page::Artists,
                        body: PageBody::Artists { items: page.items },
                    });
                    let _ = int_tx.send(Internal::PageFetched(Page::Artists));
                }
                Err(error) => bridge::emit(Event::Error(error.to_string())),
            }
        });
    }

    fn load_playlist(&self, id: String, offset: u32) {
        let api = Arc::clone(&self.api);
        let session = self.session.clone();
        let liked_ready = Arc::clone(&self.liked_ready);
        let int_tx = self.int_tx.clone();
        tokio::spawn(async move {
            let (playlist, page) = match read_playlist(&api, &session, &id, offset).await {
                Ok(read) => read,
                Err(error) => {
                    bridge::emit(Event::Error(error.to_string()));
                    return;
                }
            };
            let next = page.next_offset();
            let items: Vec<Track> = page
                .items
                .iter()
                .filter_map(PlaylistItem::into_track)
                .collect();
            let art = crate::api::models::pick_image(&playlist.images, 300).map(str::to_string);
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
                    next,
                    album: None,
                },
            });
            if offset == 0 {
                let _ = int_tx.send(Internal::PageFetched(Page::Playlist(id)));
            }
            Worker::emit_contains(api, &liked_ready, like_uris);
        });
    }

    fn load_album(&self, id: String) {
        let api = Arc::clone(&self.api);
        let session = self.session.clone();
        let favorites = Arc::clone(&self.favorites);
        let liked_ready = Arc::clone(&self.liked_ready);
        let int_tx = self.int_tx.clone();
        tokio::spawn(async move {
            match read_album(&api, &session, &id).await {
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
                        page: Page::Album(id.clone()),
                        body: PageBody::Tracks {
                            title: album.name,
                            subtitle: format!("{artists} · {year}"),
                            art,
                            context_uri: Some(context_uri),
                            total: items.len() as u32,
                            offset: 0,
                            next: None,
                            items,
                            album: Some(header),
                        },
                    });
                    let _ = int_tx.send(Internal::PageFetched(Page::Album(id)));
                    Worker::emit_contains(api.clone(), &liked_ready, like_uris);
                    Worker::emit_library_state(api, favorites, Some(album_id), artist_id);
                }
                Err(error) => bridge::emit(Event::Error(error.to_string())),
            }
        });
    }

    fn load_artist(&self, id: String) {
        let api = Arc::clone(&self.api);
        let session = self.session.clone();
        let favorites = Arc::clone(&self.favorites);
        let liked_ready = Arc::clone(&self.liked_ready);
        let int_tx = self.int_tx.clone();
        tokio::spawn(async move {
            let (artist, top, albums) = tokio::join!(
                api.artist(&id),
                read_top_tracks(&api, &session, &id),
                api.artist_albums(&id, "album,single", 0, 20),
            );
            let artist = match artist {
                Ok(a) => a,
                Err(error) => {
                    bridge::emit(Event::Error(error.to_string()));
                    return;
                }
            };
            let complete = top.is_ok() && albums.is_ok();
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
                page: Page::Artist(id.clone()),
                body: PageBody::Artist {
                    artist,
                    top,
                    albums,
                },
            });
            if complete {
                let _ = int_tx.send(Internal::PageFetched(Page::Artist(id)));
            }
            Worker::emit_contains(api.clone(), &liked_ready, like_uris);
            Worker::emit_library_state(api, favorites, None, Some(artist_id));
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
                Err(error) => {
                    tracing::debug!("art {url}: {error}");
                    bridge::emit(Event::ArtFailed(url));
                }
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

    fn add_to_playlist(&mut self, playlist_id: String, name: String, uris: Vec<String>) {
        self.fetched.remove(&Page::Playlist(playlist_id.clone()));
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
        let library = Arc::clone(&self.library);
        tokio::spawn(async move {
            match api.create_playlist(&name, false, "").await {
                Ok(mut playlist) => {
                    let playlist_id = playlist.id.clone();
                    if playlist.name.is_empty() {
                        playlist.name = name;
                    }
                    let playlist_name = playlist.name.clone();
                    if !uris.is_empty() {
                        if let Err(error) = api.add_playlist_items(&playlist_id, &uris, None).await
                        {
                            bridge::emit(Event::Error(error.to_string()));
                            return;
                        }
                        playlist.bump_track_total(uris.len() as u32);
                    }
                    // The new list goes on top of the held library; no refetch.
                    if let Some(lists) = library.add_front(playlist).await {
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

    fn set_favorite(&mut self, kind: FavoriteKind, id: String, saved: bool) {
        if id.is_empty() {
            return;
        }
        self.fetched.remove(match kind {
            FavoriteKind::Album => &Page::Albums,
            FavoriteKind::Artist => &Page::Artists,
        });
        let api = Arc::clone(&self.api);
        let favorites = Arc::clone(&self.favorites);
        tokio::spawn(async move {
            let result = match kind {
                FavoriteKind::Album => api.set_saved_album(&id, saved).await,
                FavoriteKind::Artist => api.set_followed_artist(&id, saved).await,
            };
            match result {
                Ok(()) => {
                    favorites
                        .lock()
                        .unwrap_or_else(|p| p.into_inner())
                        .insert((kind, id.clone()), saved);
                    bridge::emit(Event::Favorite { kind, id, saved })
                }
                Err(error) => bridge::emit(Event::Error(error.to_string())),
            }
        });
    }

    /// Album saved / artist followed for a page's buttons: from what this
    /// session already knows, else one background request each.
    fn emit_library_state(
        api: Arc<ApiClient>,
        favorites: Favorites,
        album_id: Option<String>,
        artist_id: Option<String>,
    ) {
        tokio::spawn(background(async move {
            let wanted = [
                (FavoriteKind::Album, album_id),
                (FavoriteKind::Artist, artist_id),
            ];
            for (kind, id) in wanted {
                let Some(id) = id else {
                    continue;
                };
                let known = favorites
                    .lock()
                    .unwrap_or_else(|p| p.into_inner())
                    .get(&(kind, id.clone()))
                    .copied();
                let saved = match known {
                    Some(saved) => saved,
                    None => {
                        let answer = match kind {
                            FavoriteKind::Album => api.album_is_saved(&id).await,
                            FavoriteKind::Artist => api.artist_is_followed(&id).await,
                        };
                        match answer {
                            Ok(saved) => {
                                favorites
                                    .lock()
                                    .unwrap_or_else(|p| p.into_inner())
                                    .insert((kind, id.clone()), saved);
                                saved
                            }
                            Err(error) => {
                                tracing::debug!("library state {kind:?}: {error}");
                                continue;
                            }
                        }
                    }
                };
                bridge::emit(Event::Favorite { kind, id, saved });
            }
        }));
    }

    fn set_saved(&mut self, uri: String, saved: bool) {
        self.liked.set(uri.clone(), saved);
        self.liked.save(&self.dirs);
        self.fetched.remove(&Page::Liked);
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

    /// Brings `liked.json` up to date with Spotify after `delay`, in the
    /// background. Usually one request; see [`liked_settled`].
    fn sync_liked(&mut self, delay: Duration) {
        if self.liked_syncing {
            return;
        }
        self.liked_syncing = true;
        self.liked_synced_at = Some(Instant::now());
        let api = Arc::clone(&self.api);
        let known = self.liked.uris.clone();
        let int_tx = self.int_tx.clone();
        tokio::spawn(background(async move {
            tokio::time::sleep(delay).await;
            let message = match read_liked(&api, &known).await {
                Some(uris) => Internal::LikedSynced(uris),
                None => Internal::LikedSyncEnded,
            };
            let _ = int_tx.send(message);
        }));
    }

    /// Liked state for rows, only while the synced liked set is not ready:
    /// once it is, the UI marks rows from it without asking Spotify.
    fn emit_contains(api: Arc<ApiClient>, liked_ready: &AtomicBool, uris: Vec<String>) {
        if uris.is_empty() || liked_ready.load(Ordering::SeqCst) {
            return;
        }
        tokio::spawn(background(async move {
            for chunk in uris.chunks(50) {
                match api.contains(chunk).await {
                    Ok(flags) => {
                        bridge::emit(Event::SavedMany(chunk.iter().cloned().zip(flags).collect()))
                    }
                    Err(error) => {
                        tracing::warn!("contains: {error}");
                        break;
                    }
                }
            }
        }));
    }

    fn refresh_devices(&mut self) {
        let api = Arc::clone(&self.api);
        tokio::spawn(async move {
            match api.devices().await {
                Ok(devices) => bridge::emit(Event::Devices(devices)),
                Err(ApiError::NotSignedIn | ApiError::RateLimited) => {}
                Err(error) => tracing::debug!("devices: {error}"),
            }
        });
    }

    fn local_active(&self) -> bool {
        self.engine
            .as_ref()
            .is_some_and(crate::player::Engine::connect_active)
    }

    /// Whether the player bar follows another device: signed in, and this
    /// computer is not the active Connect device.
    fn watching_remote(&self) -> bool {
        self.signed_in && !self.engine_busy && !self.local_active()
    }

    /// Polls playback on another device when [`remote_poll_due`] says so.
    async fn poll_remote_if_due(&mut self) {
        if !self.watching_remote() {
            self.remote_pushed = false;
            return;
        }
        if self.remote_inflight || self.api.cooling_down().await {
            return;
        }
        let playing = self
            .remote
            .as_ref()
            .filter(|now| now.playback == Playback::Playing)
            .map(|now| (now.position_ms, now.duration_ms));
        if remote_poll_due(
            self.remote_polled_at,
            self.remote_command_at,
            self.remote_pushed,
            self.session.pushes(),
            playing,
            Instant::now(),
        ) {
            self.refresh_playback();
        }
    }

    /// Asks Spotify what plays on another device; one request in flight.
    fn refresh_playback(&mut self) {
        if self.remote_inflight || !self.signed_in {
            return;
        }
        self.remote_inflight = true;
        self.remote_pushed = false;
        self.remote_polled_at = Some(Instant::now());
        let api = Arc::clone(&self.api);
        let int_tx = self.int_tx.clone();
        tokio::spawn(background(async move {
            let message = match api.playback_state().await {
                Ok(state) => Internal::RemoteState(state.as_ref().map(now_from_remote)),
                Err(ApiError::NotSignedIn) => Internal::RemoteFailed,
                Err(error) => {
                    tracing::debug!("playback: {error}");
                    Internal::RemoteFailed
                }
            };
            let _ = int_tx.send(message);
        }));
    }

    fn on_remote_state(&mut self, now: Option<NowPlaying>) {
        self.remote_inflight = false;
        // This computer in the answer is the engine's to report.
        let local_id = self
            .engine
            .as_ref()
            .map(|engine| engine.device_id().to_string());
        let now = now.filter(|now| now.device_id.is_none() || now.device_id != local_id);
        self.remote = now.clone();
        if self.local_active() {
            return;
        }
        let Some(mut now) = now else {
            // Nothing plays anywhere now: stop the remote clock on the bar.
            if !self.now.is_local && self.now.playback == Playback::Playing {
                self.now.playback = Playback::Paused;
                bridge::emit(Event::NowPlaying(self.now.clone()));
            }
            return;
        };
        // Before the liked set is synced, "not in it" is not an answer.
        now.saved = self
            .liked_ready
            .load(Ordering::SeqCst)
            .then(|| self.liked.uris.contains(&now.uri));
        self.now = now.clone();
        bridge::emit(Event::NowPlaying(now));
    }

    /// Spotify Connect pushes a cluster update to every device of the account
    /// whenever playback changes on any of them: a hint to poll sooner.
    fn watch_cluster(&self, session: Session) {
        let updates = match session
            .dealer()
            .add_listen_for("hm://connect-state/v1/cluster")
        {
            Ok(updates) => updates,
            Err(error) => {
                tracing::debug!("Connect pushes unavailable: {error}");
                return;
            }
        };
        let int_tx = self.int_tx.clone();
        let pushes = Arc::clone(&self.session.pushes);
        pushes.store(true, Ordering::SeqCst);
        tokio::spawn(async move {
            let mut updates = Box::pin(updates);
            while updates.next().await.is_some() {
                if int_tx.send(Internal::ClusterChanged).is_err() {
                    break;
                }
            }
            pushes.store(false, Ordering::SeqCst);
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
            // A starting session brings the profile itself.
            self.profile_fresh || self.engine_busy,
            self.profile_inflight,
            self.profile_retry_at,
            Instant::now(),
        ) {
            self.refresh_me();
        }
        self.poll_remote_if_due().await;
        if self
            .liked_synced_at
            .is_some_and(|at| at.elapsed() >= LIKED_REFRESH)
        {
            self.sync_liked(Duration::ZERO);
        }
    }

    fn refresh_me(&mut self) {
        if self.profile_inflight || self.profile_fresh || !self.signed_in {
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
                Err(ApiError::NotSignedIn | ApiError::SignInExpired) => {
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

    /// The account as the playback session knows it, so `/me` is asked
    /// only when there is no session.
    fn read_profile(&mut self, session: Session) {
        self.profile_inflight = true;
        let int_tx = self.int_tx.clone();
        tokio::spawn(async move {
            let user = session_reads::me(&session).await;
            let _ = int_tx.send(Internal::ProfileOk(user));
        });
    }

    fn on_profile(&mut self, mut user: User) {
        self.profile_inflight = false;
        self.profile_retry_at = None;
        if !self.signed_in {
            return;
        }
        self.profile_fresh = true;
        // A session read can miss the display name or the plan.
        if let Some(known) = self.user.as_ref().filter(|known| known.id == user.id) {
            if user.display_name.is_none() {
                user.display_name = known.display_name.clone();
            }
            if user.product.is_none() {
                user.product = known.product.clone();
            }
        }
        let premium = user.product.as_deref().map(|p| p == "premium");
        self.user = Some(user.clone());
        self.premium = premium;
        crate::cache::write_json_later(self.dirs.profile_file(), user.clone());
        if premium == Some(false)
            && let Some(engine) = self.engine.take()
        {
            self.session.set(None);
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
        // Last run's profile may have said free and held the engine back.
        if premium == Some(true) {
            self.resume_engine();
        }
    }

    fn on_profile_skipped(&mut self) {
        self.profile_inflight = false;
        self.profile_retry_at = Some(Instant::now() + Duration::from_secs(300));
    }

    fn arm_resume(&mut self, spec: LoadSpec, delay: Duration) {
        self.resume_verify = Some((spec, 0));
        self.schedule_verify(delay);
    }

    /// One verify timer counts at a time: a newer arm makes older timers
    /// no-ops, so two quick clicks cannot run two retry chains.
    fn schedule_verify(&mut self, delay: Duration) {
        self.resume_gen = self.resume_gen.wrapping_add(1);
        let generation = self.resume_gen;
        let int_tx = self.int_tx.clone();
        tokio::spawn(async move {
            tokio::time::sleep(delay).await;
            let _ = int_tx.send(Internal::VerifyResume(generation));
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
        self.schedule_verify(Duration::from_secs(4));
    }

    async fn media(&mut self, command: MediaCommand) {
        match command {
            MediaCommand::Play => {
                if self.local_target() {
                    self.player(PlayerCommand::Play);
                } else if let Some(device) = self.remote_device() {
                    self.show_remote_playback(Playback::Playing);
                    self.remote_command(
                        move |api| async move { api.play(Some(&device), None).await },
                    );
                } else {
                    self.play(self.now_as_play_request(), None).await;
                }
            }
            MediaCommand::Pause | MediaCommand::Stop => {
                if self.local_target() {
                    self.player(PlayerCommand::Pause);
                } else {
                    self.show_remote_playback(Playback::Paused);
                    let device = self.remote_device();
                    self.remote_command(
                        move |api| async move { api.pause(device.as_deref()).await },
                    );
                }
            }
            MediaCommand::PlayPause => {
                if self.local_target() {
                    self.player(PlayerCommand::Toggle);
                } else if self.now.playback == Playback::Playing {
                    self.show_remote_playback(Playback::Paused);
                    let device = self.remote_device();
                    self.remote_command(
                        move |api| async move { api.pause(device.as_deref()).await },
                    );
                } else if let Some(device) = self.remote_device() {
                    self.show_remote_playback(Playback::Playing);
                    self.remote_command(
                        move |api| async move { api.play(Some(&device), None).await },
                    );
                } else {
                    self.play(self.now_as_play_request(), None).await;
                }
            }
            MediaCommand::Next => {
                if self.local_target() {
                    self.ignore_current_track();
                    self.advance_gen = self.advance_gen.wrapping_add(1);
                    self.player(PlayerCommand::Next);
                } else {
                    let device = self.remote_device();
                    self.remote_command(
                        move |api| async move { api.next(device.as_deref()).await },
                    );
                }
            }
            MediaCommand::Previous => {
                if self.local_target() {
                    self.ignore_current_track();
                    self.advance_gen = self.advance_gen.wrapping_add(1);
                    self.player(PlayerCommand::Previous);
                } else {
                    let device = self.remote_device();
                    self.remote_command(
                        move |api| async move { api.previous(device.as_deref()).await },
                    );
                }
            }
            MediaCommand::SeekBy(delta) => {
                let pos = self.now.position_ms as i64 + delta;
                let pos = pos.max(0) as u32;
                self.seek(pos);
            }
            MediaCommand::SetPosition {
                track_uri,
                position_ms,
            } => {
                if self.now.uri == track_uri || self.now.uri.is_empty() {
                    self.seek(position_ms);
                }
            }
            MediaCommand::SetVolume(volume) => {
                let percent = (volume * 100.0).clamp(0.0, 100.0) as u8;
                let librespot = ((percent as u32) * 65535 / 100) as u16;
                if self.local_target() {
                    self.player(PlayerCommand::Volume(librespot));
                } else {
                    self.now.volume_percent = percent;
                    let device = self.remote_device();
                    self.remote_command(move |api| async move {
                        api.set_volume(percent, device.as_deref()).await
                    });
                }
            }
            MediaCommand::SetShuffle(shuffle) => {
                if self.local_target() {
                    self.player(PlayerCommand::Shuffle(shuffle));
                } else {
                    self.now.shuffle = shuffle;
                    let device = self.remote_device();
                    self.remote_command(move |api| async move {
                        api.set_shuffle(shuffle, device.as_deref()).await
                    });
                }
            }
            MediaCommand::SetRepeat(mode) => {
                if self.local_target() {
                    self.player(PlayerCommand::Repeat(mode));
                } else {
                    self.now.repeat = mode;
                    let device = self.remote_device();
                    self.remote_command(move |api| async move {
                        api.set_repeat(mode.api_name(), device.as_deref()).await
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

    /// Transport goes to this computer when it is the active Connect device,
    /// or when nothing plays elsewhere (so Play starts it here).
    fn local_target(&self) -> bool {
        self.engine.is_some() && (self.local_active() || self.remote_device().is_none())
    }

    /// The other device playing or paused, from the last poll.
    fn remote_device(&self) -> Option<String> {
        self.remote
            .as_ref()
            .filter(|now| now.playback != Playback::Stopped)
            .and_then(|now| now.device_id.clone())
    }

    /// Sends a command to another device without holding the worker loop:
    /// a rate-limited Web API can take a minute to answer. Polls quickly for
    /// a few seconds afterwards to confirm what happened.
    fn remote_command<F, Fut>(&mut self, command: F)
    where
        F: FnOnce(Arc<ApiClient>) -> Fut + Send + 'static,
        Fut: std::future::Future<Output = Result<(), ApiError>> + Send,
    {
        self.remote_command_at = Some(Instant::now());
        let api = Arc::clone(&self.api);
        tokio::spawn(async move {
            if let Err(error) = command(api).await {
                bridge::emit(Event::Error(friendly_player_error(&error)));
            }
        });
    }

    /// Shows a play/pause on another device at once; the next poll confirms.
    fn show_remote_playback(&mut self, playback: Playback) {
        if self.now.uri.is_empty() {
            return;
        }
        self.now.playback = playback;
        if let Some(remote) = &mut self.remote {
            remote.playback = playback;
        }
        bridge::emit(Event::NowPlaying(self.now.clone()));
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
        if !state.connect_active {
            // Another device took playback. Its poll owns the player bar,
            // and the local player stopping is not ours to advance past.
            if self.last_playback != Playback::Stopped {
                self.last_playback = Playback::Stopped;
                self.advance_gen = self.advance_gen.wrapping_add(1);
                self.remote_command_at = Some(Instant::now());
                self.remote_pushed = true;
            }
            return;
        }
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

    fn seek(&mut self, position_ms: u32) {
        if self.local_target() {
            self.player(PlayerCommand::Seek(position_ms));
            return;
        }
        self.now.position_ms = position_ms;
        let device = self.remote_device();
        self.remote_command(
            move |api| async move { api.seek(position_ms, device.as_deref()).await },
        );
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

type Favorites = Arc<std::sync::Mutex<HashMap<(FavoriteKind, String), bool>>>;

fn remember_favorites<'a>(
    favorites: &Favorites,
    kind: FavoriteKind,
    ids: impl Iterator<Item = &'a str>,
) {
    let mut known = favorites.lock().unwrap_or_else(|p| p.into_inner());
    for id in ids.filter(|id| !id.is_empty()) {
        known.insert((kind, id.to_string()), true);
    }
}

/// Tries the playback session first: it has no per-app quota. A refusal it
/// gives is final; anything else falls back to the Web API.
async fn from_session<T, F, Fut>(
    session: &SessionSlot,
    what: &str,
    read: F,
) -> Option<Result<T, ApiError>>
where
    F: FnOnce(Session) -> Fut,
    Fut: std::future::Future<Output = Result<T, Failure>>,
{
    let session = session.wait().await?;
    match session_reads::within(read(session)).await {
        Ok(found) => Some(Ok(found)),
        Err(Failure::Definitive(error)) => Some(Err(error)),
        Err(Failure::Retry(reason)) => {
            tracing::info!("{what} over the playback session failed; asking the Web API: {reason}");
            None
        }
    }
}

async fn read_playlist(
    api: &ApiClient,
    session: &SessionSlot,
    id: &str,
    offset: u32,
) -> Result<(Playlist, crate::api::models::Page<PlaylistItem>), ApiError> {
    let read = from_session(session, "playlist", |session| async move {
        session_reads::playlist_page(&session, id, offset, SESSION_PAGE_SIZE).await
    });
    if let Some(found) = read.await {
        return found;
    }
    if offset > 0 {
        // Only the rows are merged into a page already showing its header.
        let page = api.playlist_items(id, offset, PAGE_SIZE).await?;
        return Ok((Playlist::default(), page));
    }
    let (playlist, embedded) = api.playlist_with_items(id).await?;
    let page = match embedded {
        Some(page) => page,
        None => api.playlist_items(id, 0, PAGE_SIZE).await?,
    };
    Ok((playlist, page))
}

async fn read_album(api: &ApiClient, session: &SessionSlot, id: &str) -> Result<Album, ApiError> {
    let read = from_session(session, "album", |session| async move {
        session_reads::album(&session, id).await
    });
    if let Some(found) = read.await {
        return found;
    }
    api.album(id).await
}

async fn read_top_tracks(
    api: &ApiClient,
    session: &SessionSlot,
    id: &str,
) -> Result<Vec<Track>, ApiError> {
    let read = from_session(session, "popular tracks", |session| async move {
        session_reads::artist_top_tracks(&session, id).await
    });
    if let Some(found) = read.await {
        return found;
    }
    api.artist_top_tracks(id).await
}

fn saved_uri(track: &Track) -> Option<String> {
    if !track.uri.is_empty() {
        return Some(track.uri.clone());
    }
    track.id.as_ref().map(|id| format!("spotify:track:{id}"))
}

/// Liked Songs as Spotify has them now, read newest first until
/// [`liked_settled`] says the rest is unchanged. `None` when Spotify could
/// not answer; the known set stands.
async fn read_liked(api: &ApiClient, known: &HashSet<String>) -> Option<HashSet<String>> {
    let mut seen: Vec<String> = Vec::new();
    let mut offset = 0_u32;
    loop {
        if api.cooling_down().await {
            return None;
        }
        let page = match api.saved_tracks(offset, PAGE_SIZE).await {
            Ok(page) => page,
            Err(error) => {
                tracing::warn!("liked library: {error}");
                return None;
            }
        };
        let uris: Vec<String> = page
            .items
            .iter()
            .filter_map(|saved| saved_uri(&saved.track))
            .collect();
        let all_known = !uris.is_empty() && uris.iter().all(|uri| known.contains(uri));
        seen.extend(uris);
        if let Some(settled) = liked_settled(known, &seen, all_known, page.total) {
            return Some(settled);
        }
        match page.next_offset() {
            Some(next) if next > offset => offset = next,
            _ => return Some(seen.into_iter().collect()),
        }
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

/// Whether to ask `/me`. `settled`: a profile arrived since sign-in, or a
/// starting session will bring one.
fn profile_poll_due(
    settled: bool,
    inflight: bool,
    retry_at: Option<Instant>,
    now: Instant,
) -> bool {
    !settled && !inflight && retry_at.is_none_or(|at| now >= at)
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
    fn remote_playback_is_polled_slowly_unless_something_changed() {
        let now = Instant::now();
        let ago = |secs| Some(now - Duration::from_secs(secs));
        assert!(remote_poll_due(None, None, false, false, None, now));
        // Without a session: every 15 seconds.
        assert!(!remote_poll_due(ago(10), None, false, false, None, now));
        assert!(remote_poll_due(ago(15), None, false, false, None, now));
        // With Connect pushes: a minute, or soon after a push.
        assert!(!remote_poll_due(ago(30), None, false, true, None, now));
        assert!(remote_poll_due(ago(60), None, false, true, None, now));
        assert!(!remote_poll_due(ago(1), None, true, true, None, now));
        assert!(remote_poll_due(ago(2), None, true, true, None, now));
        // Shortly after a command: every 4 seconds.
        assert!(remote_poll_due(ago(4), ago(3), false, true, None, now));
        assert!(!remote_poll_due(ago(4), ago(20), false, true, None, now));
    }

    #[test]
    fn a_remote_song_ending_is_looked_at_once_without_pushes() {
        let now = Instant::now();
        let polled = Some(now - Duration::from_secs(6));
        // 5 seconds were left at the last poll: due a second after it ends.
        assert!(remote_poll_due(
            polled,
            None,
            false,
            false,
            Some((175_000, 180_000)),
            now
        ));
        // Plenty left: wait for the regular interval.
        assert!(!remote_poll_due(
            polled,
            None,
            false,
            false,
            Some((10_000, 180_000)),
            now
        ));
        // Pushes announce the next song themselves.
        assert!(!remote_poll_due(
            polled,
            None,
            false,
            true,
            Some((175_000, 180_000)),
            now
        ));
    }

    #[test]
    fn liked_songs_settle_after_one_page_when_nothing_changed() {
        let known: HashSet<String> = ["a", "b", "c"].map(String::from).into();
        let seen = ["a", "b"].map(String::from);
        assert_eq!(liked_settled(&known, &seen, true, 3), Some(known.clone()));
        // A new like on top: the page was not all known.
        let seen = ["z", "a"].map(String::from);
        assert_eq!(liked_settled(&known, &seen, false, 4), None);
        // An unlike elsewhere: the count does not add up, so read on.
        let seen = ["a", "b"].map(String::from);
        assert_eq!(liked_settled(&known, &seen, true, 2), None);
        // Nothing known yet: the whole list must be read.
        assert_eq!(liked_settled(&HashSet::new(), &seen, true, 2), None);
    }

    #[test]
    fn catalogue_pages_stay_fresh_longer_than_library_pages() {
        assert!(Page::Album("a".into()).fresh_for() > Page::Playlist("p".into()).fresh_for());
        assert!(Page::Home.fresh_for() >= Duration::from_secs(60));
        assert_eq!(Page::Queue.fresh_for(), Duration::ZERO);
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
