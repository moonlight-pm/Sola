//! Catalogue reads over the librespot playback session, shaped as the Web
//! API answers them.
//!
//! Adapted from Spotifast (MIT), https://github.com/crmne/spotifast
//!
//! The session speaks the protocol Spotify's own clients use, which has no
//! per-app quota: playlists, the playlist library (rootlist), album pages,
//! and an artist's popular tracks open without waiting on the shared Web
//! API app's rate limit. A read the session cannot answer is a
//! [`Failure::Retry`], and the caller asks the Web API instead.

use std::collections::{BTreeSet, HashMap};
use std::future::Future;
use std::time::Duration;

use base64::Engine as _;
use librespot_core::date::Date;
use librespot_core::error::ErrorKind;
use librespot_core::spotify_id::SpotifyId;
use librespot_core::{Error as CoreError, FileId, Session, SpotifyUri};
use librespot_metadata::artist::CountryTopTracks;
use librespot_metadata::image::{ImageSize, Images};
use librespot_metadata::playlist::attribute::PlaylistAttributes;
use librespot_metadata::playlist::item::PlaylistItem as SessionRow;
use librespot_metadata::{
    Album as SessionAlbum, Artist as SessionArtist, Episode as SessionEpisode, Metadata,
    Playlist as SessionPlaylist, Track as SessionTrack,
};
use librespot_protocol::extended_metadata::{
    BatchedEntityRequest, BatchedExtensionResponse, EntityRequest, ExtensionQuery,
};
use librespot_protocol::extension_kind::ExtensionKind;
use librespot_protocol::playlist4_external::{Item, ListItems, MetaItem, SelectedListContent};
use protobuf::{EnumOrUnknown, Message as _};

use crate::api::ApiError;
use crate::api::models::{
    Album, ArtistRef, Episode, Image, Owner, Page, PlayableItem, Playlist, PlaylistItem, Show,
    Track, TrackCount, User, UserRef,
};

const IMAGE_HOST: &str = "https://i.scdn.co/image/";
/// A session read slower than this is handed to the Web API.
const READ_TIMEOUT: Duration = Duration::from_secs(15);
const ROOTLIST_PAGE: usize = 500;
/// Owner names for the library: profile reads in flight at once, owners
/// asked about at most, and how long the library waits before showing ids.
const OWNER_NAMES_AT_ONCE: usize = 4;
const OWNER_NAMES_MAX: usize = 100;
const OWNER_NAMES_WAIT: Duration = Duration::from_secs(5);

/// Why the session could not answer a read.
#[derive(Debug)]
pub enum Failure {
    /// Spotify's answer is final (the playlist is gone or private) and the
    /// Web API would say the same, so show it rather than spend quota.
    Definitive(ApiError),
    /// The session could not be reached or understood; try the Web API.
    Retry(String),
}

impl std::fmt::Display for Failure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Definitive(error) => write!(f, "{error}"),
            Self::Retry(reason) => f.write_str(reason),
        }
    }
}

impl From<CoreError> for Failure {
    fn from(error: CoreError) -> Self {
        Self::Retry(error.to_string())
    }
}

impl From<protobuf::Error> for Failure {
    fn from(error: protobuf::Error) -> Self {
        Self::Retry(error.to_string())
    }
}

fn retry(reason: impl Into<String>) -> Failure {
    Failure::Retry(reason.into())
}

/// Not found or not allowed is what the Web API would answer too. Only the
/// item's own request speaks for the item; any other refusal is a retry.
fn refused(error: CoreError) -> Failure {
    match error.kind {
        ErrorKind::NotFound => Failure::Definitive(ApiError::Status {
            status: 404,
            message: "Spotify doesn't have this anymore.".into(),
        }),
        ErrorKind::PermissionDenied => Failure::Definitive(ApiError::Status {
            status: 403,
            message: "This isn't shared with your account.".into(),
        }),
        _ => error.into(),
    }
}

/// Bounds a read so a stalled session falls back instead of hanging a page.
pub async fn within<T>(read: impl Future<Output = Result<T, Failure>>) -> Result<T, Failure> {
    tokio::time::timeout(READ_TIMEOUT, read)
        .await
        .unwrap_or_else(|_| Err(retry("session read timed out")))
}

// ---- playlists -------------------------------------------------------------

/// A playlist's header and one page of its rows, with song details from one
/// batched request.
pub async fn playlist_page(
    session: &Session,
    id: &str,
    offset: u32,
    limit: u32,
) -> Result<(Playlist, Page<PlaylistItem>), Failure> {
    let uri = SpotifyUri::Playlist {
        id: SpotifyId::from_base62(id)?,
        user: None,
    };
    let list = SessionPlaylist::get(session, &uri).await.map_err(refused)?;
    let mut playlist = header(id, &list);
    let rows = rows(&list, offset, limit);
    let total = total(&list);
    complete(rows.len(), offset, limit, total)?;
    let playables = metadata(session, rows.iter().map(|row| &row.id)).await?;
    let items = rows.iter().map(|row| item(row, &playables)).collect();
    if let Some(owner) = playlist.owner.id.clone().filter(|owner| owner != "spotify") {
        playlist.owner.display_name = user_name(session, &owner).await;
    }
    Ok((playlist, page(items, total, offset, limit)))
}

/// The owner's user id, which the session decorates onto the playlist's URI.
fn owner_of(list: &SessionPlaylist) -> &str {
    match &list.id {
        SpotifyUri::Playlist {
            user: Some(user), ..
        } => user,
        _ => "",
    }
}

fn total(list: &SessionPlaylist) -> u32 {
    u32::try_from(list.length).unwrap_or_default()
}

fn header(id: &str, list: &SessionPlaylist) -> Playlist {
    let attributes = &list.attributes;
    let owner = non_empty(owner_of(list));
    Playlist {
        id: id.to_string(),
        name: attributes.name.clone(),
        uri: format!("spotify:playlist:{id}"),
        description: non_empty(&attributes.description),
        images: playlist_images(attributes),
        owner: Owner {
            uri: owner.as_ref().map(|owner| format!("spotify:user:{owner}")),
            id: owner,
            display_name: None,
        },
        collaborative: attributes.is_collaborative,
        snapshot_id: Some(snapshot(&list.revision)),
        items_count: Some(TrackCount { total: total(list) }),
        ..Default::default()
    }
}

/// The Web API's snapshot id is the playlist revision in base64.
fn snapshot(revision: &[u8]) -> String {
    base64::engine::general_purpose::STANDARD_NO_PAD.encode(revision)
}

/// The sized pictures when the list carries them, else its one picture.
fn playlist_images(attributes: &PlaylistAttributes) -> Vec<Image> {
    let sized: Vec<Image> = attributes
        .picture_sizes
        .iter()
        .filter_map(|picture| {
            Some(Image {
                url: image_url(&picture.url)?,
                width: match picture.target_name.as_str() {
                    "small" => Some(60),
                    "default" => Some(300),
                    "large" => Some(640),
                    _ => None,
                },
                height: None,
            })
        })
        .collect();
    if !sized.is_empty() {
        return sized;
    }
    if attributes.picture.is_empty() {
        return Vec::new();
    }
    vec![Image {
        url: format!("{IMAGE_HOST}{}", FileId::from_raw(&attributes.picture)),
        width: None,
        height: None,
    }]
}

/// A web address, or an image URI naming a file on Spotify's image host.
fn image_url(reference: &str) -> Option<String> {
    if reference.starts_with("https://") || reference.starts_with("http://") {
        return Some(reference.to_string());
    }
    reference
        .strip_prefix("spotify:image:")
        .map(|hex| format!("{IMAGE_HOST}{hex}"))
}

/// The rows at `offset`, at most `limit`, within rows that start at
/// `position` (zero when Spotify sends the whole list).
fn rows(list: &SessionPlaylist, offset: u32, limit: u32) -> &[SessionRow] {
    let contents = &list.contents;
    &contents.items[range(contents.items.len(), contents.position, offset, limit)]
}

fn range(len: usize, position: i32, offset: u32, limit: u32) -> std::ops::Range<usize> {
    let position = u32::try_from(position).unwrap_or_default();
    let start = (offset.saturating_sub(position) as usize).min(len);
    let end = start.saturating_add(limit as usize).min(len);
    start..end
}

/// A window Spotify cut short of the rows that remain is a retry, so the
/// next page never starts after rows nobody was shown.
fn complete(count: usize, offset: u32, limit: u32, total: u32) -> Result<(), Failure> {
    if (count as u32) < limit && offset.saturating_add(count as u32) < total {
        return Err(retry(format!(
            "window at {offset} answered {count} rows of {total}"
        )));
    }
    Ok(())
}

/// `next` is only tested for presence: there while rows remain.
fn page(items: Vec<PlaylistItem>, total: u32, offset: u32, limit: u32) -> Page<PlaylistItem> {
    let next = (offset.saturating_add(items.len() as u32) < total).then(String::new);
    Page {
        items,
        total,
        limit,
        offset,
        next,
    }
}

/// A row as the Web API gives it. A local file has no details to look up;
/// its URI carries what the Web API would have said of it.
fn item(row: &SessionRow, playables: &HashMap<String, PlayableItem>) -> PlaylistItem {
    let uri = row.id.to_uri().unwrap_or_default();
    PlaylistItem {
        added_at: iso8601(row.attributes.timestamp.as_timestamp_ms()),
        added_by: non_empty(&row.attributes.added_by).map(|id| UserRef { id: Some(id) }),
        is_local: matches!(row.id, SpotifyUri::Local { .. }),
        item: playables
            .get(&uri)
            .cloned()
            .or_else(|| local_track(&row.id, uri)),
        track: None,
    }
}

fn local_track(id: &SpotifyUri, uri: String) -> Option<PlayableItem> {
    let SpotifyUri::Local {
        artist,
        album_title,
        track_title,
        duration,
    } = id
    else {
        return None;
    };
    Some(PlayableItem::Track(Track {
        uri,
        name: track_title.clone(),
        duration_ms: u32::try_from(duration.as_millis()).unwrap_or(u32::MAX),
        artists: non_empty(artist)
            .map(|name| ArtistRef {
                name,
                ..Default::default()
            })
            .into_iter()
            .collect(),
        album: non_empty(album_title).map(|name| Album {
            name,
            ..Default::default()
        }),
        is_local: true,
        ..Default::default()
    }))
}

// ---- library ---------------------------------------------------------------

/// Every saved and followed playlist, Spotify's own among them, in the
/// order Spotify keeps them: one rootlist request per 500 rows. Owners come
/// as ids, so their names are read from their profiles, a few at a time.
pub async fn library(session: &Session) -> Result<Vec<Playlist>, Failure> {
    let pages = rootlist(session).await?;
    let mut playlists = library_rows(&pages)?;
    let names = owner_names(session, &playlists).await;
    for playlist in &mut playlists {
        if let Some(name) = playlist.owner.id.as_ref().and_then(|id| names.get(id)) {
            playlist.owner.display_name = Some(name.clone());
        }
    }
    Ok(playlists)
}

async fn rootlist(session: &Session) -> Result<Vec<ListItems>, Failure> {
    let mut pages = Vec::new();
    let mut from = 0usize;
    loop {
        let bytes = session
            .spclient()
            .get_rootlist(from, Some(ROOTLIST_PAGE))
            .await?;
        let Some((contents, more)) =
            rootlist_answer_page(SelectedListContent::parse_from_bytes(&bytes)?, from)?
        else {
            break;
        };
        let count = contents.items.len();
        pages.push(contents);
        if !more || count == 0 {
            break;
        }
        from += count;
    }
    Ok(pages)
}

/// One rootlist answer at `from`: its page and whether rows follow, or
/// `None` when the list ends before `from`. An answer that leaves out rows
/// it says remain is a retry rather than the whole list.
fn rootlist_answer_page(
    response: SelectedListContent,
    from: usize,
) -> Result<Option<(ListItems, bool)>, Failure> {
    let length = response
        .has_length()
        .then(|| usize::try_from(response.length()).unwrap_or_default());
    let Some(contents) = response.contents.into_option() else {
        if let Some(length) = length.filter(|length| *length > from) {
            return Err(retry(format!(
                "rootlist answered no rows at {from} of {length}"
            )));
        }
        return Ok(None);
    };
    let truncated = contents.truncated();
    if truncated && contents.items.is_empty() {
        return Err(retry(format!(
            "rootlist answered an empty page at {from} marked as cut short"
        )));
    }
    Ok(Some((contents, truncated)))
}

/// The playlists among the rootlist's rows. A page without the details
/// Spotify decorates rows with would list nameless playlists: retry.
fn library_rows(pages: &[ListItems]) -> Result<Vec<Playlist>, Failure> {
    let mut playlists = Vec::new();
    for page in pages {
        if page.meta_items.len() != page.items.len() {
            return Err(retry(format!(
                "rootlist page answered {} rows with {} details",
                page.items.len(),
                page.meta_items.len()
            )));
        }
        for (item, meta) in page.items.iter().zip(&page.meta_items) {
            if let Some(playlist) = library_row(item, meta) {
                playlists.push(playlist);
            }
        }
    }
    Ok(playlists)
}

/// One playlist row; `None` for a folder marker or a playlist Spotify says
/// is gone, which the Web API leaves out of the list too.
fn library_row(item: &Item, meta: &MetaItem) -> Option<Playlist> {
    let id = item.uri().strip_prefix("spotify:playlist:")?;
    if !matches!(meta.status_code(), 0 | 200) {
        return None;
    }
    let attributes = PlaylistAttributes::try_from(&*meta.attributes).ok()?;
    let owner = non_empty(meta.owner_username());
    Some(Playlist {
        id: id.to_string(),
        uri: item.uri().to_string(),
        description: non_empty(&attributes.description),
        images: playlist_images(&attributes),
        owner: Owner {
            uri: owner.as_ref().map(|owner| format!("spotify:user:{owner}")),
            id: owner,
            display_name: None,
        },
        public: item
            .attributes
            .as_ref()
            .filter(|attributes| attributes.has_public())
            .map(|attributes| attributes.public()),
        collaborative: attributes.is_collaborative,
        snapshot_id: meta.has_revision().then(|| snapshot(meta.revision())),
        items_count: meta.has_length().then(|| TrackCount {
            total: u32::try_from(meta.length()).unwrap_or_default(),
        }),
        name: attributes.name,
        ..Default::default()
    })
}

async fn owner_names(session: &Session, playlists: &[Playlist]) -> HashMap<String, String> {
    let owners: BTreeSet<String> = playlists
        .iter()
        .filter_map(|playlist| playlist.owner.id.clone())
        .filter(|owner| owner != "spotify")
        .take(OWNER_NAMES_MAX)
        .collect();
    let mut names = HashMap::new();
    let slots = std::sync::Arc::new(tokio::sync::Semaphore::new(OWNER_NAMES_AT_ONCE));
    let mut reads = tokio::task::JoinSet::new();
    for owner in owners {
        let session = session.clone();
        let slots = slots.clone();
        reads.spawn(async move {
            let _slot = slots.acquire_owned().await.ok()?;
            let name = user_name(&session, &owner).await?;
            Some((owner, name))
        });
    }
    let _ = tokio::time::timeout(OWNER_NAMES_WAIT, async {
        while let Some(read) = reads.join_next().await {
            if let Ok(Some((owner, name))) = read {
                names.insert(owner, name);
            }
        }
    })
    .await;
    names
}

/// A user's display name, from the profile view Spotify's own clients read.
async fn user_name(session: &Session, user_id: &str) -> Option<String> {
    let bytes = session
        .spclient()
        .get_user_profile(user_id, Some(0), Some(0))
        .await
        .ok()?;
    let json: serde_json::Value = serde_json::from_slice(&bytes).ok()?;
    profile_name(&json)
}

/// The signed-in account. Id, country and plan come with the login; the
/// display name is read from the profile view, and is missing if that
/// read fails.
pub async fn me(session: &Session) -> User {
    let id = session.username();
    let display_name = tokio::time::timeout(READ_TIMEOUT, user_name(session, &id))
        .await
        .ok()
        .flatten();
    User {
        uri: Some(format!("spotify:user:{id}")),
        display_name,
        images: Vec::new(),
        product: session.get_user_attribute("type"),
        country: non_empty(&session.country()),
        id,
    }
}

fn profile_name(json: &serde_json::Value) -> Option<String> {
    json.get("name")
        .and_then(|value| value.as_str())
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

// ---- albums and artists ----------------------------------------------------

/// An album page: header, cover, and every song in disc order.
pub async fn album(session: &Session, id: &str) -> Result<Album, Failure> {
    let uri = SpotifyUri::Album {
        id: SpotifyId::from_base62(id)?,
    };
    let found = SessionAlbum::get(session, &uri).await.map_err(refused)?;
    let header = album_header(&found);
    let songs: Vec<SpotifyUri> = found.tracks().cloned().collect();
    let details = metadata(session, songs.iter()).await?;
    let items: Vec<Track> = songs
        .iter()
        .filter_map(|uri| match details.get(&uri.to_uri().ok()?) {
            Some(PlayableItem::Track(track)) => {
                let mut track = track.clone();
                track.album = Some(header.clone());
                Some(track)
            }
            _ => None,
        })
        .collect();
    if items.is_empty() && !songs.is_empty() {
        return Err(retry("no song details were available for this album"));
    }
    let total = items.len() as u32;
    Ok(Album {
        total_tracks: Some(total),
        tracks: Some(Page {
            items,
            total,
            limit: total,
            offset: 0,
            next: None,
        }),
        ..header
    })
}

/// Popular tracks in the account's market (or worldwide), in Spotify's order.
pub async fn artist_top_tracks(session: &Session, id: &str) -> Result<Vec<Track>, Failure> {
    let uri = SpotifyUri::Artist {
        id: SpotifyId::from_base62(id)?,
    };
    let artist = SessionArtist::get(session, &uri).await?;
    let uris = artist_top_track_uris(&artist.top_tracks, &session.country())
        .ok_or_else(|| retry("artist metadata has no popular tracks for this market"))?;
    let found = metadata(session, uris.iter()).await?;
    let tracks: Vec<Track> = uris
        .iter()
        .filter_map(|uri| match found.get(&uri.to_uri().ok()?) {
            Some(PlayableItem::Track(track)) => Some(track.clone()),
            _ => None,
        })
        .collect();
    if tracks.is_empty() && !uris.is_empty() {
        return Err(retry("no popular track details were available"));
    }
    Ok(tracks)
}

fn artist_top_track_uris(top_tracks: &CountryTopTracks, country: &str) -> Option<Vec<SpotifyUri>> {
    top_tracks
        .iter()
        .find(|top| top.country == country)
        .or_else(|| top_tracks.iter().find(|top| top.country == "ZZ"))
        .map(|top| top.tracks.iter().take(10).cloned().collect())
}

// ---- song details ----------------------------------------------------------

/// Track and episode details for `uris`, in one batched request. A URI
/// Spotify says it no longer has (404) or withholds (451) is absent, as the
/// Web API leaves such a row empty; any other gap fails the read.
async fn metadata<'a>(
    session: &Session,
    uris: impl Iterator<Item = &'a SpotifyUri>,
) -> Result<HashMap<String, PlayableItem>, Failure> {
    let (request, asked) = batch(uris);
    if asked.is_empty() {
        return Ok(HashMap::new());
    }
    let response = session.spclient().get_extended_metadata(request).await?;
    answers(&asked, response)
}

/// One request for every track and episode among `uris`, each asked once.
fn batch<'a>(
    uris: impl Iterator<Item = &'a SpotifyUri>,
) -> (BatchedEntityRequest, BTreeSet<String>) {
    let mut request = BatchedEntityRequest::new();
    let mut asked = BTreeSet::new();
    for uri in uris {
        let kind = match uri {
            SpotifyUri::Track { .. } => ExtensionKind::TRACK_V4,
            SpotifyUri::Episode { .. } => ExtensionKind::EPISODE_V4,
            _ => continue,
        };
        let Ok(text) = uri.to_uri() else {
            continue;
        };
        if !asked.insert(text.clone()) {
            continue;
        }
        request.entity_request.push(EntityRequest {
            entity_uri: text,
            query: vec![ExtensionQuery {
                extension_kind: EnumOrUnknown::new(kind),
                ..Default::default()
            }],
            ..Default::default()
        });
    }
    (request, asked)
}

fn answers(
    asked: &BTreeSet<String>,
    response: BatchedExtensionResponse,
) -> Result<HashMap<String, PlayableItem>, Failure> {
    let mut unanswered: BTreeSet<&str> = asked.iter().map(String::as_str).collect();
    let mut playables = HashMap::new();
    for array in response.extended_metadata {
        let provider = array.header.provider_error_status;
        if !matches!(provider, 0 | 200) {
            return Err(retry(format!("metadata provider answered {provider}")));
        }
        let kind = match array.extension_kind.enum_value() {
            Ok(kind @ (ExtensionKind::TRACK_V4 | ExtensionKind::EPISODE_V4)) => kind,
            _ => continue,
        };
        for data in array.extension_data {
            unanswered.remove(data.entity_uri.as_str());
            match data.header.status_code {
                0 | 200 => {}
                404 | 451 => continue,
                code => return Err(retry(format!("{} answered {code}", data.entity_uri))),
            }
            let item = data
                .extension_data
                .as_ref()
                .and_then(|any| playable(kind, &any.value))
                .ok_or_else(|| retry(format!("unreadable metadata for {}", data.entity_uri)))?;
            playables.insert(data.entity_uri, item);
        }
    }
    if let Some(uri) = unanswered.first() {
        return Err(retry(format!("no metadata answer for {uri}")));
    }
    Ok(playables)
}

fn playable(kind: ExtensionKind, bytes: &[u8]) -> Option<PlayableItem> {
    match kind {
        ExtensionKind::TRACK_V4 => {
            let message = librespot_protocol::metadata::Track::parse_from_bytes(bytes).ok()?;
            Some(PlayableItem::Track(track(
                SessionTrack::try_from(&message).ok()?,
            )))
        }
        ExtensionKind::EPISODE_V4 => {
            let message = librespot_protocol::metadata::Episode::parse_from_bytes(bytes).ok()?;
            // The typed episode keeps the show's name but not its id.
            let show_id = SpotifyId::from_raw(message.show.gid())
                .ok()
                .and_then(|id| id.to_base62().ok());
            Some(PlayableItem::Episode(episode(
                SessionEpisode::try_from(&message).ok()?,
                show_id,
            )))
        }
        _ => None,
    }
}

fn track(track: SessionTrack) -> Track {
    Track {
        id: track.id.to_id().ok(),
        uri: track.id.to_uri().unwrap_or_default(),
        duration_ms: u32::try_from(track.duration).unwrap_or_default(),
        explicit: track.is_explicit,
        artists: track.artists.iter().map(artist_ref).collect(),
        album: Some(album_header(&track.album)),
        track_number: u32::try_from(track.number).ok(),
        disc_number: u32::try_from(track.disc_number).ok(),
        popularity: u8::try_from(track.popularity.clamp(0, 100)).ok(),
        name: track.name,
        ..Default::default()
    }
}

fn album_header(album: &SessionAlbum) -> Album {
    use librespot_metadata::album::AlbumType;
    let kind = match album.album_type {
        AlbumType::SINGLE | AlbumType::EP => "single",
        AlbumType::COMPILATION => "compilation",
        _ => "album",
    };
    Album {
        id: album.id.to_id().unwrap_or_default(),
        uri: album.id.to_uri().unwrap_or_default(),
        name: album.name.clone(),
        album_type: Some(kind.to_string()),
        images: images(&album.covers),
        artists: album.artists.iter().map(artist_ref).collect(),
        release_date: release_date(&album.date),
        label: non_empty(&album.label),
        ..Default::default()
    }
}

fn episode(episode: SessionEpisode, show_id: Option<String>) -> Episode {
    Episode {
        id: episode.id.to_id().unwrap_or_default(),
        uri: episode.id.to_uri().unwrap_or_default(),
        duration_ms: u32::try_from(episode.duration).unwrap_or_default(),
        images: images(&episode.covers),
        release_date: iso8601(episode.publish_time.as_timestamp_ms()),
        explicit: episode.is_explicit,
        show: show_id.map(|id| Show {
            uri: format!("spotify:show:{id}"),
            id,
            name: episode.show_name.clone(),
            ..Default::default()
        }),
        name: episode.name,
        description: episode.description,
        ..Default::default()
    }
}

fn artist_ref(artist: &SessionArtist) -> ArtistRef {
    ArtistRef {
        id: artist.id.to_id().ok(),
        name: artist.name.clone(),
        uri: artist.id.to_uri().ok(),
    }
}

/// Covers with a width the image picker can rank: the message's own, or
/// the one its size class implies.
fn images(images: &Images) -> Vec<Image> {
    images
        .iter()
        .map(|image| {
            let width = u32::try_from(image.width)
                .ok()
                .filter(|width| *width > 0)
                .or(match image.size {
                    ImageSize::SMALL => Some(64),
                    ImageSize::DEFAULT => Some(300),
                    ImageSize::LARGE => Some(640),
                    ImageSize::XLARGE => Some(1280),
                });
            Image {
                url: format!("{IMAGE_HOST}{}", image.id),
                width,
                height: u32::try_from(image.height)
                    .ok()
                    .filter(|h| *h > 0)
                    .or(width),
            }
        })
        .collect()
}

/// `YYYY-MM-DD` from the album's own calendar date (pre-1970 included);
/// `None` for the year 0 a missing date reads as.
fn release_date(date: &Date) -> Option<String> {
    (date.year() > 0).then(|| {
        format!(
            "{:04}-{:02}-{:02}",
            date.year(),
            u8::from(date.month()),
            date.day()
        )
    })
}

fn non_empty(text: &str) -> Option<String> {
    (!text.is_empty()).then(|| text.to_string())
}

/// Milliseconds since the epoch as the Web API writes a time; `None` for the
/// zero a list gives when it never recorded one.
fn iso8601(timestamp_ms: i64) -> Option<String> {
    if timestamp_ms <= 0 {
        return None;
    }
    let secs = timestamp_ms.div_euclid(1000);
    let (year, month, day) = civil_from_days(secs.div_euclid(86_400));
    let of_day = secs.rem_euclid(86_400);
    Some(format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}Z",
        of_day / 3600,
        of_day % 3600 / 60,
        of_day % 60
    ))
}

/// Proleptic Gregorian date for days since 1970-01-01 (Hinnant's algorithm).
fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let month = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let year = yoe + era * 400 + i64::from(month <= 2);
    (year, month, day)
}

#[cfg(test)]
mod tests {
    use super::*;
    use librespot_protocol::entity_extension_data::EntityExtensionData;
    use librespot_protocol::extended_metadata::EntityExtensionDataArray;

    fn track_uri(n: u8) -> SpotifyUri {
        SpotifyUri::Track {
            id: SpotifyId::from_raw(&[n; 16]).unwrap(),
        }
    }

    #[test]
    fn times_are_written_as_the_web_api_writes_them() {
        assert_eq!(iso8601(0), None);
        assert_eq!(iso8601(1_000).as_deref(), Some("1970-01-01T00:00:01Z"));
        assert_eq!(
            iso8601(1_710_267_720_000).as_deref(),
            Some("2024-03-12T18:22:00Z")
        );
        assert_eq!(civil_from_days(-1), (1969, 12, 31));
        assert_eq!(civil_from_days(11_016), (2000, 2, 29));
    }

    #[test]
    fn album_dates_before_1970_keep_their_year() {
        let date = |year, month, day| {
            let mut message = librespot_protocol::metadata::Date::new();
            message.set_year(year);
            message.set_month(month);
            message.set_day(day);
            Date::try_from(&message).unwrap()
        };
        assert_eq!(
            release_date(&date(1965, 8, 6)).as_deref(),
            Some("1965-08-06")
        );
        assert_eq!(
            release_date(&date(2024, 3, 12)).as_deref(),
            Some("2024-03-12")
        );
        let missing = Date::try_from(&librespot_protocol::metadata::Date::new()).unwrap();
        assert_eq!(release_date(&missing), None);
    }

    #[test]
    fn rows_are_taken_relative_to_where_the_answer_starts() {
        assert_eq!(range(100, 0, 0, 50), 0..50);
        assert_eq!(range(100, 0, 50, 50), 50..100);
        assert_eq!(range(30, 50, 50, 50), 0..30);
        assert_eq!(range(10, 0, 50, 50), 10..10);
    }

    #[test]
    fn a_short_window_is_not_paged_past() {
        assert!(complete(50, 0, 50, 120).is_ok());
        assert!(complete(20, 100, 50, 120).is_ok());
        assert!(matches!(complete(10, 0, 50, 120), Err(Failure::Retry(_))));
    }

    #[test]
    fn a_page_ends_where_the_list_does() {
        let full = page(vec![PlaylistItem::default(); 50], 120, 0, 50);
        assert!(full.next.is_some());
        let last = page(vec![PlaylistItem::default(); 20], 120, 100, 50);
        assert!(last.next.is_none());
        assert_eq!(last.offset, 100);
    }

    #[test]
    fn pictures_resolve_to_the_image_host() {
        assert_eq!(
            image_url("spotify:image:ab67").as_deref(),
            Some("https://i.scdn.co/image/ab67")
        );
        assert_eq!(
            image_url("https://mosaic.scdn.co/640/x").as_deref(),
            Some("https://mosaic.scdn.co/640/x")
        );
        assert_eq!(image_url("garbage"), None);
    }

    #[test]
    fn a_batch_asks_once_for_each_song() {
        let a = track_uri(1);
        let b = track_uri(2);
        let album = SpotifyUri::Album {
            id: SpotifyId::from_raw(&[3; 16]).unwrap(),
        };
        let (request, asked) = batch([&a, &b, &a, &album].into_iter());
        assert_eq!(asked.len(), 2);
        assert_eq!(request.entity_request.len(), 2);
    }

    fn answer(uri: &str, status: i32, bytes: Option<Vec<u8>>) -> EntityExtensionData {
        let mut data = EntityExtensionData::new();
        data.entity_uri = uri.to_string();
        data.header.mut_or_insert_default().status_code = status;
        if let Some(bytes) = bytes {
            data.extension_data.mut_or_insert_default().value = bytes;
        }
        data
    }

    fn response(rows: Vec<EntityExtensionData>) -> BatchedExtensionResponse {
        let mut array = EntityExtensionDataArray::new();
        array.extension_kind = EnumOrUnknown::new(ExtensionKind::TRACK_V4);
        array.extension_data = rows;
        let mut response = BatchedExtensionResponse::new();
        response.extended_metadata.push(array);
        response
    }

    #[test]
    fn a_song_spotify_no_longer_has_leaves_its_row_empty() {
        let uri = track_uri(1).to_uri().unwrap();
        let asked = BTreeSet::from([uri.clone()]);
        let found = answers(&asked, response(vec![answer(&uri, 404, None)])).unwrap();
        assert!(found.is_empty());
    }

    #[test]
    fn an_unanswered_song_is_a_retry_not_an_empty_row() {
        let uri = track_uri(1).to_uri().unwrap();
        let asked = BTreeSet::from([uri]);
        assert!(matches!(
            answers(&asked, response(Vec::new())),
            Err(Failure::Retry(_))
        ));
    }

    #[test]
    fn unreadable_song_bytes_are_a_retry() {
        let uri = track_uri(1).to_uri().unwrap();
        let asked = BTreeSet::from([uri.clone()]);
        assert!(matches!(
            answers(
                &asked,
                response(vec![answer(&uri, 200, Some(vec![0xff, 0xff]))])
            ),
            Err(Failure::Retry(_))
        ));
    }

    #[test]
    fn a_profile_answer_carries_the_display_name() {
        let json = serde_json::json!({"name": "Joshua", "image_url": ""});
        assert_eq!(profile_name(&json).as_deref(), Some("Joshua"));
        assert_eq!(profile_name(&serde_json::json!({"name": ""})), None);
    }
}
