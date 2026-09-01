//! Kit UI: library rail, pages, player bar.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::Duration;

use iced::keyboard;
use iced::keyboard::key::Named as NamedKey;
use iced::widget::image::Handle as ImageHandle;
use iced::widget::{
    Space, button, column, container, image as iced_image, row, scrollable, slider,
};
use iced::{
    Alignment, Background, Border, Color, Element, Event as IcedEvent, Length, Padding, Subscription,
    Task, Theme,
};
use sola_bus::Message;
use sola_bus::topics::Topic;
use sola_kit::app::{apply_theme_update, bus_subscription, is_self_quit};
use sola_kit::components::icon::{icon_handle, icon_svg};
use sola_kit::components::style::{
    CHROME_SURFACE, RADIUS_SM, SPACE_LG, SPACE_MD, SPACE_SM, SPACE_XL, SPACE_XS,
    hairline, mix_white,
};
use sola_kit::components::text as kit_text;
use sola_kit::components::text_input::text_input;
use sola_kit::components::toolbar::toolbar_icon_tip;
use sola_kit::components::popover::{Placement, popover, popover_anchored};
use sola_kit::components::{SidebarItem, SidebarSection, button as kit_btn, sidebar};
use sola_kit::theme::default_theme;

use crate::api::models::{Album, Artist, Device, Playlist, Track, pick_image};
use crate::api::PlayRequest as ApiPlay;
use crate::bridge;
use crate::cache::{self, Skipped};
use crate::paths::AppDirs;
use crate::player::Playback;
use crate::settings::Settings;
use crate::worker::{
    AuthStatus, Cmd, Event, LocalPlayback, NowPlaying, Page, PageBody,
};

const APP_ID: &str = "sola-spotify";
const SIDEBAR_W: f32 = 220.0;
const PLAYER_H: f32 = 76.0;
const COVER_ROW: f32 = 40.0;
const COVER_TILE: f32 = 148.0;
const COVER_PLAYER: f32 = 56.0;

#[derive(Debug, Clone)]
pub enum Msg {
    Bus(Arc<Message>),
    Worker(Event),
    SignIn,
    CancelSignIn,
    SignOut,
    PlayHere,
    Open(Page),
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
    Toggle,
    Next,
    Prev,
    SeekFrac(f32),
    Volume(u8),
    Shuffle,
    Repeat,
    Like,
    SaveTrack(String),
    SkipTrack { uri: String },
    Transfer(String),
    ToggleDevices,
    LoadMore,
    DismissError,
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
    shuffle: iced::widget::svg::Handle,
    repeat: iced::widget::svg::Handle,
    speaker: iced::widget::svg::Handle,
    volume: iced::widget::svg::Handle,
    settings: iced::widget::svg::Handle,
    plus: iced::widget::svg::Handle,
    minus: iced::widget::svg::Handle,
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
            shuffle: icon_handle("lucide/shuffle"),
            repeat: icon_handle("lucide/repeat"),
            speaker: icon_handle("lucide/speaker"),
            volume: icon_handle("lucide/volume-2"),
            settings: icon_handle("lucide/settings"),
            plus: icon_handle("lucide/circle-plus"),
            minus: icon_handle("lucide/circle-minus"),
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
        let page = Page::decode(&settings.last_page).unwrap_or(Page::Home);
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
            body: None,
            page_cache: HashMap::new(),
            now: NowPlaying::default(),
            devices: Vec::new(),
            playlists: Vec::new(),
            art: HashMap::new(),
            art_inflight: HashSet::new(),
            saved: HashMap::new(),
            search: String::new(),
            devices_open: false,
            error: None,
            settings,
            playing_since: None,
            pending_uri: None,
            player_art_hold: None,
            skipped,
            dirs,
        };
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
        (Self::from_disk(), sola_kit::window_ready_task(Msg::WindowReady))
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
            _ => None,
        });
        let tick = if self.now.playback == Playback::Playing {
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
                    self.playing_since = (self.now.playback == Playback::Playing)
                        .then(std::time::Instant::now);
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
            Msg::SkipTrack { uri } => {
                self.toggle_skip(uri);
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
            Msg::WindowReady(id) => {
                self.window_id = id;
                Task::none()
            }
            Msg::TitleDrag => sola_kit::drag(self.window_id),
            Msg::TitleResize(dir) => sola_kit::drag_resize(self.window_id, dir),
            Msg::TitleClose => {
                bridge::send(Cmd::Shutdown);
                iced::exit()
            }
            Msg::Tick => Task::none(),
        }
    }

    fn navigate(&mut self, page: Page) {
        self.page = page.clone();
        if page.persist_as_last() {
            self.settings.last_page = page.encode();
            self.settings.save(&self.dirs);
        }
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

    fn play_now(&mut self, uri: &str, context: Option<String>) {
        let snapshot = self.current_tracks().iter().find(|t| t.uri == uri).map(|t| {
            (
                t.name.clone(),
                t.artist_names(),
                t.album.as_ref().map(|a| a.name.clone()).unwrap_or_default(),
                t.image(300).map(str::to_string),
                t.duration_ms,
                t.uri.clone(),
            )
        });
        if let Some((title, artists, album, art_url, duration_ms, track_uri)) = snapshot {
            self.pending_uri = Some(track_uri.clone());
            self.now.title = title;
            self.now.artists = artists;
            self.now.album = album;
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
        self.saved.get(uri).copied().unwrap_or(false)
    }

    fn toggle_saved(&mut self, uri: &str) {
        if uri.is_empty() {
            return;
        }
        let saved = !self.is_saved(uri);
        self.saved.insert(uri.to_string(), saved);
        if self.now.uri == uri {
            self.now.saved = Some(saved);
        }
        bridge::send(Cmd::SetSaved {
            uri: uri.to_string(),
            saved,
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
            && self.now.uri == uri
            && matches!(self.now.playback, Playback::Playing | Playback::Loading)
        {
            self.pending_uri = None;
            bridge::send(Cmd::Media(crate::media::MediaCommand::Next));
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
            Event::Auth(status) => self.auth = status,
            Event::User(user) => self.user = Some(user),
            Event::Premium(p) => self.premium = p,
            Event::LocalPlayback(local) => self.local = local,
            Event::Page { page, body } => {
                if let PageBody::Home { playlists, .. } = &body {
                    self.playlists = playlists.clone();
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
                        }
                    }
                }
            }
            Event::NowPlaying(now) => {
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
                self.want_art(now.art_url.as_deref());
                if now.playback == Playback::Playing {
                    self.playing_since = Some(std::time::Instant::now());
                } else if now.playback != Playback::Loading {
                    self.playing_since = None;
                }
                if let Some(saved) = now.saved {
                    self.saved.insert(now.uri.clone(), saved);
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
                self.saved.insert(uri.clone(), saved);
                if self.now.uri == uri {
                    self.now.saved = Some(saved);
                }
            }
            Event::Settings(settings) => self.settings = settings,
            Event::Error(err) => self.error = Some(err),
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
            PageBody::Artist { artist, top, albums } => {
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
            AuthStatus::SignedOut | AuthStatus::Failed(_) | AuthStatus::WaitingForBrowser { .. } => {
                self.view_login()
            }
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
            _ => "Sign in with Spotify to browse your library. Playing on this computer needs Premium.",
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
            column![self.view_page(), self.view_player()]
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
            SidebarItem::new("Search", Msg::Open(Page::Search)).active(matches!(self.page, Page::Search)),
            SidebarItem::new("Liked Songs", Msg::Open(Page::Liked)).active(self.page == Page::Liked),
            SidebarItem::new("Albums", Msg::Open(Page::Albums)).active(self.page == Page::Albums),
            SidebarItem::new("Artists", Msg::Open(Page::Artists)).active(self.page == Page::Artists),
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
        browse.push(SidebarItem::new("Settings", Msg::Open(Page::Settings)).active(self.page == Page::Settings));

        let mut sections = vec![SidebarSection::new("Library", browse)];
        if !self.playlists.is_empty() {
            let items: Vec<_> = self
                .playlists
                .iter()
                .map(|p| {
                    SidebarItem::new(
                        p.name.clone(),
                        Msg::OpenPlaylist(p.id.clone()),
                    )
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

    fn view_page(&self) -> Element<'_, Msg> {
        let inner: Element<'_, Msg> = if self.body_matches(&self.page) {
            match &self.page {
                Page::Home => self.view_home(),
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
        let mut header = row![self.cover(art, 96.0)]
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
                container(row![
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
                .padding(Padding::from([SPACE_SM, SPACE_MD])))
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
        if !playlists.is_empty() {
            col = col.push(kit_text::subheading("Playlists"));
            col = col.push(self.playlist_tiles(playlists));
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
            ..
        }) = &self.body
        else {
            return Space::new().into();
        };
        let play_all = context_uri.clone().map(Msg::PlayContext);
        let mut header = row![self.cover(art.as_deref(), 96.0)]
            .spacing(SPACE_LG)
            .align_y(Alignment::Center);
        let mut titles = column![
            kit_text::heading(title.clone()),
            kit_text::caption(subtitle.clone()).style(kit_text::muted),
        ]
        .spacing(SPACE_SM);
        if let Some(msg) = play_all {
            titles = titles.push(kit_btn::labeled("Play", kit_btn::primary).on_press(msg));
        }
        header = header.push(titles);
        let list = self.track_list(items, context_uri.clone());
        let mut col = column![header, list].spacing(SPACE_XL).padding(SPACE_XL);
        if items.len() < *total as usize {
            col = col.push(kit_btn::labeled_sm("Load more", kit_btn::secondary).on_press(Msg::LoadMore));
        }
        scrollable(col.width(Length::Fill))
            .width(Length::Fill)
            .height(Length::Fill)
            .into()
    }

    fn view_albums(&self) -> Element<'_, Msg> {
        let Some(PageBody::Albums { items, total, .. }) = &self.body else {
            return Space::new().into();
        };
        let mut col = column![kit_text::heading("Albums"), self.album_tiles(items)]
            .spacing(SPACE_XL)
            .padding(SPACE_XL);
        if items.len() < *total as usize {
            col = col.push(kit_btn::labeled_sm("Load more", kit_btn::secondary).on_press(Msg::LoadMore));
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
            column![kit_text::heading("Artists"), self.artist_tiles(items)]
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
        let header = row![
            self.cover(pick_image(&artist.images, 300), 120.0),
            column![
                kit_text::heading(artist.name.clone()),
                kit_text::caption(format!(
                    "{} followers",
                    artist.followers.as_ref().map(|f| f.total).unwrap_or(0)
                ))
                .style(kit_text::muted),
            ]
            .spacing(SPACE_SM),
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
                button(
                    row![
                        self.cover(item.image(64), COVER_ROW),
                        column![
                            kit_text::body(item.name().to_string()),
                            kit_text::caption(item.subtitle()).style(kit_text::muted),
                        ]
                        .spacing(SPACE_XS)
                        .width(Length::Fill),
                        kit_text::caption(format_ms(item.duration_ms())).style(kit_text::muted),
                    ]
                    .spacing(SPACE_MD)
                    .align_y(Alignment::Center)
                    .padding(Padding::from([SPACE_SM, SPACE_MD])),
                )
                .style(kit_btn::list_item(false))
                .on_press(Msg::PlayTrack {
                    uri,
                    context: None,
                })
                .width(Length::Fill)
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
        let rows: Vec<Element<'a, Msg>> = tracks
            .iter()
            .enumerate()
            .map(|(i, track)| {
                let uri = track.uri.clone();
                let ctx = context.clone();
                let playing = same_track_uri(&self.now.uri, &uri);
                let saved = self.is_saved(&uri);
                let skipped = self.skipped.contains(&uri);
                let artist_id = track.artists.first().and_then(|a| a.id.clone());
                let album_id = track.album.as_ref().map(|a| a.id.clone());
                let mut meta = row![].spacing(SPACE_SM);
                if let Some(id) = artist_id {
                    meta = meta.push(
                        button(kit_text::caption(track.artist_names()).style(kit_text::muted))
                            .style(kit_btn::ghost)
                            .on_press(Msg::OpenArtist(id))
                            .padding(0),
                    );
                } else {
                    meta = meta.push(kit_text::caption(track.artist_names()).style(kit_text::muted));
                }
                if let Some(id) = album_id.filter(|s| !s.is_empty())
                    && let Some(album) = track.album.as_ref()
                {
                    meta = meta.push(kit_text::caption("·").style(kit_text::muted));
                    meta = meta.push(
                        button(kit_text::caption(album.name.clone()).style(kit_text::muted))
                            .style(kit_btn::ghost)
                            .on_press(Msg::OpenAlbum(id))
                            .padding(0),
                    );
                }
                let play = button(
                    row![
                        kit_text::caption(format!("{}", i + 1))
                            .style(kit_text::muted)
                            .width(Length::Fixed(24.0)),
                        self.cover(track.image(64), COVER_ROW),
                        column![
                            {
                                let title = kit_text::body(track.name.clone());
                                if skipped && !playing {
                                    title.style(kit_text::muted)
                                } else if playing {
                                    title.style(kit_text::accent)
                                } else {
                                    title
                                }
                            },
                            meta,
                        ]
                        .spacing(SPACE_XS)
                        .width(Length::Fill),
                    ]
                    .spacing(SPACE_MD)
                    .align_y(Alignment::Center),
                )
                .style(kit_btn::list_item(playing))
                .on_press(Msg::PlayTrack {
                    uri: uri.clone(),
                    context: ctx,
                })
                .width(Length::Fill);

                let plus = if saved {
                    toolbar_icon_tip(self.icons.check.clone(), "Unlike", Some(Msg::SaveTrack(uri.clone())))
                } else {
                    toolbar_icon_tip(self.icons.plus.clone(), "Like", Some(Msg::SaveTrack(uri.clone())))
                };
                let skip_tip = if skipped {
                    "Play this again"
                } else {
                    "Don't play this"
                };
                let marks = row![
                    plus,
                    toolbar_icon_tip(
                        self.icons.minus.clone(),
                        skip_tip,
                        Some(Msg::SkipTrack { uri: uri.clone() }),
                    ),
                ]
                .spacing(SPACE_XS)
                .align_y(Alignment::Center);
                row![
                    play,
                    marks,
                    kit_text::caption(format_ms(track.duration_ms)).style(kit_text::muted),
                ]
                .spacing(SPACE_SM)
                .align_y(Alignment::Center)
                .padding(Padding::from([SPACE_SM, SPACE_MD]))
                .into()
            })
            .collect();
        column(rows).spacing(1.0).width(Length::Fill).into()
    }

    fn playlist_tiles<'a>(&'a self, playlists: &'a [Playlist]) -> Element<'a, Msg> {
        self.tile_row(
            playlists
                .iter()
                .map(|p| {
                    (
                        p.name.clone(),
                        p.owner_name().to_string(),
                        pick_image(&p.images, 200).map(str::to_string),
                        Msg::OpenPlaylist(p.id.clone()),
                    )
                })
                .collect(),
        )
    }

    fn album_tiles<'a>(&'a self, albums: &'a [Album]) -> Element<'a, Msg> {
        self.tile_row(
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
                .collect(),
        )
    }

    fn artist_tiles<'a>(&'a self, artists: &'a [Artist]) -> Element<'a, Msg> {
        self.tile_row(
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
                .collect(),
        )
    }

    fn tile_row<'a>(&'a self, tiles: Vec<(String, String, Option<String>, Msg)>) -> Element<'a, Msg> {
        let items: Vec<Element<'a, Msg>> = tiles
            .into_iter()
            .map(|(title, sub, art, msg)| {
                button(
                    column![
                        self.cover(art.as_deref(), COVER_TILE),
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
            .collect();
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
        let like_icon = if self.now_saved() {
            self.icons.check.clone()
        } else {
            self.icons.plus.clone()
        };
        let like_tip = if self.now_saved() { "Unlike" } else { "Like" };

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
            .width(Length::Fill)
        } else if self.now.title.is_empty() {
            column![
                kit_text::body("Nothing playing").style(kit_text::muted),
                kit_text::caption("Play a song to use this computer — one-time setup in the browser.").style(kit_text::muted),
            ]
            .spacing(SPACE_XS)
            .width(Length::Fill)
        } else {
            column![
                kit_text::body(self.now.title.clone()),
                kit_text::caption(self.now.artists.clone()).style(kit_text::muted),
            ]
            .spacing(SPACE_XS)
            .width(Length::Fill)
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
            kit_text::caption(format_ms(pos)).style(kit_text::muted).width(Length::Fixed(36.0)),
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
            toolbar_icon_tip(like_icon, like_tip, Some(Msg::Like)),
            devices,
            icon_svg(self.icons.volume.clone(), 14),
            slider(0.0..=100.0, self.now.volume_percent as f32, |v| {
                Msg::Volume(v as u8)
            })
            .width(Length::Fixed(88.0)),
        ]
        .spacing(SPACE_MD)
        .align_y(Alignment::Center);

        let bar = row![
            self.cover(self.now.art_url.as_deref(), COVER_PLAYER),
            now,
            column![transport, seek]
                .spacing(SPACE_XS)
                .align_x(Alignment::Center)
                .width(Length::FillPortion(4)),
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

    fn cover<'a>(&'a self, url: Option<&str>, size: f32) -> Element<'a, Msg> {
        let held = (size - COVER_PLAYER).abs() < f32::EPSILON;
        let inner: Element<'a, Msg> = if let Some(url) = url
            && let Some(handle) = self.art.get(url)
        {
            iced_image(handle.clone())
                .width(Length::Fixed(size))
                .height(Length::Fixed(size))
                .content_fit(iced::ContentFit::Cover)
                .into()
        } else if held
            && let Some(handle) = &self.player_art_hold
        {
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
        container(inner)
            .width(Length::Fixed(size))
            .height(Length::Fixed(size))
            .style(cover_frame_style)
            .into()
    }
}

fn key_msg(key: keyboard::Key, modifiers: keyboard::Modifiers) -> Option<Msg> {
    match key {
        keyboard::Key::Named(NamedKey::Space) if !modifiers.command() => Some(Msg::Toggle),
        keyboard::Key::Named(NamedKey::ArrowRight) if modifiers.command() => Some(Msg::Next),
        keyboard::Key::Named(NamedKey::ArrowLeft) if modifiers.command() => Some(Msg::Prev),
        keyboard::Key::Character(c) if modifiers.command() && c.eq_ignore_ascii_case("f") => {
            Some(Msg::Open(Page::Search))
        }
        keyboard::Key::Character(c) if modifiers.command() && c.eq_ignore_ascii_case("h") => {
            Some(Msg::Open(Page::Home))
        }
        keyboard::Key::Character(c) if modifiers.command() && c.eq_ignore_ascii_case("l") => {
            Some(Msg::Open(Page::Liked))
        }
        _ => None,
    }
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

fn ghost_block_style(theme: &Theme) -> container::Style {
    let p = theme.extended_palette();
    container::Style {
        background: Some(Background::Color(mix_white(p.background.strong.color, 0.04))),
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
