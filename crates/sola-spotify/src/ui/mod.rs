//! Kit UI: library rail, pages, player bar.

mod nav;

use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::Duration;

use iced::keyboard;
use iced::keyboard::key::Named as NamedKey;
use iced::mouse;
use iced::widget::image::Handle as ImageHandle;
use iced::widget::{
    Space, button, column, container, image as iced_image, mouse_area, operation, rich_text, row,
    scrollable, slider, span, stack,
};
use iced::{
    Alignment, Background, Border, Color, Element, Event as IcedEvent, Length, Padding,
    Subscription, Task, Theme,
};
use sola_bus::Message;
use sola_bus::topics::Topic;
use sola_kit::app::{apply_theme_update, bus_subscription, is_self_quit};
use sola_kit::components::icon::{icon_handle, icon_svg, icon_svg_colored};
use sola_kit::components::popover::{Placement, popover, popover_anchored};
use sola_kit::components::style::{
    CHROME_SURFACE, RADIUS_SM, SPACE_LG, SPACE_MD, SPACE_SM, SPACE_XL, SPACE_XS, alpha, hairline,
    mix_white,
};
use sola_kit::components::text as kit_text;
use sola_kit::components::text_input::text_input;
use sola_kit::components::toolbar::toolbar_icon_tip;
use sola_kit::components::{SidebarItem, SidebarSection, button as kit_btn, sidebar};
use sola_kit::fonts;
use sola_kit::theme::default_theme;

use crate::api::PlayRequest as ApiPlay;
use crate::api::models::{
    Album, Artist, ArtistRef, Device, PlayableItem, Playlist, Track, format_added_at,
    generated_sort_key, pick_image, playlists_for_add,
};
use crate::bridge;
use crate::cache::{self, Liked, Skipped};
use crate::paths::AppDirs;
use crate::player::Playback;
use crate::settings::Settings;
use crate::worker::{
    AuthStatus, Cmd, Event, FavoriteKind, LocalPlayback, NowPlaying, Page, PageBody,
};

use self::nav::{NavEntry, NavHistory};

const APP_ID: &str = "sola-spotify";
const SIDEBAR_W: f32 = 220.0;
const PLAYER_H: f32 = 84.0;
const COVER_ROW: f32 = 40.0;
const COVER_TILE: f32 = 148.0;
const COVER_PLAYER: f32 = 56.0;
const MADE_SHELF: usize = 8;

#[derive(Debug, Clone)]
pub enum Msg {
    Bus(Arc<Message>),
    Worker(Event),
    SignIn,
    CancelSignIn,
    SignOut,
    PlayHere,
    Open(Page),
    Back,
    Forward,
    SearchChanged(String),
    SearchSubmit,
    PlayTrack {
        uri: String,
        context: Option<String>,
    },
    PlayContext(String),
    OpenArtist(String),
    OpenAlbum(String),
    OpenPlaylist(String),
    HoverTrack(String),
    UnhoverTrack(String),
    Toggle,
    Next,
    Prev,
    SeekFrac(f32),
    Volume(u8),
    Shuffle,
    Repeat,
    Like,
    SaveTrack(String),
    ToggleAlbum,
    ToggleArtist,
    SkipTrack {
        uri: String,
    },
    ToggleAddTo(String),
    AddToFilter(String),
    AddToPlaylist {
        id: String,
        name: String,
    },
    AddToSubmit,
    CreatePlaylist,
    CloseAddTo,
    Transfer(String),
    ToggleDevices,
    LoadMore,
    DismissError,
    DismissNotice,
    WindowReady(Option<iced::window::Id>),
    TitleDrag,
    TitleResize(iced::window::Direction),
    TitleClose,
    Tick,
}

struct Icons {
    house: iced::widget::svg::Handle,
    search: iced::widget::svg::Handle,
    heart: iced::widget::svg::Handle,
    disc: iced::widget::svg::Handle,
    users: iced::widget::svg::Handle,
    list: iced::widget::svg::Handle,
    play: iced::widget::svg::Handle,
    pause: iced::widget::svg::Handle,
    next: iced::widget::svg::Handle,
    prev: iced::widget::svg::Handle,
    back: iced::widget::svg::Handle,
    forward: iced::widget::svg::Handle,
    shuffle: iced::widget::svg::Handle,
    repeat: iced::widget::svg::Handle,
    speaker: iced::widget::svg::Handle,
    volume: iced::widget::svg::Handle,
    settings: iced::widget::svg::Handle,
    plus: iced::widget::svg::Handle,
    minus: iced::widget::svg::Handle,
    list_plus: iced::widget::svg::Handle,
    check: iced::widget::svg::Handle,
}

impl Icons {
    fn load() -> Self {
        Self {
            house: icon_handle("lucide/house"),
            search: icon_handle("lucide/search"),
            heart: icon_handle("lucide/heart"),
            disc: icon_handle("lucide/disc"),
            users: icon_handle("lucide/users"),
            list: icon_handle("lucide/list-music"),
            play: icon_handle("lucide/play"),
            pause: icon_handle("lucide/pause"),
            next: icon_handle("lucide/skip-forward"),
            prev: icon_handle("lucide/skip-back"),
            back: icon_handle("lucide/chevron-left"),
            forward: icon_handle("lucide/chevron-right"),
            shuffle: icon_handle("lucide/shuffle"),
            repeat: icon_handle("lucide/repeat"),
            speaker: icon_handle("lucide/speaker"),
            volume: icon_handle("lucide/volume-2"),
            settings: icon_handle("lucide/settings"),
            plus: icon_handle("lucide/circle-plus"),
            minus: icon_handle("lucide/circle-minus"),
            list_plus: icon_handle("lucide/list-plus"),
            check: icon_handle("lucide/circle-check"),
        }
    }
}

pub struct App {
    theme: Theme,
    float: sola_kit::FloatState,
    window_id: Option<iced::window::Id>,
    icons: Icons,
    auth: AuthStatus,
    user: Option<crate::api::models::User>,
    premium: Option<bool>,
    local: LocalPlayback,
    page: Page,
    nav: NavHistory,
    body: Option<PageBody>,
    page_cache: HashMap<Page, PageBody>,
    now: NowPlaying,
    devices: Vec<Device>,
    playlists: Vec<Playlist>,
    art: HashMap<String, ImageHandle>,
    art_inflight: HashSet<String>,
    saved: HashMap<String, bool>,
    search: String,
    devices_open: bool,
    error: Option<String>,
    settings: Settings,
    playing_since: Option<std::time::Instant>,
    pending_uri: Option<String>,
    player_art_hold: Option<ImageHandle>,
    skipped: Skipped,
    dirs: AppDirs,
    /// Last focused/played track — graphite lift, independent of live playback.
    selected_uri: Option<String>,
    hovered_uri: Option<String>,
    library_albums: HashSet<String>,
    library_artists: HashSet<String>,
    add_to: Option<AddTo>,
    notice: Option<Notice>,
}

struct AddTo {
    uri: String,
    query: String,
}

struct Notice {
    text: String,
    until: std::time::Instant,
}

impl Default for App {
    fn default() -> Self {
        Self::from_disk()
    }
}

impl App {
    fn from_disk() -> Self {
        let dirs = AppDirs::discover();
        let settings = Settings::load(&dirs);
        let skipped = Skipped::load(&dirs);
        let fallback = Page::decode(&settings.last_page).unwrap_or(Page::Home);
        let nav = NavHistory::from_saved(&settings.nav, fallback);
        let current = nav.current().clone();
        let page = current.page.clone();
        let last_track = settings.last_track.clone();
        let mut app = Self {
            theme: default_theme(),
            float: sola_kit::FloatState::new(APP_ID),
            window_id: None,
            icons: Icons::load(),
            auth: AuthStatus::Starting,
            user: None,
            premium: None,
            local: LocalPlayback::Unavailable,
            page: page.clone(),
            nav,
            body: None,
            page_cache: HashMap::new(),
            now: NowPlaying::default(),
            devices: Vec::new(),
            playlists: Vec::new(),
            art: HashMap::new(),
            art_inflight: HashSet::new(),
            saved: HashMap::new(),
            search: current.search,
            devices_open: false,
            error: None,
            settings,
            playing_since: None,
            pending_uri: None,
            player_art_hold: None,
            skipped,
            dirs,
            selected_uri: (!last_track.is_empty()).then_some(last_track),
            hovered_uri: None,
            library_albums: HashSet::new(),
            library_artists: HashSet::new(),
            add_to: None,
            notice: None,
        };
        for uri in Liked::load(&app.dirs).uris {
            app.saved.insert(uri, true);
        }
        if let Some(home) = app.read_page_disk(&Page::Home) {
            if let PageBody::Home { playlists, .. } = &home {
                app.playlists = playlists.clone();
            }
            app.page_cache.insert(Page::Home, home.clone());
            if page == Page::Home {
                app.body = Some(home);
            }
        }
        if page != Page::Home
            && let Some(cached) = app.read_page_disk(&page)
        {
            app.page_cache.insert(page.clone(), cached.clone());
            app.body = Some(cached);
        }
        app
    }

    fn read_page_disk(&self, page: &Page) -> Option<PageBody> {
        let key = page.cache_key()?;
        let path = self.dirs.page_cache_dir().join(format!("{key}.json"));
        cache::read_json(&path)
    }

    fn write_page_disk(&self, page: &Page, body: &PageBody) {
        let Some(key) = page.cache_key() else {
            return;
        };
        let path = self.dirs.page_cache_dir().join(format!("{key}.json"));
        cache::write_json(&path, body);
    }

    pub fn boot() -> (Self, Task<Msg>) {
        (
            Self::from_disk(),
            sola_kit::window_ready_task(Msg::WindowReady),
        )
    }

    pub fn title(&self) -> String {
        if self.now.title.is_empty() {
            "Spotify".into()
        } else {
            format!("{} — Spotify", self.now.title)
        }
    }

    pub fn theme(&self) -> Theme {
        sola_kit::theme_for(self.float.is_floating_any(), &self.theme)
    }

    pub fn subscription(&self) -> Subscription<Msg> {
        let bus = bus_subscription().map(Msg::Bus);
        let worker = bridge::subscription().map(Msg::Worker);
        let keys = iced::event::listen_with(|event, _status, _id| match event {
            IcedEvent::Keyboard(keyboard::Event::KeyPressed { key, modifiers, .. }) => {
                key_msg(key, modifiers)
            }
            IcedEvent::Mouse(mouse::Event::ButtonPressed(mouse::Button::Back)) => Some(Msg::Back),
            IcedEvent::Mouse(mouse::Event::ButtonPressed(mouse::Button::Forward)) => {
                Some(Msg::Forward)
            }
            _ => None,
        });
        let tick = if self.now.playback == Playback::Playing || self.notice.is_some() {
            iced::time::every(Duration::from_millis(250)).map(|_| Msg::Tick)
        } else {
            Subscription::none()
        };
        Subscription::batch([bus, worker, keys, tick])
    }

    pub fn update(&mut self, msg: Msg) -> Task<Msg> {
        match msg {
            Msg::Bus(message) => self.on_bus(&message),
            Msg::Worker(ev) => self.on_worker(ev),
            Msg::SignIn => {
                bridge::send(Cmd::SignIn);
                Task::none()
            }
            Msg::CancelSignIn => {
                bridge::send(Cmd::CancelSignIn);
                Task::none()
            }
            Msg::SignOut => {
                bridge::send(Cmd::SignOut);
                Task::none()
            }
            Msg::PlayHere => {
                self.devices_open = false;
                bridge::send(Cmd::AuthorizePlayback);
                Task::none()
            }
            Msg::Open(page) => {
                self.navigate(page);
                Task::none()
            }
            Msg::Back => {
                self.go_back();
                Task::none()
            }
            Msg::Forward => {
                self.go_forward();
                Task::none()
            }
            Msg::SearchChanged(s) => {
                self.search = s;
                Task::none()
            }
            Msg::SearchSubmit => {
                self.navigate(Page::Search);
                Task::none()
            }
            Msg::PlayTrack { uri, context } => {
                self.play_now(&uri, context);
                Task::none()
            }
            Msg::PlayContext(uri) => {
                let uris = self.current_track_uris();
                let mut request = if uris.len() >= 2 {
                    ApiPlay::tracks(uris)
                } else {
                    ApiPlay::context(uri.clone())
                };
                if request.context_uri.is_none() {
                    request.context_uri = Some(uri);
                }
                bridge::send(Cmd::Play {
                    request,
                    device_id: None,
                });
                Task::none()
            }
            Msg::OpenArtist(id) => {
                self.navigate(Page::Artist(id));
                Task::none()
            }
            Msg::OpenAlbum(id) => {
                self.navigate(Page::Album(id));
                Task::none()
            }
            Msg::OpenPlaylist(id) => {
                self.navigate(Page::Playlist(id));
                Task::none()
            }
            Msg::HoverTrack(uri) => {
                self.hovered_uri = Some(uri);
                Task::none()
            }
            Msg::UnhoverTrack(uri) => {
                if self.hovered_uri.as_deref() == Some(uri.as_str()) {
                    self.hovered_uri = None;
                }
                Task::none()
            }
            Msg::Toggle => {
                self.transport_toggle();
                Task::none()
            }
            Msg::Next => {
                self.pending_uri = None;
                bridge::send(Cmd::Media(crate::media::MediaCommand::Next));
                Task::none()
            }
            Msg::Prev => {
                self.pending_uri = None;
                bridge::send(Cmd::Media(crate::media::MediaCommand::Previous));
                Task::none()
            }
            Msg::SeekFrac(frac) => {
                if self.now.duration_ms > 0 {
                    let pos = (frac.clamp(0.0, 1.0) * self.now.duration_ms as f32) as u32;
                    bridge::send(Cmd::Media(crate::media::MediaCommand::SetPosition {
                        track_uri: self.now.uri.clone(),
                        position_ms: pos,
                    }));
                    self.now.position_ms = pos;
                    self.playing_since =
                        (self.now.playback == Playback::Playing).then(std::time::Instant::now);
                }
                Task::none()
            }
            Msg::Volume(percent) => {
                bridge::send(Cmd::Media(crate::media::MediaCommand::SetVolume(
                    percent as f64 / 100.0,
                )));
                self.now.volume_percent = percent;
                Task::none()
            }
            Msg::Shuffle => {
                let next = !self.now.shuffle;
                bridge::send(Cmd::Media(crate::media::MediaCommand::SetShuffle(next)));
                self.now.shuffle = next;
                Task::none()
            }
            Msg::Repeat => {
                let next = self.now.repeat.next();
                bridge::send(Cmd::Media(crate::media::MediaCommand::SetRepeat(next)));
                self.now.repeat = next;
                Task::none()
            }
            Msg::Like => {
                if !self.now.uri.is_empty() {
                    self.toggle_saved(&self.now.uri.clone());
                }
                Task::none()
            }
            Msg::SaveTrack(uri) => {
                self.toggle_saved(&uri);
                Task::none()
            }
            Msg::ToggleAlbum => {
                self.toggle_album();
                Task::none()
            }
            Msg::ToggleArtist => {
                self.toggle_artist();
                Task::none()
            }
            Msg::SkipTrack { uri } => {
                self.toggle_skip(uri);
                Task::none()
            }
            Msg::ToggleAddTo(uri) => self.toggle_add_to(uri),
            Msg::AddToFilter(query) => {
                if let Some(add_to) = &mut self.add_to {
                    add_to.query = query;
                }
                Task::none()
            }
            Msg::AddToPlaylist { id, name } => {
                self.commit_add_to(id, name);
                Task::none()
            }
            Msg::AddToSubmit => {
                self.submit_add_to();
                Task::none()
            }
            Msg::CreatePlaylist => {
                self.create_playlist_from_picker();
                Task::none()
            }
            Msg::CloseAddTo => {
                self.add_to = None;
                self.devices_open = false;
                Task::none()
            }
            Msg::Transfer(id) => {
                self.devices_open = false;
                if matches!(&self.local, LocalPlayback::Ready { device_id } if *device_id == id)
                    || id == "local"
                {
                    if matches!(self.local, LocalPlayback::Ready { .. }) {
                        bridge::send(Cmd::Transfer {
                            device_id: id,
                            play: true,
                        });
                    } else {
                        bridge::send(Cmd::AuthorizePlayback);
                    }
                    return Task::none();
                }
                bridge::send(Cmd::Transfer {
                    device_id: id,
                    play: true,
                });
                Task::none()
            }
            Msg::ToggleDevices => {
                self.add_to = None;
                self.devices_open = !self.devices_open;
                if self.devices_open {
                    bridge::send(Cmd::RefreshDevices);
                }
                Task::none()
            }
            Msg::LoadMore => {
                bridge::send(Cmd::LoadMore);
                Task::none()
            }
            Msg::DismissError => {
                self.error = None;
                Task::none()
            }
            Msg::DismissNotice => {
                self.notice = None;
                Task::none()
            }
            Msg::WindowReady(id) => {
                self.window_id = id;
                self.snap_selected()
            }
            Msg::TitleDrag => sola_kit::drag(self.window_id),
            Msg::TitleResize(dir) => sola_kit::drag_resize(self.window_id, dir),
            Msg::TitleClose => {
                bridge::send(Cmd::Shutdown);
                iced::exit()
            }
            Msg::Tick => {
                if let Some(notice) = &self.notice
                    && notice.until <= std::time::Instant::now()
                {
                    self.notice = None;
                }
                Task::none()
            }
        }
    }

    fn navigate(&mut self, page: Page) {
        self.nav.push(page.clone(), self.search.clone());
        self.show(page);
    }

    fn go_back(&mut self) {
        if let Some(entry) = self.nav.back() {
            self.restore(entry);
        }
    }

    fn go_forward(&mut self) {
        if let Some(entry) = self.nav.forward() {
            self.restore(entry);
        }
    }

    fn restore(&mut self, entry: NavEntry) {
        if entry.page == Page::Search {
            self.search = entry.search;
        }
        self.show(entry.page);
    }

    fn persist_settings(&mut self) {
        self.settings.nav = self.nav.to_saved();
        self.settings.save(&self.dirs);
    }

    fn show(&mut self, page: Page) {
        self.add_to = None;
        self.page = page.clone();
        if page.persist_as_last() {
            self.settings.last_page = page.encode();
        }
        self.persist_settings();
        if let Some(cached) = self.page_cache.get(&page).cloned() {
            self.body = Some(cached);
        } else if let Some(cached) = self.read_page_disk(&page) {
            self.page_cache.insert(page.clone(), cached.clone());
            self.body = Some(cached);
        } else if !self.body_matches(&page) {
            self.body = None;
        }
        match &page {
            Page::Search => bridge::send(Cmd::Search(self.search.clone())),
            Page::Settings => {
                self.body = Some(PageBody::Settings);
                bridge::send(Cmd::Open(page));
            }
            other => bridge::send(Cmd::Open(other.clone())),
        }
    }

    fn body_matches(&self, page: &Page) -> bool {
        match (page, &self.body) {
            (Page::Home, Some(PageBody::Home { .. })) => true,
            (Page::MadeForYou, Some(PageBody::Playlists { .. })) => true,
            (Page::Search, Some(PageBody::Search(_))) => true,
            (Page::Settings, Some(PageBody::Settings)) => true,
            (Page::Liked, Some(PageBody::Tracks { context_uri, .. })) => {
                context_uri.as_deref() == Some("spotify:collection:tracks")
            }
            (Page::Playlist(id), Some(PageBody::Tracks { context_uri, .. })) => context_uri
                .as_deref()
                .is_some_and(|uri| uri.ends_with(id.as_str()) || uri.contains(&format!(":{id}"))),
            (Page::Album(id), Some(PageBody::Tracks { context_uri, .. })) => context_uri
                .as_deref()
                .is_some_and(|uri| uri.contains(&format!("album:{id}"))),
            (Page::Artist(id), Some(PageBody::Artist { artist, .. })) => artist.id == *id,
            (Page::Albums, Some(PageBody::Albums { .. })) => true,
            (Page::Artists, Some(PageBody::Artists { .. })) => true,
            (Page::Queue, Some(PageBody::Queue { .. })) => true,
            _ => false,
        }
    }

    fn current_tracks(&self) -> &[crate::api::models::Track] {
        match &self.body {
            Some(PageBody::Tracks { items, .. }) if self.body_matches(&self.page) => items,
            Some(PageBody::Home { recent, .. }) if self.page == Page::Home => recent,
            Some(PageBody::Artist { top, .. }) => top,
            Some(PageBody::Search(results)) => results
                .tracks
                .as_ref()
                .map(|p| p.items.as_slice())
                .unwrap_or(&[]),
            _ => &[],
        }
    }

    fn current_track_uris(&self) -> Vec<String> {
        self.current_tracks()
            .iter()
            .filter(|t| !t.uri.is_empty())
            .map(|t| t.uri.clone())
            .collect()
    }

    fn fill_now_links(&self, now: &mut NowPlaying) {
        if now.uri.is_empty() {
            return;
        }
        let same = same_track_uri(&now.uri, &self.now.uri);
        if now.artist_links.is_empty() {
            if same && !self.now.artist_links.is_empty() {
                now.artist_links = self.now.artist_links.clone();
            } else if let Some(track) = self
                .current_tracks()
                .iter()
                .find(|t| same_track_uri(&t.uri, &now.uri))
            {
                now.artist_links = track.artist_links();
            }
        }
        if now.album_id.is_none() {
            if same {
                now.album_id = self.now.album_id.clone();
                if now.album.is_empty() {
                    now.album = self.now.album.clone();
                }
            } else if let Some(track) = self
                .current_tracks()
                .iter()
                .find(|t| same_track_uri(&t.uri, &now.uri))
            {
                now.album_id = track.album_catalog_id().map(str::to_string);
                if now.album.is_empty() {
                    now.album = track
                        .album
                        .as_ref()
                        .map(|a| a.name.clone())
                        .unwrap_or_default();
                }
            }
        }
    }

    fn play_now(&mut self, uri: &str, context: Option<String>) {
        let snapshot = self
            .current_tracks()
            .iter()
            .find(|t| t.uri == uri)
            .map(|t| {
                (
                    t.name.clone(),
                    t.artist_names(),
                    t.album.as_ref().map(|a| a.name.clone()).unwrap_or_default(),
                    t.artist_links(),
                    t.album_catalog_id().map(str::to_string),
                    t.image(300).map(str::to_string),
                    t.duration_ms,
                    t.uri.clone(),
                )
            });
        if let Some((
            title,
            artists,
            album,
            artist_links,
            album_id,
            art_url,
            duration_ms,
            track_uri,
        )) = snapshot
        {
            self.remember_track(&track_uri);
            self.pending_uri = Some(track_uri.clone());
            self.now.title = title;
            self.now.artists = artists;
            self.now.album = album;
            self.now.artist_links = artist_links;
            self.now.album_id = album_id;
            self.now.uri = track_uri;
            self.now.art_url = art_url.clone();
            self.now.duration_ms = duration_ms;
            self.now.position_ms = 0;
            self.now.playback = Playback::Loading;
            self.now.is_local = true;
            self.playing_since = None;
            if let Some(url) = &art_url
                && let Some(handle) = self.art.get(url)
            {
                self.player_art_hold = Some(handle.clone());
            }
            self.want_art(art_url.as_deref());
        }
        let uris = self.current_track_uris();
        let mut request = if uris.len() >= 2 {
            ApiPlay {
                context_uri: context.clone(),
                uris,
                offset_uri: Some(uri.to_string()),
                offset_position: None,
                position_ms: 0,
            }
        } else if let Some(ctx) = context {
            ApiPlay::context(ctx).starting_at_uri(uri.to_string())
        } else {
            ApiPlay::tracks(vec![uri.to_string()])
        };
        if request.offset_uri.is_none() && !uri.is_empty() {
            request.offset_uri = Some(uri.to_string());
        }
        bridge::send(Cmd::Play {
            request,
            device_id: None,
        });
    }

    fn transport_toggle(&self) {
        bridge::send(Cmd::Media(crate::media::MediaCommand::PlayPause));
    }

    fn now_saved(&self) -> bool {
        self.is_saved(&self.now.uri)
    }

    fn is_saved(&self, uri: &str) -> bool {
        if uri.is_empty() {
            return false;
        }
        if self.saved.get(uri).copied().unwrap_or(false) {
            return true;
        }
        let Some(id) = uri.rsplit(':').next() else {
            return false;
        };
        self.saved.get(id).copied().unwrap_or(false)
            || self
                .saved
                .get(&format!("spotify:track:{id}"))
                .copied()
                .unwrap_or(false)
    }

    fn toggle_saved(&mut self, uri: &str) {
        if uri.is_empty() {
            return;
        }
        let saved = !self.is_saved(uri);
        self.remember_saved(uri, saved);
        bridge::send(Cmd::SetSaved {
            uri: uri.to_string(),
            saved,
        });
    }

    fn remember_favorite(&mut self, kind: FavoriteKind, id: &str, saved: bool) {
        if id.is_empty() {
            return;
        }
        let set = match kind {
            FavoriteKind::Album => &mut self.library_albums,
            FavoriteKind::Artist => &mut self.library_artists,
        };
        if saved {
            set.insert(id.to_string());
        } else {
            set.remove(id);
        }
    }

    fn toggle_album(&mut self) {
        let Some(PageBody::Tracks {
            album: Some(album), ..
        }) = &self.body
        else {
            return;
        };
        let Some(id) = album.catalog_id().map(str::to_string) else {
            return;
        };
        let album = album.clone();
        let saved = !self.library_albums.contains(&id);
        self.remember_favorite(FavoriteKind::Album, &id, saved);
        self.patch_album_library(&album, saved);
        bridge::send(Cmd::SetFavorite {
            kind: FavoriteKind::Album,
            id,
            saved,
        });
    }

    fn toggle_artist(&mut self) {
        let artist = if let Some(PageBody::Artist { artist, .. }) = &self.body {
            Some(artist.clone())
        } else if let Some(PageBody::Tracks {
            album: Some(album), ..
        }) = &self.body
        {
            album.artists.iter().find_map(artist_from_ref)
        } else {
            None
        };
        let Some(artist) = artist else {
            return;
        };
        if artist.id.is_empty() {
            return;
        }
        let saved = !self.library_artists.contains(&artist.id);
        self.remember_favorite(FavoriteKind::Artist, &artist.id, saved);
        self.patch_artist_library(&artist, saved);
        bridge::send(Cmd::SetFavorite {
            kind: FavoriteKind::Artist,
            id: artist.id,
            saved,
        });
    }

    fn patch_album_library(&mut self, album: &Album, saved: bool) {
        let Some(id) = album.catalog_id().map(str::to_string) else {
            return;
        };
        let apply = |items: &mut Vec<Album>, total: &mut u32| {
            if saved {
                if !items
                    .iter()
                    .any(|item| item.catalog_id() == Some(id.as_str()))
                {
                    let mut entry = album.clone();
                    entry.tracks = None;
                    items.insert(0, entry);
                    *total = total.saturating_add(1);
                }
            } else if let Some(index) = items
                .iter()
                .position(|item| item.catalog_id() == Some(id.as_str()))
            {
                items.remove(index);
                *total = total.saturating_sub(1);
            }
        };
        if let Some(PageBody::Albums { items, total, .. }) = self.page_cache.get_mut(&Page::Albums)
        {
            apply(items, total);
        }
        if self.page == Page::Albums
            && let Some(PageBody::Albums { items, total, .. }) = &mut self.body
        {
            apply(items, total);
        }
        if let Some(body) = self.page_cache.get(&Page::Albums) {
            self.write_page_disk(&Page::Albums, body);
        }
    }

    fn patch_artist_library(&mut self, artist: &Artist, saved: bool) {
        if artist.id.is_empty() {
            return;
        }
        let id = artist.id.as_str();
        let apply = |items: &mut Vec<Artist>| {
            if saved {
                if !items.iter().any(|item| item.id == id) {
                    items.insert(0, artist.clone());
                }
            } else if let Some(index) = items.iter().position(|item| item.id == id) {
                items.remove(index);
            }
        };
        if let Some(PageBody::Artists { items }) = self.page_cache.get_mut(&Page::Artists) {
            apply(items);
        }
        if self.page == Page::Artists
            && let Some(PageBody::Artists { items }) = &mut self.body
        {
            apply(items);
        }
        if let Some(body) = self.page_cache.get(&Page::Artists) {
            self.write_page_disk(&Page::Artists, body);
        }
    }

    fn toggle_add_to(&mut self, uri: String) -> Task<Msg> {
        if uri.is_empty() {
            return Task::none();
        }
        self.devices_open = false;
        if self.add_to.as_ref().is_some_and(|open| open.uri == uri) {
            self.add_to = None;
            return Task::none();
        }
        self.add_to = Some(AddTo {
            uri,
            query: String::new(),
        });
        operation::focus(add_filter_id())
    }

    fn ranked_add_playlists(&self) -> Vec<&Playlist> {
        let query = self
            .add_to
            .as_ref()
            .map(|open| open.query.as_str())
            .unwrap_or("");
        playlists_for_add(
            &self.playlists,
            self.user.as_ref().map(|u| u.id.as_str()),
            query,
            Some(self.settings.last_playlist.as_str()),
        )
    }

    fn submit_add_to(&mut self) {
        if self.add_to.is_none() {
            return;
        }
        if let Some(playlist) = self.ranked_add_playlists().first() {
            let id = playlist.id.clone();
            let name = playlist.name.clone();
            self.commit_add_to(id, name);
            return;
        }
        if self
            .add_to
            .as_ref()
            .is_some_and(|open| open.query.trim().is_empty())
        {
            self.create_playlist_from_picker();
        }
    }

    fn create_playlist_from_picker(&mut self) {
        let Some(open) = self.add_to.take() else {
            return;
        };
        if open.uri.is_empty() {
            return;
        }
        self.show_notice("Added to New playlist");
        bridge::send(Cmd::CreatePlaylist {
            name: "New playlist".into(),
            uris: vec![open.uri],
        });
    }

    fn commit_add_to(&mut self, playlist_id: String, name: String) {
        let Some(open) = self.add_to.take() else {
            return;
        };
        if open.uri.is_empty() || playlist_id.is_empty() {
            return;
        }
        self.apply_added(&playlist_id, &name, &[open.uri.clone()]);
        bridge::send(Cmd::AddToPlaylist {
            playlist_id,
            name,
            uris: vec![open.uri],
        });
    }

    fn apply_added(&mut self, playlist_id: &str, name: &str, uris: &[String]) {
        if playlist_id.is_empty() {
            return;
        }
        self.settings.last_playlist = playlist_id.to_string();
        self.persist_settings();
        self.show_notice(format!("Added to {name}"));
        let n = uris.len() as u32;
        if let Some(playlist) = self.playlists.iter_mut().find(|p| p.id == playlist_id) {
            playlist.bump_track_total(n);
        }
        if self.page == Page::Playlist(playlist_id.to_string()) {
            let snapshots: Vec<Track> = uris
                .iter()
                .filter_map(|uri| self.track_snapshot(uri))
                .collect();
            if let Some(PageBody::Tracks { items, total, .. }) = &mut self.body {
                for track in snapshots {
                    items.push(track);
                    *total = total.saturating_add(1);
                }
            }
        }
    }

    fn finish_added(&mut self, playlist_id: &str, name: &str, uris: &[String]) {
        if self.settings.last_playlist != playlist_id {
            self.settings.last_playlist = playlist_id.to_string();
            self.persist_settings();
        }
        if self.notice.as_ref().is_none_or(|n| !n.text.contains(name)) {
            self.show_notice(format!("Added to {name}"));
        }
        let n = uris.len() as u32;
        if let Some(playlist) = self.playlists.iter_mut().find(|p| p.id == playlist_id) {
            // create path: the new list may already have the count from a Playlists event
            if playlist.track_total() == 0 {
                playlist.bump_track_total(n);
            }
        }
    }

    fn track_snapshot(&self, uri: &str) -> Option<Track> {
        if let Some(track) = self
            .current_tracks()
            .iter()
            .find(|t| same_track_uri(&t.uri, uri))
        {
            return Some(track.clone());
        }
        if same_track_uri(&self.now.uri, uri) && !self.now.title.is_empty() {
            return Some(Track {
                name: self.now.title.clone(),
                uri: self.now.uri.clone(),
                duration_ms: self.now.duration_ms,
                artists: vec![crate::api::models::ArtistRef {
                    name: self.now.artists.clone(),
                    ..Default::default()
                }],
                album: (!self.now.album.is_empty()).then(|| crate::api::models::Album {
                    name: self.now.album.clone(),
                    images: self
                        .now
                        .art_url
                        .as_ref()
                        .map(|url| {
                            vec![crate::api::models::Image {
                                url: url.clone(),
                                width: None,
                                height: None,
                            }]
                        })
                        .unwrap_or_default(),
                    ..Default::default()
                }),
                ..Track::default()
            });
        }
        None
    }

    fn show_notice(&mut self, text: impl Into<String>) {
        self.error = None;
        self.notice = Some(Notice {
            text: text.into(),
            until: std::time::Instant::now() + Duration::from_secs(4),
        });
    }

    fn toggle_skip(&mut self, uri: String) {
        let skipped = self.skipped.toggle(uri.clone());
        self.skipped.save(&self.dirs);
        bridge::send(Cmd::SetSkipped {
            uri: uri.clone(),
            skipped,
        });
        if skipped
            && same_track_uri(&self.now.uri, &uri)
            && matches!(self.now.playback, Playback::Playing | Playback::Loading)
        {
            self.pending_uri = None;
        }
    }

    fn on_bus(&mut self, message: &Message) -> Task<Msg> {
        self.float.update(message);
        apply_theme_update(message, &mut self.theme);
        if is_self_quit(message, APP_ID) {
            bridge::send(Cmd::Shutdown);
            return iced::exit();
        }
        if let Some(Topic::MenuAction(p)) = Topic::parse(message)
            && p.app_id == APP_ID
        {
            return self.on_menu(&p.action_id);
        }
        Task::none()
    }

    fn on_menu(&mut self, action: &str) -> Task<Msg> {
        match action {
            "sign_in" => self.update(Msg::SignIn),
            "play_here" => self.update(Msg::PlayHere),
            "quit" => {
                bridge::send(Cmd::Shutdown);
                iced::exit()
            }
            "play_pause" => self.update(Msg::Toggle),
            "next" => self.update(Msg::Next),
            "prev" => self.update(Msg::Prev),
            "shuffle" => self.update(Msg::Shuffle),
            "repeat" => self.update(Msg::Repeat),
            "back" => self.update(Msg::Back),
            "forward" => self.update(Msg::Forward),
            "home" => self.update(Msg::Open(Page::Home)),
            "search" => self.update(Msg::Open(Page::Search)),
            "liked" => self.update(Msg::Open(Page::Liked)),
            "queue" => self.update(Msg::Open(Page::Queue)),
            "settings" => self.update(Msg::Open(Page::Settings)),
            _ => Task::none(),
        }
    }

    fn on_worker(&mut self, ev: Event) -> Task<Msg> {
        match ev {
            Event::Auth(status) => {
                let just_connected = matches!(status, AuthStatus::Connected { .. })
                    && !matches!(self.auth, AuthStatus::Connected { .. });
                self.auth = status;
                if just_connected && !self.page.persist_as_last() {
                    self.show(self.page.clone());
                }
            }
            Event::User(user) => self.user = Some(user),
            Event::Premium(p) => self.premium = p,
            Event::LocalPlayback(local) => self.local = local,
            Event::Page { page, body } => {
                match &body {
                    PageBody::Home { playlists, .. }
                    | PageBody::Playlists {
                        items: playlists, ..
                    } => {
                        self.playlists = playlists.clone();
                    }
                    PageBody::Albums { items, .. } => {
                        for album in items {
                            if let Some(id) = album.catalog_id() {
                                self.library_albums.insert(id.to_string());
                            }
                        }
                    }
                    PageBody::Artists { items } => {
                        for artist in items {
                            if !artist.id.is_empty() {
                                self.library_artists.insert(artist.id.clone());
                            }
                        }
                    }
                    _ => {}
                }
                self.want_page_art(&body);
                let for_us = page == self.page;
                match (&mut self.body, &body) {
                    (
                        Some(PageBody::Tracks {
                            items,
                            total,
                            offset,
                            ..
                        }),
                        PageBody::Tracks {
                            items: more,
                            offset: more_off,
                            total: more_total,
                            ..
                        },
                    ) if for_us && *more_off > 0 => {
                        items.extend(more.iter().cloned());
                        *total = *more_total;
                        *offset = *more_off;
                        if let Some(body) = self.body.clone() {
                            self.page_cache.insert(page.clone(), body.clone());
                            self.write_page_disk(&page, &body);
                        }
                    }
                    _ => {
                        self.page_cache.insert(page.clone(), body.clone());
                        self.write_page_disk(&page, &body);
                        if for_us {
                            self.body = Some(body);
                            return self.snap_selected();
                        }
                    }
                }
            }
            Event::NowPlaying(mut now) => {
                if now.uri.is_empty() && !self.now.uri.is_empty() {
                    return Task::none();
                }
                if let Some(pending) = &self.pending_uri {
                    if !now.uri.is_empty() && !same_track_uri(&now.uri, pending) {
                        return Task::none();
                    }
                    if now.playback == Playback::Playing && same_track_uri(&now.uri, pending) {
                        self.pending_uri = None;
                    }
                }
                self.fill_now_links(&mut now);
                self.want_art(now.art_url.as_deref());
                if now.playback == Playback::Playing {
                    self.playing_since = Some(std::time::Instant::now());
                } else if now.playback != Playback::Loading {
                    self.playing_since = None;
                }
                if let Some(saved) = now.saved {
                    self.saved.insert(now.uri.clone(), saved);
                }
                if !now.uri.is_empty() {
                    self.remember_track(&now.uri);
                }
                self.now = now;
            }
            Event::Devices(devices) => self.devices = devices,
            Event::Art { url, bytes } => {
                self.art_inflight.remove(&url);
                let handle = ImageHandle::from_bytes(bytes.to_vec());
                if self.now.art_url.as_deref() == Some(url.as_str()) {
                    self.player_art_hold = Some(handle.clone());
                }
                self.art.insert(url, handle);
            }
            Event::Saved { uri, saved } => {
                self.remember_saved(&uri, saved);
            }
            Event::Favorite { kind, id, saved } => {
                self.remember_favorite(kind, &id, saved);
            }
            Event::Liked(uris) => {
                self.saved.retain(|_, saved| !*saved);
                for uri in uris {
                    self.remember_saved(&uri, true);
                }
            }
            Event::Playlists(items) => {
                self.playlists = items.clone();
                match &mut self.body {
                    Some(PageBody::Home { playlists, .. }) => *playlists = items,
                    Some(PageBody::Playlists { items: dest, .. }) => *dest = items,
                    _ => {}
                }
            }
            Event::AddedToPlaylist {
                playlist_id,
                name,
                uris,
            } => {
                self.finish_added(&playlist_id, &name, &uris);
            }
            Event::Settings(mut settings) => {
                if settings.last_track.is_empty() {
                    settings.last_track = self.settings.last_track.clone();
                }
                if settings.last_page.is_empty() {
                    settings.last_page = self.settings.last_page.clone();
                }
                if settings.last_playlist.is_empty() {
                    settings.last_playlist = self.settings.last_playlist.clone();
                }
                if !self.settings.nav.entries.is_empty() {
                    settings.nav = self.settings.nav.clone();
                }
                self.settings = settings;
            }
            Event::Error(err) => {
                self.notice = None;
                self.error = Some(err);
            }
            Event::Raise => {
                if let Some(id) = self.window_id {
                    return iced::window::gain_focus(id);
                }
            }
            Event::Quit => {
                bridge::send(Cmd::Shutdown);
                return iced::exit();
            }
        }
        Task::none()
    }

    fn want_art(&mut self, url: Option<&str>) {
        let Some(url) = url.filter(|u| !u.is_empty()) else {
            return;
        };
        if self.art.contains_key(url) || self.art_inflight.contains(url) {
            return;
        }
        self.art_inflight.insert(url.to_string());
        bridge::send(Cmd::FetchArt(url.to_string()));
    }

    fn want_page_art(&mut self, body: &PageBody) {
        match body {
            PageBody::Home {
                recent,
                playlists,
                top_artists,
                top_tracks,
            } => {
                for t in recent.iter().chain(top_tracks.iter()) {
                    self.want_art(t.image(200));
                }
                for p in playlists {
                    self.want_art(pick_image(&p.images, 200));
                }
                for a in top_artists {
                    self.want_art(pick_image(&a.images, 200));
                }
            }
            PageBody::Search(results) => {
                if let Some(page) = &results.tracks {
                    for t in &page.items {
                        self.want_art(t.image(200));
                    }
                }
                if let Some(page) = &results.albums {
                    for a in &page.items {
                        self.want_art(pick_image(&a.images, 200));
                    }
                }
                if let Some(page) = &results.artists {
                    for a in &page.items {
                        self.want_art(pick_image(&a.images, 200));
                    }
                }
                if let Some(page) = &results.playlists {
                    for p in &page.items {
                        self.want_art(pick_image(&p.images, 200));
                    }
                }
            }
            PageBody::Tracks { art, items, .. } => {
                self.want_art(art.as_deref());
                for t in items {
                    self.want_art(t.image(64));
                }
            }
            PageBody::Albums { items, .. } => {
                for a in items {
                    self.want_art(pick_image(&a.images, 200));
                }
            }
            PageBody::Artists { items } => {
                for a in items {
                    self.want_art(pick_image(&a.images, 200));
                }
            }
            PageBody::Artist {
                artist,
                top,
                albums,
            } => {
                self.want_art(pick_image(&artist.images, 300));
                for t in top {
                    self.want_art(t.image(64));
                }
                for a in albums {
                    self.want_art(pick_image(&a.images, 200));
                }
            }
            PageBody::Queue { items } => {
                for i in items {
                    self.want_art(i.image(64));
                }
            }
            PageBody::Playlists { items, .. } => {
                for p in items {
                    self.want_art(pick_image(&p.images, 200));
                }
            }
            PageBody::Settings => {}
        }
    }

    fn position_now(&self) -> u32 {
        let base = self.now.position_ms;
        if self.now.playback == Playback::Playing
            && let Some(at) = self.playing_since
        {
            let extra = at.elapsed().as_millis() as u32;
            return base
                .saturating_add(extra)
                .min(self.now.duration_ms.max(base));
        }
        base
    }

    pub fn view(&self) -> Element<'_, Msg> {
        let body: Element<'_, Msg> = match &self.auth {
            AuthStatus::SignedOut
            | AuthStatus::Failed(_)
            | AuthStatus::WaitingForBrowser { .. } => self.view_login(),
            AuthStatus::Starting | AuthStatus::Connecting => self.view_connecting(),
            AuthStatus::Connected { .. } => self.view_app(),
        };
        let body = if let Some(err) = &self.error {
            column![
                container(
                    row![
                        kit_text::caption(err.clone()).style(kit_text::danger),
                        Space::new().width(Length::Fill),
                        kit_btn::labeled_sm("Dismiss", kit_btn::ghost).on_press(Msg::DismissError),
                    ]
                    .spacing(SPACE_MD)
                    .align_y(Alignment::Center),
                )
                .padding(Padding::from([SPACE_SM, SPACE_LG]))
                .width(Length::Fill)
                .style(error_bar_style),
                body,
            ]
            .height(Length::Fill)
            .into()
        } else if let Some(notice) = &self.notice {
            column![
                container(
                    row![
                        kit_text::caption(notice.text.clone()).style(kit_text::success),
                        Space::new().width(Length::Fill),
                        kit_btn::labeled_sm("Dismiss", kit_btn::ghost).on_press(Msg::DismissNotice),
                    ]
                    .spacing(SPACE_MD)
                    .align_y(Alignment::Center),
                )
                .padding(Padding::from([SPACE_SM, SPACE_LG]))
                .width(Length::Fill)
                .style(notice_bar_style),
                body,
            ]
            .height(Length::Fill)
            .into()
        } else {
            body
        };
        sola_kit::wrap_if_floating(
            self.float.is_floating_any(),
            "Spotify",
            Msg::TitleDrag,
            Msg::TitleClose,
            Msg::TitleResize,
            body,
        )
    }

    fn view_connecting(&self) -> Element<'_, Msg> {
        container(
            column![
                kit_text::heading("Spotify"),
                kit_text::body("Signing in…").style(kit_text::muted),
            ]
            .spacing(SPACE_LG)
            .align_x(Alignment::Center),
        )
        .width(Length::Fill)
        .height(Length::Fill)
        .center_x(Length::Fill)
        .center_y(Length::Fill)
        .style(canvas_style)
        .into()
    }

    fn view_login(&self) -> Element<'_, Msg> {
        let status = match &self.auth {
            AuthStatus::WaitingForBrowser { .. } => {
                "A browser tab is open. Approve Sola, then come back here."
            }
            AuthStatus::Failed(err) => err.as_str(),
            _ => {
                "Sign in with Spotify to browse your library. Playing on this computer needs Premium."
            }
        };
        let action = match &self.auth {
            AuthStatus::WaitingForBrowser { .. } => {
                kit_btn::labeled("Cancel", kit_btn::secondary).on_press(Msg::CancelSignIn)
            }
            _ => kit_btn::labeled("Sign in with Spotify", kit_btn::primary).on_press(Msg::SignIn),
        };
        container(
            column![
                icon_svg(self.icons.disc.clone(), 36),
                kit_text::heading("Spotify"),
                kit_text::body(status.to_string()).style(kit_text::muted),
                action,
            ]
            .spacing(SPACE_LG)
            .max_width(420)
            .align_x(Alignment::Center),
        )
        .width(Length::Fill)
        .height(Length::Fill)
        .center_x(Length::Fill)
        .center_y(Length::Fill)
        .padding(SPACE_XL)
        .style(canvas_style)
        .into()
    }

    fn view_app(&self) -> Element<'_, Msg> {
        let content = row![
            self.view_library(),
            v_hairline(),
            column![self.view_nav(), self.view_page(), self.view_player()]
                .width(Length::Fill)
                .height(Length::Fill),
        ]
        .height(Length::Fill);

        container(content)
            .width(Length::Fill)
            .height(Length::Fill)
            .style(canvas_style)
            .into()
    }

    fn view_library(&self) -> Element<'_, Msg> {
        let mut browse = vec![
            SidebarItem::new("Home", Msg::Open(Page::Home)).active(self.page == Page::Home),
            SidebarItem::new("Search", Msg::Open(Page::Search))
                .active(matches!(self.page, Page::Search)),
            SidebarItem::new("Liked Songs", Msg::Open(Page::Liked))
                .active(self.page == Page::Liked),
            SidebarItem::new("Made for you", Msg::Open(Page::MadeForYou))
                .active(self.page == Page::MadeForYou),
            SidebarItem::new("Albums", Msg::Open(Page::Albums)).active(self.page == Page::Albums),
            SidebarItem::new("Artists", Msg::Open(Page::Artists))
                .active(self.page == Page::Artists),
            SidebarItem::new("Queue", Msg::Open(Page::Queue)).active(self.page == Page::Queue),
        ];
        let _ = (
            &self.icons.house,
            &self.icons.search,
            &self.icons.heart,
            &self.icons.disc,
            &self.icons.users,
            &self.icons.list,
        );
        browse.push(
            SidebarItem::new("Settings", Msg::Open(Page::Settings))
                .active(self.page == Page::Settings),
        );

        let mut sections = vec![SidebarSection::new("Library", browse)];
        let (_, yours) = split_playlists(&self.playlists);
        if !yours.is_empty() {
            let items: Vec<_> = yours
                .iter()
                .map(|p| {
                    SidebarItem::new(p.name.clone(), Msg::OpenPlaylist(p.id.clone()))
                        .active(self.page == Page::Playlist(p.id.clone()))
                })
                .collect();
            sections.push(SidebarSection::new("Playlists", items).fill());
        }
        container(sidebar(sections))
            .width(Length::Fixed(SIDEBAR_W))
            .height(Length::Fill)
            .style(chrome_style)
            .into()
    }

    fn view_nav(&self) -> Element<'_, Msg> {
        container(
            row![
                toolbar_icon_tip(
                    self.icons.back.clone(),
                    "Back",
                    self.nav.can_back().then_some(Msg::Back),
                ),
                toolbar_icon_tip(
                    self.icons.forward.clone(),
                    "Forward",
                    self.nav.can_forward().then_some(Msg::Forward),
                ),
            ]
            .spacing(SPACE_XS)
            .align_y(Alignment::Center),
        )
        .padding(Padding {
            top: SPACE_LG,
            right: SPACE_XL,
            bottom: SPACE_SM,
            left: SPACE_XL,
        })
        .width(Length::Fill)
        .into()
    }

    fn view_page(&self) -> Element<'_, Msg> {
        let inner: Element<'_, Msg> = if self.body_matches(&self.page) {
            match &self.page {
                Page::Home => self.view_home(),
                Page::MadeForYou => self.view_made_for_you(),
                Page::Search => self.view_search(),
                Page::Liked | Page::Playlist(_) | Page::Album(_) => self.view_tracks(),
                Page::Albums => self.view_albums(),
                Page::Artists => self.view_artists(),
                Page::Artist(_) => self.view_artist(),
                Page::Queue => self.view_queue(),
                Page::Settings => self.view_settings(),
            }
        } else {
            match &self.page {
                Page::Search => self.view_search(),
                Page::Settings => self.view_settings(),
                Page::Home => self.view_ghost("Home", "Loading your library…", None),
                Page::MadeForYou => self.view_ghost("Made for you", "Loading mixes…", None),
                Page::Liked => self.view_ghost("Liked Songs", "Loading…", None),
                Page::Albums => self.view_ghost("Albums", "Loading…", None),
                Page::Artists => self.view_ghost("Artists", "Loading…", None),
                Page::Queue => self.view_ghost("Queue", "Loading…", None),
                Page::Playlist(id) => {
                    let pl = self.playlists.iter().find(|p| p.id == *id);
                    self.view_ghost(
                        pl.map(|p| p.name.as_str()).unwrap_or("Playlist"),
                        pl.map(|p| p.owner_name()).unwrap_or("Loading…"),
                        pl.and_then(|p| pick_image(&p.images, 300)),
                    )
                }
                Page::Album(_) => self.view_ghost("Album", "Loading…", None),
                Page::Artist(_) => self.view_ghost("Artist", "Loading…", None),
            }
        };
        container(inner)
            .width(Length::Fill)
            .height(Length::Fill)
            .into()
    }

    fn view_ghost(&self, title: &str, subtitle: &str, art: Option<&str>) -> Element<'_, Msg> {
        let mut header = row![self.cover(art, 96.0, false)]
            .spacing(SPACE_LG)
            .align_y(Alignment::Center);
        header = header.push(
            column![
                kit_text::heading(title.to_string()),
                kit_text::caption(subtitle.to_string()).style(kit_text::muted),
            ]
            .spacing(SPACE_SM),
        );
        let rows: Vec<Element<'_, Msg>> = (0..8)
            .map(|_| {
                container(
                    row![
                        container(Space::new())
                            .width(Length::Fixed(COVER_ROW))
                            .height(Length::Fixed(COVER_ROW))
                            .style(ghost_block_style),
                        column![
                            container(Space::new())
                                .width(Length::FillPortion(3))
                                .height(Length::Fixed(10.0))
                                .style(ghost_block_style),
                            container(Space::new())
                                .width(Length::FillPortion(2))
                                .height(Length::Fixed(8.0))
                                .style(ghost_block_style),
                        ]
                        .spacing(SPACE_SM)
                        .width(Length::Fill),
                    ]
                    .spacing(SPACE_MD)
                    .align_y(Alignment::Center)
                    .padding(Padding::from([SPACE_SM, SPACE_MD])),
                )
                .width(Length::Fill)
                .into()
            })
            .collect();
        scrollable(
            column![header, column(rows).spacing(SPACE_XS)]
                .spacing(SPACE_XL)
                .padding(SPACE_XL)
                .width(Length::Fill),
        )
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
    }

    fn view_home(&self) -> Element<'_, Msg> {
        let Some(PageBody::Home {
            recent,
            playlists,
            top_artists,
            top_tracks,
        }) = &self.body
        else {
            return Space::new().into();
        };
        let mut col = column![kit_text::heading("Home")].spacing(SPACE_XL);
        if !recent.is_empty() {
            col = col.push(kit_text::subheading("Recently played"));
            col = col.push(self.track_list(recent, None));
        }
        if !top_tracks.is_empty() {
            col = col.push(kit_text::subheading("Top songs"));
            col = col.push(self.track_list(top_tracks, None));
        }
        let (made, yours) = split_playlists(playlists);
        if !made.is_empty() {
            let total = made.len();
            let shelf: Vec<&Playlist> = made.iter().copied().take(MADE_SHELF).collect();
            let header: Element<'_, Msg> = if total > MADE_SHELF {
                row![
                    kit_text::subheading("Made for you").width(Length::Fill),
                    kit_btn::labeled_sm("See all", kit_btn::ghost)
                        .on_press(Msg::Open(Page::MadeForYou)),
                ]
                .align_y(Alignment::Center)
                .into()
            } else {
                kit_text::subheading("Made for you").into()
            };
            col = col.push(header);
            col = col.push(self.playlist_tiles(shelf));
        }
        if !yours.is_empty() {
            col = col.push(kit_text::subheading("Playlists"));
            col = col.push(self.playlist_tiles(yours));
        }
        if !top_artists.is_empty() {
            col = col.push(kit_text::subheading("Top artists"));
            col = col.push(self.artist_tiles(top_artists));
        }
        scrollable(col.padding(SPACE_XL).width(Length::Fill))
            .width(Length::Fill)
            .height(Length::Fill)
            .into()
    }

    fn view_made_for_you(&self) -> Element<'_, Msg> {
        let items = match &self.body {
            Some(PageBody::Playlists { items, .. }) => items.as_slice(),
            _ => self.playlists.as_slice(),
        };
        let mut made: Vec<&Playlist> = items.iter().filter(|p| p.is_generated()).collect();
        made.sort_by_key(|p| generated_sort_key(&p.name));
        let mut col = column![
            kit_text::heading("Made for you"),
            kit_text::caption(format!("{} mixes", made.len())).style(kit_text::muted),
        ]
        .spacing(SPACE_SM);
        if made.is_empty() {
            col = col.push(
                kit_text::body("No mixes yet. Follow Discover Weekly in Spotify, or play more — these appear as Spotify builds them.")
                    .style(kit_text::muted),
            );
        } else {
            col = col.push(self.playlist_grid(made));
        }
        scrollable(col.spacing(SPACE_XL).padding(SPACE_XL).width(Length::Fill))
            .width(Length::Fill)
            .height(Length::Fill)
            .into()
    }

    fn view_search(&self) -> Element<'_, Msg> {
        let field = text_input("Search songs, artists, albums…", &self.search)
            .on_input(Msg::SearchChanged)
            .on_submit(Msg::SearchSubmit);
        let mut col = column![field].spacing(SPACE_XL).padding(SPACE_XL);
        if let Some(PageBody::Search(results)) = &self.body {
            if results.is_empty() && !self.search.is_empty() {
                col = col.push(kit_text::body("No results").style(kit_text::muted));
            }
            if let Some(page) = &results.tracks
                && !page.items.is_empty()
            {
                col = col.push(kit_text::subheading("Songs"));
                col = col.push(self.track_list(&page.items, None));
            }
            if let Some(page) = &results.artists
                && !page.items.is_empty()
            {
                col = col.push(kit_text::subheading("Artists"));
                col = col.push(self.artist_tiles(&page.items));
            }
            if let Some(page) = &results.albums
                && !page.items.is_empty()
            {
                col = col.push(kit_text::subheading("Albums"));
                col = col.push(self.album_tiles(&page.items));
            }
            if let Some(page) = &results.playlists
                && !page.items.is_empty()
            {
                col = col.push(kit_text::subheading("Playlists"));
                col = col.push(self.playlist_tiles(&page.items));
            }
        }
        scrollable(col.width(Length::Fill))
            .width(Length::Fill)
            .height(Length::Fill)
            .into()
    }

    fn view_tracks(&self) -> Element<'_, Msg> {
        let Some(PageBody::Tracks {
            title,
            subtitle,
            art,
            context_uri,
            items,
            total,
            album,
            ..
        }) = &self.body
        else {
            return Space::new().into();
        };
        let play_all = context_uri.clone().map(Msg::PlayContext);
        let mut header = row![self.cover(art.as_deref(), 96.0, false)]
            .spacing(SPACE_LG)
            .align_y(Alignment::Center);
        let mut titles = column![
            kit_text::heading(title.clone()),
            kit_text::caption(subtitle.clone()).style(kit_text::muted),
        ]
        .spacing(SPACE_SM);
        let mut actions = row![].spacing(SPACE_SM).align_y(Alignment::Center);
        let mut has_actions = false;
        if let Some(msg) = play_all {
            actions = actions.push(kit_btn::labeled("Play", kit_btn::primary).on_press(msg));
            has_actions = true;
        }
        if let Some(album) = album {
            if let Some(id) = album.catalog_id() {
                actions = actions.push(self.favorite_btn(
                    self.library_albums.contains(id),
                    "Saved",
                    "Save",
                    Msg::ToggleAlbum,
                ));
                has_actions = true;
            }
            if let Some(id) = album.artists.iter().find_map(|artist| artist.catalog_id()) {
                actions = actions.push(self.favorite_btn(
                    self.library_artists.contains(id),
                    "Following",
                    "Follow",
                    Msg::ToggleArtist,
                ));
                has_actions = true;
            }
        }
        if has_actions {
            titles = titles.push(actions);
        }
        header = header.push(titles);
        let list = self.track_list(items, context_uri.clone());
        let mut col = column![header, list].spacing(SPACE_XL).padding(SPACE_XL);
        if items.len() < *total as usize {
            col = col
                .push(kit_btn::labeled_sm("Load more", kit_btn::secondary).on_press(Msg::LoadMore));
        }
        scrollable(col.width(Length::Fill))
            .id(tracks_scroll_id())
            .width(Length::Fill)
            .height(Length::Fill)
            .into()
    }

    fn view_albums(&self) -> Element<'_, Msg> {
        let Some(PageBody::Albums { items, total, .. }) = &self.body else {
            return Space::new().into();
        };
        let mut col = column![kit_text::heading("Albums"), self.album_grid(items)]
            .spacing(SPACE_XL)
            .padding(SPACE_XL);
        if items.len() < *total as usize {
            col = col
                .push(kit_btn::labeled_sm("Load more", kit_btn::secondary).on_press(Msg::LoadMore));
        }
        scrollable(col.width(Length::Fill))
            .width(Length::Fill)
            .height(Length::Fill)
            .into()
    }

    fn view_artists(&self) -> Element<'_, Msg> {
        let Some(PageBody::Artists { items }) = &self.body else {
            return Space::new().into();
        };
        scrollable(
            column![kit_text::heading("Artists"), self.artist_grid(items)]
                .spacing(SPACE_XL)
                .padding(SPACE_XL)
                .width(Length::Fill),
        )
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
    }

    fn view_artist(&self) -> Element<'_, Msg> {
        let Some(PageBody::Artist {
            artist,
            top,
            albums,
        }) = &self.body
        else {
            return Space::new().into();
        };
        let following = !artist.id.is_empty() && self.library_artists.contains(&artist.id);
        let mut heading = column![
            kit_text::heading(artist.name.clone()),
            kit_text::caption(format!(
                "{} followers",
                artist.followers.as_ref().map(|f| f.total).unwrap_or(0)
            ))
            .style(kit_text::muted),
        ]
        .spacing(SPACE_SM);
        if !artist.id.is_empty() {
            heading = heading.push(self.favorite_btn(
                following,
                "Following",
                "Follow",
                Msg::ToggleArtist,
            ));
        }
        let header = row![
            self.cover(pick_image(&artist.images, 300), 120.0, false),
            heading,
        ]
        .spacing(SPACE_LG)
        .align_y(Alignment::Center);
        let mut col = column![header].spacing(SPACE_XL).padding(SPACE_XL);
        if !top.is_empty() {
            col = col.push(kit_text::subheading("Popular"));
            col = col.push(self.track_list(top, None));
        }
        if !albums.is_empty() {
            col = col.push(kit_text::subheading("Discography"));
            col = col.push(self.album_tiles(albums));
        }
        scrollable(col.width(Length::Fill))
            .width(Length::Fill)
            .height(Length::Fill)
            .into()
    }

    fn view_queue(&self) -> Element<'_, Msg> {
        let Some(PageBody::Queue { items }) = &self.body else {
            return Space::new().into();
        };
        let rows: Vec<Element<'_, Msg>> = items
            .iter()
            .map(|item| {
                let uri = item.uri().to_string();
                let hovered = self
                    .hovered_uri
                    .as_deref()
                    .is_some_and(|h| same_track_uri(h, &uri));
                let meta: Element<'_, Msg> = match item {
                    PlayableItem::Track(track) => {
                        self.track_meta(&track.artists, track.album.as_ref(), false)
                    }
                    _ => kit_text::caption(item.subtitle())
                        .style(kit_text::muted)
                        .into(),
                };
                let play = mouse_area(
                    container(
                        row![
                            self.cover(item.image(64), COVER_ROW, false),
                            column![kit_text::body(item.name().to_string()), meta]
                                .spacing(SPACE_XS)
                                .width(Length::Fill),
                        ]
                        .spacing(SPACE_MD)
                        .align_y(Alignment::Center)
                        .width(Length::Fill),
                    )
                    .style(track_hit_style(false, hovered))
                    .width(Length::Fill),
                )
                .on_press(Msg::PlayTrack {
                    uri: uri.clone(),
                    context: None,
                })
                .on_enter(Msg::HoverTrack(uri.clone()))
                .on_exit(Msg::UnhoverTrack(uri.clone()))
                .interaction(mouse::Interaction::Pointer);
                let add = if item.is_track() {
                    self.add_mark(&uri)
                } else {
                    Space::new().into()
                };
                row![
                    play,
                    add,
                    kit_text::caption(format_ms(item.duration_ms()))
                        .style(kit_text::muted)
                        .width(Length::Fixed(40.0)),
                ]
                .spacing(SPACE_SM)
                .align_y(Alignment::Center)
                .padding(Padding::from([SPACE_SM, SPACE_MD]))
                .into()
            })
            .collect();
        scrollable(
            column![kit_text::heading("Queue"), column(rows).spacing(SPACE_XS)]
                .spacing(SPACE_XL)
                .padding(SPACE_XL)
                .width(Length::Fill),
        )
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
    }

    fn view_settings(&self) -> Element<'_, Msg> {
        let local = match &self.local {
            LocalPlayback::Ready { .. } => "This computer is a Spotify Connect device.",
            LocalPlayback::Connecting => "Connecting local playback…",
            LocalPlayback::Authorizing => "Approve playback in the browser…",
            LocalPlayback::Failed(err) => err.as_str(),
            LocalPlayback::Unavailable => "Local playback is not set up. Premium can play here.",
        };
        let play_here = if matches!(self.local, LocalPlayback::Ready { .. }) {
            kit_btn::labeled("Play here is ready", kit_btn::secondary)
        } else {
            kit_btn::labeled("Set up playback here", kit_btn::primary).on_press(Msg::PlayHere)
        };
        let user = self
            .user
            .as_ref()
            .map(|u| u.name().to_string())
            .unwrap_or_else(|| "—".into());
        let plan = match self.premium {
            Some(true) => "Premium",
            Some(false) => "Free (browse only on this computer)",
            None => "—",
        };
        scrollable(
            column![
                kit_text::heading("Settings"),
                kit_text::subheading("Account"),
                kit_text::body(format!("{user} · {plan}")),
                kit_btn::labeled_sm("Sign out", kit_btn::danger).on_press(Msg::SignOut),
                kit_text::subheading("This computer"),
                kit_text::body(local.to_string()).style(kit_text::muted),
                play_here,
                kit_text::caption(format!(
                    "Connect name “{}” · {} kbps",
                    self.settings.device_name, self.settings.bitrate_kbps
                ))
                .style(kit_text::muted),
            ]
            .spacing(SPACE_LG)
            .padding(SPACE_XL)
            .max_width(520)
            .width(Length::Fill),
        )
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
    }

    fn track_list<'a>(&'a self, tracks: &'a [Track], context: Option<String>) -> Element<'a, Msg> {
        let accent = self.theme.extended_palette().primary.base.color;
        let rows: Vec<Element<'a, Msg>> = tracks
            .iter()
            .enumerate()
            .map(|(i, track)| {
                let uri = track.uri.clone();
                let ctx = context.clone();
                let live = same_track_uri(&self.now.uri, &uri)
                    && matches!(self.now.playback, Playback::Playing | Playback::Loading);
                let selected = self
                    .selected_uri
                    .as_deref()
                    .is_some_and(|sel| same_track_uri(sel, &uri));
                let hovered = self
                    .hovered_uri
                    .as_deref()
                    .is_some_and(|h| same_track_uri(h, &uri));
                let saved = self.is_saved(&uri);
                let skipped = self.skipped.contains(&uri);
                let withdrawn = skipped && !live;
                let title: Element<'a, Msg> = if live {
                    kit_text::body(track.name.clone())
                        .style(kit_text::accent)
                        .into()
                } else if withdrawn {
                    let mute = self.theme.extended_palette().secondary.base.text;
                    rich_text![
                        span::<(), iced::Font>(track.name.clone())
                            .strikethrough(true)
                            .color(mute)
                    ]
                    .size(13)
                    .font(fonts::ui())
                    .into()
                } else {
                    kit_text::body(track.name.clone()).into()
                };
                let index: Element<'a, Msg> = if live {
                    container(icon_svg_colored(self.icons.play.clone(), 14, accent))
                        .width(Length::Fixed(24.0))
                        .height(Length::Fixed(COVER_ROW))
                        .align_x(iced::alignment::Horizontal::Center)
                        .align_y(iced::alignment::Vertical::Center)
                        .into()
                } else {
                    kit_text::caption(format!("{}", i + 1))
                        .style(kit_text::muted)
                        .width(Length::Fixed(24.0))
                        .into()
                };
                let play = mouse_area(
                    container(
                        row![
                            index,
                            self.cover(track.image(64), COVER_ROW, withdrawn),
                            column![
                                title,
                                self.track_meta(&track.artists, track.album.as_ref(), selected),
                            ]
                            .spacing(SPACE_XS)
                            .width(Length::Fill),
                        ]
                        .spacing(SPACE_MD)
                        .align_y(Alignment::Center)
                        .width(Length::Fill),
                    )
                    .style(track_hit_style(selected, hovered))
                    .width(Length::Fill),
                )
                .on_press(Msg::PlayTrack {
                    uri: uri.clone(),
                    context: ctx,
                })
                .on_enter(Msg::HoverTrack(uri.clone()))
                .on_exit(Msg::UnhoverTrack(uri.clone()))
                .interaction(mouse::Interaction::Pointer);

                let plus = self.like_mark(
                    saved,
                    Msg::SaveTrack(uri.clone()),
                    if saved { "Unlike" } else { "Like" },
                );
                let skip_tip = if skipped {
                    "Play this again"
                } else {
                    "Don't play this"
                };
                let marks = row![
                    plus,
                    self.add_mark(&uri),
                    toolbar_icon_tip(
                        self.icons.minus.clone(),
                        skip_tip,
                        Some(Msg::SkipTrack { uri: uri.clone() }),
                    ),
                ]
                .spacing(SPACE_XS)
                .align_y(Alignment::Center);
                let mut tail = row![marks].spacing(SPACE_SM).align_y(Alignment::Center);
                if let Some(added) = track.added_at.as_deref().and_then(format_added_at) {
                    tail = tail.push(
                        kit_text::caption(added)
                            .style(kit_text::muted)
                            .width(Length::Fixed(88.0)),
                    );
                }
                tail = tail.push(
                    kit_text::caption(format_ms(track.duration_ms))
                        .style(kit_text::muted)
                        .width(Length::Fixed(40.0)),
                );
                row![play, tail]
                    .spacing(SPACE_SM)
                    .align_y(Alignment::Center)
                    .padding(Padding::from([SPACE_SM, SPACE_MD]))
                    .into()
            })
            .collect();
        column(rows).spacing(1.0).width(Length::Fill).into()
    }

    fn track_meta(
        &self,
        artists: &[ArtistRef],
        album: Option<&Album>,
        selected: bool,
    ) -> Element<'_, Msg> {
        let p = self.theme.extended_palette();
        let color = if selected {
            p.background.base.text
        } else {
            p.secondary.base.text
        };
        let mut spans = Vec::new();
        for artist in artists {
            if artist.name.is_empty() {
                continue;
            }
            if !spans.is_empty() {
                spans.push(span(", ").color(color));
            }
            let mut piece = span(artist.name.clone()).color(color);
            if let Some(id) = artist.catalog_id() {
                piece = piece.link(Msg::OpenArtist(id.to_string()));
            }
            spans.push(piece);
        }
        if let Some(album) = album.filter(|a| !a.name.is_empty()) {
            if !spans.is_empty() {
                spans.push(span(" · ").color(color));
            }
            let mut piece = span(album.name.clone()).color(color);
            if let Some(id) = album.catalog_id() {
                piece = piece.link(Msg::OpenAlbum(id.to_string()));
            }
            spans.push(piece);
        }
        if spans.is_empty() {
            return Space::new().into();
        }
        rich_text(spans)
            .size(11)
            .font(fonts::ui())
            .on_link_click(|msg| msg)
            .into()
    }

    fn now_meta(&self) -> Element<'_, Msg> {
        let artists: Vec<ArtistRef> = if self.now.artist_links.is_empty() {
            if self.now.artists.is_empty() {
                Vec::new()
            } else {
                vec![ArtistRef {
                    name: self.now.artists.clone(),
                    ..ArtistRef::default()
                }]
            }
        } else {
            self.now
                .artist_links
                .iter()
                .map(|(name, id)| ArtistRef {
                    name: name.clone(),
                    id: Some(id.clone()),
                    ..ArtistRef::default()
                })
                .collect()
        };
        let album = (!self.now.album.is_empty() || self.now.album_id.is_some()).then(|| Album {
            name: self.now.album.clone(),
            id: self.now.album_id.clone().unwrap_or_default(),
            ..Album::default()
        });
        self.track_meta(&artists, album.as_ref(), false)
    }

    fn playlist_tiles<'a>(
        &'a self,
        playlists: impl IntoIterator<Item = &'a Playlist>,
    ) -> Element<'a, Msg> {
        let items = self.playlist_tile_widgets(playlists);
        scrollable(row(items).spacing(SPACE_MD))
            .direction(scrollable::Direction::Horizontal(
                scrollable::Scrollbar::new(),
            ))
            .width(Length::Fill)
            .into()
    }

    fn album_tiles<'a>(&'a self, albums: &'a [Album]) -> Element<'a, Msg> {
        self.tile_row(Self::album_tile_data(albums))
    }

    fn album_grid<'a>(&'a self, albums: &'a [Album]) -> Element<'a, Msg> {
        self.catalog_grid(Self::album_tile_data(albums))
    }

    fn album_tile_data(albums: &[Album]) -> Vec<(String, String, Option<String>, Msg)> {
        albums
            .iter()
            .map(|a| {
                (
                    a.name.clone(),
                    crate::api::models::join_names(a.artists.iter().map(|x| x.name.as_str())),
                    pick_image(&a.images, 200).map(str::to_string),
                    Msg::OpenAlbum(a.id.clone()),
                )
            })
            .collect()
    }

    fn artist_tiles<'a>(&'a self, artists: &'a [Artist]) -> Element<'a, Msg> {
        self.tile_row(Self::artist_tile_data(artists))
    }

    fn artist_grid<'a>(&'a self, artists: &'a [Artist]) -> Element<'a, Msg> {
        self.catalog_grid(Self::artist_tile_data(artists))
    }

    fn artist_tile_data(artists: &[Artist]) -> Vec<(String, String, Option<String>, Msg)> {
        artists
            .iter()
            .map(|a| {
                (
                    a.name.clone(),
                    "Artist".to_string(),
                    pick_image(&a.images, 200).map(str::to_string),
                    Msg::OpenArtist(a.id.clone()),
                )
            })
            .collect()
    }

    fn playlist_grid<'a>(&'a self, playlists: Vec<&'a Playlist>) -> Element<'a, Msg> {
        self.wrap_tiles(self.playlist_tile_widgets(playlists))
    }

    fn catalog_grid<'a>(
        &'a self,
        tiles: Vec<(String, String, Option<String>, Msg)>,
    ) -> Element<'a, Msg> {
        self.wrap_tiles(self.tile_widgets(tiles))
    }

    fn wrap_tiles<'a>(&'a self, items: Vec<Element<'a, Msg>>) -> Element<'a, Msg> {
        row(items)
            .spacing(SPACE_MD)
            .width(Length::Fill)
            .wrap()
            .vertical_spacing(SPACE_MD)
            .into()
    }

    fn tile_widgets<'a>(
        &'a self,
        tiles: Vec<(String, String, Option<String>, Msg)>,
    ) -> Vec<Element<'a, Msg>> {
        tiles
            .into_iter()
            .map(|(title, sub, art, msg)| {
                button(
                    column![
                        self.cover(art.as_deref(), COVER_TILE, false),
                        kit_text::body(title),
                        kit_text::caption(sub).style(kit_text::muted),
                    ]
                    .spacing(SPACE_SM)
                    .width(Length::Fixed(COVER_TILE)),
                )
                .style(kit_btn::list_item(false))
                .on_press(msg)
                .padding(SPACE_SM)
                .into()
            })
            .collect()
    }

    fn playlist_tile_widgets<'a>(
        &'a self,
        playlists: impl IntoIterator<Item = &'a Playlist>,
    ) -> Vec<Element<'a, Msg>> {
        playlists
            .into_iter()
            .map(|p| {
                let n = p.track_total();
                let sub = if p.is_generated() {
                    if n > 0 {
                        format!("Made for you · {n} songs")
                    } else {
                        "Made for you".into()
                    }
                } else if n > 0 {
                    format!("{} · {n} songs", p.owner_name())
                } else {
                    p.owner_name().to_string()
                };
                button(
                    column![
                        self.cover(pick_image(&p.images, 200), COVER_TILE, false),
                        kit_text::body(p.name.clone()),
                        kit_text::caption(sub).style(kit_text::muted),
                    ]
                    .spacing(SPACE_SM)
                    .width(Length::Fixed(COVER_TILE)),
                )
                .style(kit_btn::list_item(false))
                .on_press(Msg::OpenPlaylist(p.id.clone()))
                .padding(SPACE_SM)
                .into()
            })
            .collect()
    }

    fn tile_row<'a>(
        &'a self,
        tiles: Vec<(String, String, Option<String>, Msg)>,
    ) -> Element<'a, Msg> {
        let items = self.tile_widgets(tiles);
        scrollable(row(items).spacing(SPACE_MD))
            .direction(scrollable::Direction::Horizontal(
                scrollable::Scrollbar::new(),
            ))
            .width(Length::Fill)
            .into()
    }

    fn view_player(&self) -> Element<'_, Msg> {
        let playing = self.now.playback == Playback::Playing;
        let play_icon = if playing {
            self.icons.pause.clone()
        } else {
            self.icons.play.clone()
        };
        let pos = self.position_now();
        let frac = if self.now.duration_ms == 0 {
            0.0
        } else {
            pos as f32 / self.now.duration_ms as f32
        };
        let like_btn = self.like_mark(
            self.now_saved(),
            Msg::Like,
            if self.now_saved() { "Unlike" } else { "Like" },
        );
        let add_btn = self.add_mark(&self.now.uri);

        let now = if matches!(
            self.local,
            LocalPlayback::Authorizing | LocalPlayback::Connecting
        ) {
            let line = match &self.local {
                LocalPlayback::Authorizing => "Approve playback in the browser…",
                _ => "Connecting this computer…",
            };
            column![
                kit_text::body("This computer").style(kit_text::muted),
                kit_text::caption(line).style(kit_text::muted),
            ]
            .spacing(SPACE_XS)
            .width(Length::FillPortion(2))
        } else if self.now.title.is_empty() {
            let caption = match &self.local {
                LocalPlayback::Ready { .. } => "Pick a song",
                LocalPlayback::Failed(_) => "Playback needs setup",
                _ => "Play a song to set up this computer",
            };
            column![
                kit_text::body("Nothing playing"),
                kit_text::caption(caption).style(kit_text::muted),
            ]
            .spacing(SPACE_XS)
            .width(Length::FillPortion(2))
        } else {
            column![kit_text::body(self.now.title.clone()), self.now_meta(),]
                .spacing(SPACE_XS)
                .width(Length::FillPortion(2))
        };

        let transport = row![
            toolbar_icon_tip(self.icons.shuffle.clone(), "Shuffle", Some(Msg::Shuffle)),
            toolbar_icon_tip(self.icons.prev.clone(), "Previous", Some(Msg::Prev)),
            toolbar_icon_tip(play_icon, "Play/Pause", Some(Msg::Toggle)),
            toolbar_icon_tip(self.icons.next.clone(), "Next", Some(Msg::Next)),
            toolbar_icon_tip(self.icons.repeat.clone(), "Repeat", Some(Msg::Repeat)),
        ]
        .spacing(SPACE_SM)
        .align_y(Alignment::Center);

        let seek = row![
            kit_text::caption(format_ms(pos))
                .style(kit_text::muted)
                .width(Length::Fixed(36.0)),
            slider(0.0..=1.0, frac, Msg::SeekFrac).width(Length::FillPortion(3)),
            kit_text::caption(format_ms(self.now.duration_ms))
                .style(kit_text::muted)
                .width(Length::Fixed(36.0)),
        ]
        .spacing(SPACE_SM)
        .align_y(Alignment::Center)
        .width(Length::FillPortion(3));

        let devices = self.view_device_picker();

        let right = row![
            like_btn,
            add_btn,
            devices,
            icon_svg(self.icons.volume.clone(), 14),
            slider(0.0..=100.0, self.now.volume_percent as f32, |v| {
                Msg::Volume(v as u8)
            })
            .width(Length::Fixed(88.0)),
        ]
        .spacing(SPACE_MD)
        .align_y(Alignment::Center);

        let cover = self.cover(self.now.art_url.as_deref(), COVER_PLAYER, false);
        let cover: Element<'_, Msg> = match self.now.album_id.clone() {
            Some(id) if !id.is_empty() => button(cover)
                .style(kit_btn::ghost)
                .padding(0)
                .on_press(Msg::OpenAlbum(id))
                .into(),
            _ => cover,
        };
        let bar = row![
            cover,
            now,
            column![transport, seek]
                .spacing(SPACE_XS)
                .align_x(Alignment::Center)
                .width(Length::FillPortion(3)),
            right,
        ]
        .spacing(SPACE_LG)
        .align_y(Alignment::Center)
        .padding(Padding::from([SPACE_MD, SPACE_LG]))
        .height(Length::Fixed(PLAYER_H));

        container(bar)
            .width(Length::Fill)
            .style(player_style)
            .into()
    }

    fn view_device_picker(&self) -> Element<'_, Msg> {
        let device_label = if !self.now.device_name.is_empty() {
            self.now.device_name.clone()
        } else if matches!(self.local, LocalPlayback::Ready { .. }) {
            "This computer".into()
        } else {
            "This computer".into()
        };
        let trigger = button(
            row![
                icon_svg(self.icons.speaker.clone(), 14),
                kit_text::caption(device_label),
            ]
            .spacing(SPACE_SM)
            .align_y(Alignment::Center),
        )
        .style(kit_btn::ghost)
        .on_press(Msg::ToggleDevices);

        if !self.devices_open {
            return trigger.into();
        }

        let local_id = match &self.local {
            LocalPlayback::Ready { device_id } => device_id.clone(),
            _ => "local".to_string(),
        };
        let local_selected = self.now.is_local
            || self.now.device_id.as_deref() == Some(local_id.as_str())
            || (self.now.device_id.is_none() && matches!(self.local, LocalPlayback::Ready { .. }));
        let (local_caption, local_msg): (&str, Option<Msg>) = match &self.local {
            LocalPlayback::Ready { .. } => ("Ready", Some(Msg::Transfer(local_id.clone()))),
            LocalPlayback::Authorizing => ("Approve in the browser…", None),
            LocalPlayback::Connecting => ("Connecting…", None),
            LocalPlayback::Failed(err) => (err.as_str(), Some(Msg::PlayHere)),
            LocalPlayback::Unavailable => ("Play here — one-time setup", Some(Msg::PlayHere)),
        };
        let mut local_row = button(
            column![
                kit_text::body("This computer"),
                kit_text::caption(local_caption.to_string()).style(kit_text::muted),
            ]
            .spacing(SPACE_XS)
            .width(Length::Fill)
            .padding(Padding::from([SPACE_SM, SPACE_MD])),
        )
        .style(kit_btn::list_item(local_selected))
        .width(Length::Fill);
        if let Some(msg) = local_msg {
            local_row = local_row.on_press(msg);
        }

        let remote: Vec<Element<'_, Msg>> = self
            .devices
            .iter()
            .filter(|d| d.id.as_deref() != Some(local_id.as_str()))
            .filter_map(|d| {
                let id = d.id.clone()?;
                Some(
                    button(
                        column![
                            kit_text::body(d.name.clone()),
                            kit_text::caption(d.kind.clone()).style(kit_text::muted),
                        ]
                        .spacing(SPACE_XS)
                        .width(Length::Fill)
                        .padding(Padding::from([SPACE_SM, SPACE_MD])),
                    )
                    .style(kit_btn::list_item(d.is_active))
                    .on_press(Msg::Transfer(id))
                    .width(Length::Fill)
                    .into(),
                )
            })
            .collect();

        let mut list = column![
            kit_text::caption("Play on").style(kit_text::muted),
            local_row,
        ]
        .spacing(SPACE_SM)
        .padding(SPACE_MD)
        .width(Length::Fixed(260.0));
        if !remote.is_empty() {
            list = list.push(kit_text::caption("Other devices").style(kit_text::muted));
            list = list.push(column(remote).spacing(SPACE_XS).width(Length::Fill));
        }

        popover_anchored(trigger, popover(list), Msg::ToggleDevices)
            .placement(Placement::Below)
            .into()
    }

    fn favorite_btn(
        &self,
        on: bool,
        on_label: &'static str,
        off_label: &'static str,
        msg: Msg,
    ) -> Element<'_, Msg> {
        kit_btn::labeled_sm(
            if on { on_label } else { off_label },
            if on {
                kit_btn::ghost
            } else {
                kit_btn::secondary
            },
        )
        .on_press(msg)
        .into()
    }

    fn remember_saved(&mut self, uri: &str, saved: bool) {
        if uri.is_empty() {
            return;
        }
        self.saved.insert(uri.to_string(), saved);
        if let Some(id) = uri.rsplit(':').next()
            && id != uri
        {
            self.saved.insert(id.to_string(), saved);
            self.saved.insert(format!("spotify:track:{id}"), saved);
        }
        if same_track_uri(&self.now.uri, uri) {
            self.now.saved = Some(saved);
        }
    }

    fn add_mark(&self, uri: &str) -> Element<'_, Msg> {
        let open = self.add_to.as_ref().is_some_and(|state| state.uri == uri);
        let p = self.theme.extended_palette();
        let color = if open {
            p.primary.base.color
        } else {
            p.secondary.base.text
        };
        let icon = icon_svg_colored(self.icons.list_plus.clone(), 16, color);
        let mut btn = button(icon)
            .padding(sola_kit::components::style::PAD_CONTROL_SM)
            .style(kit_btn::ghost);
        if !uri.is_empty() {
            btn = btn.on_press(Msg::ToggleAddTo(uri.to_string()));
        }
        let trigger: Element<'_, Msg> = if uri.is_empty() {
            btn.into()
        } else {
            let tip = container(kit_text::caption("Add to playlist")).padding(Padding {
                top: 5.0,
                right: 8.0,
                bottom: 5.0,
                left: 8.0,
            });
            iced::widget::tooltip(btn, tip, iced::widget::tooltip::Position::Bottom)
                .gap(6)
                .into()
        };
        if !open {
            return trigger;
        }
        popover_anchored(trigger, self.view_add_picker(), Msg::CloseAddTo)
            .placement(Placement::Below)
            .into()
    }

    fn view_add_picker(&self) -> Element<'_, Msg> {
        let query = self
            .add_to
            .as_ref()
            .map(|state| state.query.as_str())
            .unwrap_or("");
        let field = text_input("Find a playlist", query)
            .id(add_filter_id())
            .on_input(Msg::AddToFilter)
            .on_submit(Msg::AddToSubmit)
            .width(Length::Fill);
        let ranked = self.ranked_add_playlists();
        let needle = query.trim().to_lowercase();
        let show_new = needle.is_empty() || "new playlist".contains(&needle);
        let new_row = button(
            column![
                kit_text::body("New playlist"),
                kit_text::caption("Create and add").style(kit_text::muted),
            ]
            .spacing(SPACE_XS)
            .width(Length::Fill)
            .padding(Padding::from([SPACE_SM, SPACE_MD])),
        )
        .style(kit_btn::list_item(false))
        .on_press(Msg::CreatePlaylist)
        .width(Length::Fill);
        let count = ranked.len();
        let rows: Vec<Element<'_, Msg>> = ranked
            .into_iter()
            .map(|playlist| {
                let n = playlist.track_total();
                let sub = if n == 1 {
                    "1 song".to_string()
                } else {
                    format!("{n} songs")
                };
                button(
                    column![
                        kit_text::body(playlist.name.clone()),
                        kit_text::caption(sub).style(kit_text::muted),
                    ]
                    .spacing(SPACE_XS)
                    .width(Length::Fill)
                    .padding(Padding::from([SPACE_SM, SPACE_MD])),
                )
                .style(kit_btn::list_item(false))
                .on_press(Msg::AddToPlaylist {
                    id: playlist.id.clone(),
                    name: playlist.name.clone(),
                })
                .width(Length::Fill)
                .into()
            })
            .collect();
        let mut list = column![field]
            .spacing(SPACE_SM)
            .padding(SPACE_MD)
            .width(Length::Fixed(260.0));
        if show_new {
            list = list.push(new_row);
        }
        if rows.is_empty() && !show_new {
            list = list.push(kit_text::caption("No matching playlists").style(kit_text::muted));
        } else if !rows.is_empty() {
            let height = (count as f32 * 48.0).clamp(48.0, 240.0);
            list = list.push(
                scrollable(column(rows).spacing(SPACE_XS).width(Length::Fill))
                    .height(Length::Fixed(height))
                    .width(Length::Fill),
            );
        }
        popover(list).into()
    }

    fn like_mark(&self, saved: bool, msg: Msg, tip: &'static str) -> Element<'_, Msg> {
        let p = self.theme.extended_palette();
        let color = if saved {
            p.primary.base.color
        } else {
            p.secondary.base.text
        };
        let icon = icon_svg_colored(self.icons.plus.clone(), 16, color);
        let btn = button(icon)
            .padding(sola_kit::components::style::PAD_CONTROL_SM)
            .style(kit_btn::ghost)
            .on_press(msg);
        let tip = container(kit_text::caption(tip)).padding(Padding {
            top: 5.0,
            right: 8.0,
            bottom: 5.0,
            left: 8.0,
        });
        iced::widget::tooltip(btn, tip, iced::widget::tooltip::Position::Bottom)
            .gap(6)
            .into()
    }

    fn remember_track(&mut self, uri: &str) {
        if uri.is_empty() {
            return;
        }
        self.selected_uri = Some(uri.to_string());
        if self.settings.last_track != uri {
            self.settings.last_track = uri.to_string();
            self.persist_settings();
        }
    }

    fn snap_selected(&self) -> Task<Msg> {
        let Some(PageBody::Tracks { items, .. }) = &self.body else {
            return Task::none();
        };
        let Some(sel) = &self.selected_uri else {
            return Task::none();
        };
        let Some(idx) = items.iter().position(|t| same_track_uri(&t.uri, sel)) else {
            return Task::none();
        };
        let denom = (items.len().saturating_sub(1)).max(1) as f32;
        operation::snap_to(
            tracks_scroll_id(),
            iced::widget::scrollable::RelativeOffset {
                x: 0.0,
                y: idx as f32 / denom,
            },
        )
    }

    fn cover<'a>(&'a self, url: Option<&str>, size: f32, dim: bool) -> Element<'a, Msg> {
        let held = (size - COVER_PLAYER).abs() < f32::EPSILON;
        let inner: Element<'a, Msg> = if let Some(url) = url
            && let Some(handle) = self.art.get(url)
        {
            iced_image(handle.clone())
                .width(Length::Fixed(size))
                .height(Length::Fixed(size))
                .content_fit(iced::ContentFit::Cover)
                .into()
        } else if held && let Some(handle) = &self.player_art_hold {
            iced_image(handle.clone())
                .width(Length::Fixed(size))
                .height(Length::Fixed(size))
                .content_fit(iced::ContentFit::Cover)
                .into()
        } else {
            container(icon_svg(self.icons.disc.clone(), (size * 0.4) as u16))
                .width(Length::Fixed(size))
                .height(Length::Fixed(size))
                .center_x(Length::Fill)
                .center_y(Length::Fill)
                .style(cover_ph_style)
                .into()
        };
        let framed: Element<'a, Msg> = container(inner)
            .width(Length::Fixed(size))
            .height(Length::Fixed(size))
            .style(cover_frame_style)
            .into();
        if !dim {
            return framed;
        }
        stack![
            framed,
            container(Space::new())
                .width(Length::Fixed(size))
                .height(Length::Fixed(size))
                .style(cover_dim_style)
        ]
        .into()
    }
}

fn tracks_scroll_id() -> iced::widget::Id {
    iced::widget::Id::new("spotify-tracks")
}

fn key_msg(key: keyboard::Key, modifiers: keyboard::Modifiers) -> Option<Msg> {
    match key {
        keyboard::Key::Named(NamedKey::Space) if !modifiers.command() => Some(Msg::Toggle),
        keyboard::Key::Named(NamedKey::ArrowLeft) if modifiers.alt() && !modifiers.command() => {
            Some(Msg::Back)
        }
        keyboard::Key::Named(NamedKey::ArrowRight) if modifiers.alt() && !modifiers.command() => {
            Some(Msg::Forward)
        }
        keyboard::Key::Named(NamedKey::ArrowRight) if modifiers.command() => Some(Msg::Next),
        keyboard::Key::Named(NamedKey::ArrowLeft) if modifiers.command() => Some(Msg::Prev),
        keyboard::Key::Character(c) if modifiers.command() && c == "[" => Some(Msg::Back),
        keyboard::Key::Character(c) if modifiers.command() && c == "]" => Some(Msg::Forward),
        keyboard::Key::Character(c) if modifiers.command() && c.eq_ignore_ascii_case("f") => {
            Some(Msg::Open(Page::Search))
        }
        keyboard::Key::Character(c) if modifiers.command() && c.eq_ignore_ascii_case("h") => {
            Some(Msg::Open(Page::Home))
        }
        keyboard::Key::Character(c) if modifiers.command() && c.eq_ignore_ascii_case("l") => {
            Some(Msg::Open(Page::Liked))
        }
        keyboard::Key::Named(NamedKey::Escape) => Some(Msg::CloseAddTo),
        _ => None,
    }
}

fn split_playlists(playlists: &[Playlist]) -> (Vec<&Playlist>, Vec<&Playlist>) {
    let mut made = Vec::new();
    let mut yours = Vec::new();
    for playlist in playlists {
        if playlist.is_generated() {
            made.push(playlist);
        } else {
            yours.push(playlist);
        }
    }
    made.sort_by_key(|p| generated_sort_key(&p.name));
    (made, yours)
}

fn artist_from_ref(artist: &ArtistRef) -> Option<Artist> {
    let id = artist.catalog_id()?.to_string();
    Some(Artist {
        id: id.clone(),
        name: artist.name.clone(),
        uri: artist
            .uri
            .clone()
            .filter(|uri| !uri.is_empty())
            .unwrap_or_else(|| format!("spotify:artist:{id}")),
        ..Artist::default()
    })
}

fn same_track_uri(a: &str, b: &str) -> bool {
    !a.is_empty() && !b.is_empty() && (a == b || a.rsplit(':').next() == b.rsplit(':').next())
}

fn format_ms(ms: u32) -> String {
    let s = ms / 1000;
    format!("{}:{:02}", s / 60, s % 60)
}

fn v_hairline() -> Element<'static, Msg> {
    container(Space::new().width(1).height(Length::Fill))
        .width(1)
        .height(Length::Fill)
        .style(|theme: &Theme| {
            let p = theme.extended_palette();
            container::Style {
                background: Some(Background::Color(mix_white(p.background.base.color, 0.08))),
                ..Default::default()
            }
        })
        .into()
}

fn track_hit_style(selected: bool, hovered: bool) -> impl Fn(&Theme) -> container::Style {
    move |theme: &Theme| {
        let p = theme.extended_palette();
        let bg = if selected {
            Some(Background::Color(p.background.strong.color))
        } else if hovered {
            Some(Background::Color(alpha(p.background.strong.color, 0.70)))
        } else {
            Some(Background::Color(Color::TRANSPARENT))
        };
        container::Style {
            background: bg,
            border: Border {
                color: Color::TRANSPARENT,
                width: 0.0,
                radius: RADIUS_SM.into(),
            },
            ..Default::default()
        }
    }
}

fn canvas_style(theme: &Theme) -> container::Style {
    let p = theme.extended_palette();
    container::Style {
        background: Some(Background::Color(p.background.base.color)),
        text_color: Some(p.background.base.text),
        ..Default::default()
    }
}

fn chrome_style(_theme: &Theme) -> container::Style {
    container::Style {
        background: Some(Background::Color(CHROME_SURFACE)),
        ..Default::default()
    }
}

fn player_style(theme: &Theme) -> container::Style {
    let p = theme.extended_palette();
    container::Style {
        background: Some(Background::Color(p.background.weak.color)),
        border: Border {
            color: mix_white(p.background.weak.color, 0.08),
            width: 1.0,
            radius: 0.0.into(),
        },
        ..Default::default()
    }
}

fn error_bar_style(theme: &Theme) -> container::Style {
    let p = theme.extended_palette();
    container::Style {
        background: Some(Background::Color(p.danger.weak.color)),
        ..Default::default()
    }
}

fn notice_bar_style(theme: &Theme) -> container::Style {
    let p = theme.extended_palette();
    container::Style {
        background: Some(Background::Color(p.success.weak.color)),
        ..Default::default()
    }
}

fn add_filter_id() -> iced::widget::Id {
    iced::widget::Id::new("spotify-add-to-filter")
}

fn ghost_block_style(theme: &Theme) -> container::Style {
    let p = theme.extended_palette();
    container::Style {
        background: Some(Background::Color(mix_white(
            p.background.strong.color,
            0.04,
        ))),
        border: Border {
            color: Color::TRANSPARENT,
            width: 0.0,
            radius: RADIUS_SM.into(),
        },
        ..Default::default()
    }
}

fn cover_ph_style(theme: &Theme) -> container::Style {
    let p = theme.extended_palette();
    container::Style {
        background: Some(Background::Color(p.background.strong.color)),
        border: hairline(p, RADIUS_SM),
        ..Default::default()
    }
}

fn cover_frame_style(theme: &Theme) -> container::Style {
    let p = theme.extended_palette();
    container::Style {
        border: hairline(p, RADIUS_SM),
        ..Default::default()
    }
}

fn cover_dim_style(theme: &Theme) -> container::Style {
    let p = theme.extended_palette();
    let mut wash = p.background.base.color;
    wash.a = 0.62;
    container::Style {
        background: Some(Background::Color(wash)),
        border: hairline(p, RADIUS_SM),
        ..Default::default()
    }
}
