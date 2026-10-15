//! Live share: every canvas on the web at `<public_url>/<token>/`, served
//! by Design itself. A Cloudflare Tunnel points the public host at
//! [`ShareConfig::listen`]; Design runs the connector while it is open (see
//! [`tunnel`]), so a link works exactly while Design does, and always shows
//! the files as they are now.
//!
//! Each canvas gets a token (128 random bits) the first time Design sees it;
//! rotating it kills the old link. Under a token the server answers only for
//! that canvas: its folder, shared files in `design/` (not other canvases),
//! its `root` web root, and the files its boards loaded from elsewhere in the
//! project (recorded while it is live, kept across restarts). Anything else
//! — other tokens' files, the rest of the project, dotfiles — is a 404.
//! Root-absolute URLs (`/fonts/x`) are redirected under the token of the
//! page that asked (same-origin `Referer`).

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::io::{self, Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, RwLock};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::annot::Place;
use crate::canvas;
use crate::serve;

pub mod tunnel;

/// Default listen address (the tunnel's origin).
const DEFAULT_LISTEN: &str = "127.0.0.1:47820";
/// Connections served at once; more are dropped (the listener faces the
/// internet through the tunnel).
const MAX_CONNECTIONS: usize = 64;
const WRITE_TIMEOUT: Duration = Duration::from_secs(30);
const VIEWER: &str = include_str!("share_viewer.html");
/// Headers on every share answer: never indexed, the token never leaves
/// the origin in a `Referer`.
const PRIVATE: &str = "Referrer-Policy: same-origin\r\nX-Robots-Tag: noindex, nofollow\r\n\
                       X-Content-Type-Options: nosniff\r\n";

/// `~/.config/sola/design/share.toml`.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ShareConfig {
    /// Where links point (`https://design.moonlight.pm`). Without it links
    /// are the local listen address.
    pub public_url: Option<String>,
    #[serde(default = "default_listen")]
    pub listen: SocketAddr,
    /// Token of a remotely managed Cloudflare Tunnel whose ingress points
    /// at `listen`. Design runs `cloudflared` with it while open.
    pub tunnel_token: Option<String>,
    /// `cloudflared` binary (default: from `PATH`).
    pub cloudflared: Option<PathBuf>,
}

fn default_listen() -> SocketAddr {
    DEFAULT_LISTEN.parse().expect("default listen")
}

impl Default for ShareConfig {
    fn default() -> Self {
        Self {
            public_url: None,
            listen: default_listen(),
            tunnel_token: None,
            cloudflared: None,
        }
    }
}

pub fn config_path() -> PathBuf {
    crate::profile::data_dir().join("share.toml")
}

fn state_path() -> PathBuf {
    crate::profile::data_dir().join("share.json")
}

/// Missing file → defaults (local links only).
pub fn load_config(path: &Path) -> Result<ShareConfig, String> {
    match std::fs::read_to_string(path) {
        Ok(text) => toml::from_str(&text).map_err(|e| format!("{}: {e}", path.display())),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(ShareConfig::default()),
        Err(e) => Err(format!("{}: {e}", path.display())),
    }
}

/// What survives a restart: tokens, and each canvas's outside files.
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
struct Saved {
    /// Canvas key (`project/canvas`) → token.
    #[serde(default)]
    tokens: BTreeMap<String, String>,
    #[serde(default)]
    links: BTreeMap<String, BTreeSet<PathBuf>>,
}

/// One shareable canvas.
#[derive(Debug, Clone, PartialEq)]
struct Site {
    key: String,
    project_root: PathBuf,
    dir: PathBuf,
}

#[derive(Debug, Default)]
struct State {
    saved: Saved,
    /// Token → canvas.
    sites: HashMap<String, Site>,
    /// Canvas key → `auto` artboard id → the height Design measured, so the
    /// web lays the canvas out exactly as Design does.
    heights: HashMap<String, HashMap<String, u32>>,
}

pub struct Share {
    config: ShareConfig,
    /// Bound listen address.
    addr: SocketAddr,
    state: Arc<RwLock<State>>,
    state_path: PathBuf,
    _tunnel: Option<tunnel::Tunnel>,
}

impl Share {
    /// Listen on `config.listen` and start the tunnel connector if one is
    /// configured.
    pub fn start(config: ShareConfig) -> io::Result<Self> {
        Self::start_with(config, state_path())
    }

    fn start_with(config: ShareConfig, state_path: PathBuf) -> io::Result<Self> {
        let saved: Saved = std::fs::read_to_string(&state_path)
            .ok()
            .and_then(|t| serde_json::from_str(&t).ok())
            .unwrap_or_default();
        let state = Arc::new(RwLock::new(State {
            saved,
            sites: HashMap::new(),
            heights: HashMap::new(),
        }));
        let listener = TcpListener::bind(config.listen)?;
        let addr = listener.local_addr()?;
        let shared = state.clone();
        std::thread::Builder::new()
            .name("design-share".into())
            .spawn(move || {
                let busy = Arc::new(AtomicUsize::new(0));
                for stream in listener.incoming().flatten() {
                    if busy.load(Ordering::Acquire) >= MAX_CONNECTIONS {
                        continue;
                    }
                    busy.fetch_add(1, Ordering::AcqRel);
                    let (state, busy) = (shared.clone(), busy.clone());
                    std::thread::spawn(move || {
                        let _ = stream.set_write_timeout(Some(WRITE_TIMEOUT));
                        if let Err(e) = handle(stream, &state) {
                            tracing::debug!(error = %e, "share connection");
                        }
                        busy.fetch_sub(1, Ordering::AcqRel);
                    });
                }
            })?;
        tracing::info!(listen = %config.listen, public = ?config.public_url, "share listening");
        let tunnel = config.tunnel_token.as_deref().and_then(|token| {
            tunnel::Tunnel::start(token, config.cloudflared.as_deref())
                .map_err(|e| tracing::error!(error = %e, "share tunnel did not start"))
                .ok()
        });
        Ok(Self {
            config,
            addr,
            state,
            state_path,
            _tunnel: tunnel,
        })
    }

    /// The canvases there are (`key → (project root, canvas dir)`): each
    /// gets a token the first time.
    pub fn set_canvases(&self, canvases: impl IntoIterator<Item = (String, PathBuf, PathBuf)>) {
        let Ok(mut st) = self.state.write() else {
            return;
        };
        let mut minted = false;
        let mut sites = HashMap::new();
        for (key, project_root, dir) in canvases {
            let token = match st.saved.tokens.get(&key) {
                Some(t) => t.clone(),
                None => {
                    let t = new_token();
                    st.saved.tokens.insert(key.clone(), t.clone());
                    minted = true;
                    t
                }
            };
            sites.insert(
                token,
                Site {
                    key,
                    project_root,
                    dir,
                },
            );
        }
        st.sites = sites;
        if minted {
            self.save(&st.saved);
        }
    }

    /// The files a canvas's boards loaded from outside `design/`.
    pub fn set_links(&self, key: &str, links: BTreeSet<PathBuf>) {
        let Ok(mut st) = self.state.write() else {
            return;
        };
        if st.saved.links.get(key) == Some(&links) {
            return;
        }
        st.saved.links.insert(key.to_string(), links);
        self.save(&st.saved);
    }

    /// The height Design measured for an `auto` artboard of a canvas.
    pub fn set_height(&self, key: &str, artboard: &str, height: u32) {
        if let Ok(mut st) = self.state.write() {
            st.heights
                .entry(key.to_string())
                .or_default()
                .insert(artboard.to_string(), height);
        }
    }

    /// The public link of a canvas.
    pub fn url(&self, key: &str) -> Option<String> {
        let st = self.state.read().ok()?;
        let token = st.saved.tokens.get(key)?;
        Some(format!("{}/{token}/", self.base()))
    }

    /// A new token for `key`; the old link stops working.
    pub fn rotate(&self, key: &str) -> Option<String> {
        {
            let mut st = self.state.write().ok()?;
            let old = st.saved.tokens.get(key)?.clone();
            let token = new_token();
            if let Some(site) = st.sites.remove(&old) {
                st.sites.insert(token.clone(), site);
            }
            st.saved.tokens.insert(key.to_string(), token);
            self.save(&st.saved);
        }
        tracing::info!(canvas = %key, "share link rotated");
        self.url(key)
    }

    pub fn tunnel_configured(&self) -> bool {
        self.config.tunnel_token.is_some()
    }

    fn base(&self) -> String {
        match &self.config.public_url {
            Some(u) => u.trim_end_matches('/').to_string(),
            None => format!("http://{}", self.addr),
        }
    }

    fn save(&self, saved: &Saved) {
        let text = serde_json::to_string_pretty(saved).unwrap_or_default();
        let tmp = self.state_path.with_extension("json.tmp");
        let wrote = std::fs::write(&tmp, text).and_then(|()| {
            restrict(&tmp);
            std::fs::rename(&tmp, &self.state_path)
        });
        if let Err(e) = wrote {
            tracing::warn!(error = %e, path = %self.state_path.display(), "save share state");
        }
    }
}

/// Owner-only: tokens are secrets.
fn restrict(path: &Path) {
    use std::os::unix::fs::PermissionsExt;
    let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600));
}

/// 128 random bits as 26 lowercase base32 characters.
fn new_token() -> String {
    let mut bytes = [0u8; 16];
    std::fs::File::open("/dev/urandom")
        .and_then(|mut f| f.read_exact(&mut bytes))
        .expect("read /dev/urandom");
    base32(&bytes)
}

fn base32(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 32] = b"abcdefghijklmnopqrstuvwxyz234567";
    let mut out = String::new();
    let (mut buf, mut bits) = (0u32, 0u32);
    for &b in bytes {
        buf = (buf << 8) | u32::from(b);
        bits += 8;
        while bits >= 5 {
            bits -= 5;
            out.push(ALPHABET[((buf >> bits) & 31) as usize] as char);
        }
    }
    if bits > 0 {
        out.push(ALPHABET[((buf << (5 - bits)) & 31) as usize] as char);
    }
    out
}

/// A token-shaped first path segment.
fn looks_like_token(s: &str) -> bool {
    s.len() == 26
        && s.bytes()
            .all(|b| b.is_ascii_lowercase() || (b'2'..=b'7').contains(&b))
}

/// `/<token>/<rest>` → (token, rest without its leading `/`, query kept off).
fn split_target(target: &str) -> Option<(&str, &str)> {
    let path = target.split(['?', '#']).next()?;
    let rest = path.strip_prefix('/')?;
    let (token, rest) = rest.split_once('/').unwrap_or((rest, ""));
    looks_like_token(token).then_some((token, rest))
}

/// The share token in a same-origin `Referer`.
fn referer_token(referer: &str) -> Option<&str> {
    let rest = referer.split_once("://").map_or(referer, |(_, r)| r);
    let path = &rest[rest.find('/')?..];
    split_target(path).map(|(t, _)| t)
}

fn handle(stream: TcpStream, state: &Arc<RwLock<State>>) -> io::Result<()> {
    let req = serve::read_request(&stream)?;
    let mut out = stream;
    if req.method != "GET" && req.method != "HEAD" {
        return serve::status(&mut out, 405, "Method Not Allowed");
    }
    let head_only = req.method == "HEAD";
    let Some((token, rest)) = split_target(&req.target) else {
        // `/fonts/x` from a shared page: put it under that page's token.
        let token = req.referer.as_deref().and_then(referer_token);
        let known = token.is_some_and(|t| state.read().is_ok_and(|s| s.sites.contains_key(t)));
        return match token.filter(|_| known) {
            Some(t) => redirect(&mut out, &format!("/{t}{}", req.target)),
            None => serve::status(&mut out, 404, "Not Found"),
        };
    };
    // The viewer's relative URLs need the trailing slash.
    if req.target.split(['?', '#']).next() == Some(&format!("/{token}")) {
        return redirect(&mut out, &format!("/{token}/"));
    }
    let (site, links, heights) = {
        let st = state
            .read()
            .map_err(|_| io::Error::other("share state poisoned"))?;
        let Some(site) = st.sites.get(token).cloned() else {
            return serve::status(&mut out, 404, "Not Found");
        };
        let links = st.saved.links.get(&site.key).cloned().unwrap_or_default();
        let heights = st.heights.get(&site.key).cloned().unwrap_or_default();
        (site, links, heights)
    };
    match rest {
        "" => respond(
            &mut out,
            head_only,
            "text/html; charset=utf-8",
            VIEWER.as_bytes(),
        ),
        "~canvas.json" => {
            let body = layout_json(&site, token, &heights).to_string();
            respond(&mut out, head_only, "application/json", body.as_bytes())
        }
        "~version" => {
            let newest =
                crate::watch::newest_input(&site.dir, &links.iter().cloned().collect::<Vec<_>>())
                    .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                    .map_or(0, |d| d.as_millis());
            respond(
                &mut out,
                head_only,
                "text/plain",
                newest.to_string().as_bytes(),
            )
        }
        _ => {
            let web_root = canvas::load(&site.dir).ok().and_then(|c| c.web_root);
            let raw = format!("/{rest}");
            let file = match serve::lookup(&site.project_root, web_root.as_deref(), &raw) {
                Ok(f) => f,
                Err(serve::Refusal::Bad) => return serve::status(&mut out, 400, "Bad Request"),
                Err(_) => return serve::status(&mut out, 404, "Not Found"),
            };
            if !allowed(&site, web_root.as_deref(), &links, &file) {
                return serve::status(&mut out, 404, "Not Found");
            }
            serve::send_file(&mut out, head_only, &file, req.range.as_deref(), PRIVATE)
        }
    }
}

/// Under a token, only this canvas's files: its folder, shared files in
/// `design/` outside every other canvas, its web root, and the outside
/// files its boards loaded. Compared as real paths, so a symlink cannot
/// reach out of them.
fn allowed(site: &Site, web_root: Option<&Path>, links: &BTreeSet<PathBuf>, file: &Path) -> bool {
    let real = |p: &Path| p.canonicalize().ok();
    let Some(file) = real(file) else {
        return false;
    };
    if real(&site.dir).is_some_and(|d| file.starts_with(d)) {
        return true;
    }
    if links.iter().filter_map(|l| real(l)).any(|l| l == file) {
        return true;
    }
    if web_root.and_then(real).is_some_and(|w| file.starts_with(w)) {
        return true;
    }
    let Some(design) = real(&canvas::design_dir(&site.project_root)) else {
        return false;
    };
    let Ok(rel) = file.strip_prefix(&design) else {
        return false;
    };
    // design/<x>/…: refused when <x> is another canvas.
    match rel.components().next() {
        Some(std::path::Component::Normal(first)) if rel.components().count() > 1 => {
            !design.join(first).join(canvas::CANVAS_FILE).is_file()
        }
        _ => true,
    }
}

/// The canvas as the viewer lays it out. Board `src` files become
/// token URLs; web `src` stays. An `auto` board carries the height Design
/// measured (`measured: true`); the viewer measures only the others.
fn layout_json(site: &Site, token: &str, heights: &HashMap<String, u32>) -> serde_json::Value {
    let c = match canvas::load(&site.dir) {
        Ok(c) => c,
        // The viewer sees the problem, not this machine's paths.
        Err(e) => {
            let e = e.replace(&format!("{}/", site.project_root.display()), "");
            return json!({"error": e, "artboards": []});
        }
    };
    let boards: Vec<serde_json::Value> = c
        .artboards
        .iter()
        .map(|a| {
            let cut = a.url.find(['?', '#']).unwrap_or(a.url.len());
            let (base, suffix) = a.url.split_at(cut);
            let src = canvas::file_url_path(base)
                .and_then(|f| {
                    f.strip_prefix(&site.project_root)
                        .ok()
                        .map(Path::to_path_buf)
                })
                .map(|rel| format!("/{token}/{}{suffix}", url_path(&rel)))
                .unwrap_or_else(|| a.url.clone());
            let measured = a.auto_height.then(|| heights.get(&a.id).copied()).flatten();
            json!({
                "id": a.id, "title": a.title, "src": src,
                "x": a.x, "y": a.y, "width": a.width,
                "height": measured.unwrap_or(a.height),
                "auto": a.auto_height, "measured": measured.is_some(),
            })
        })
        .collect();
    let a = &c.annot;
    let notes: Vec<serde_json::Value> = a
        .notes
        .iter()
        .map(|n| match &n.place {
            Place::Board { board, at } => json!({"n": n.n, "text": n.text, "board": board, "at": at}),
            Place::Point { x, y } => json!({"n": n.n, "text": n.text, "x": x, "y": y}),
        })
        .collect();
    json!({
        "title": c.title,
        "artboards": boards,
        "sections": a.sections.iter().map(|s| json!({"title": s.title, "note": s.note, "boards": s.boards})).collect::<Vec<_>>(),
        "links": a.links.iter().map(|l| json!({"from": l.from, "to": l.to, "label": l.label})).collect::<Vec<_>>(),
        "notes": notes,
        "flows": a.flows.iter().map(|f| json!({"id": f.id, "title": f.title, "start": f.start, "note": f.note})).collect::<Vec<_>>(),
    })
}

/// A relative path as URL path segments (percent-escaped).
fn url_path(rel: &Path) -> String {
    rel.components()
        .filter_map(|c| match c {
            std::path::Component::Normal(s) => Some(escape(&s.to_string_lossy())),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("/")
}

fn escape(seg: &str) -> String {
    let mut out = String::new();
    for b in seg.bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'~') {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

fn respond(out: &mut TcpStream, head_only: bool, kind: &str, body: &[u8]) -> io::Result<()> {
    write!(
        out,
        "HTTP/1.1 200 OK\r\nContent-Type: {kind}\r\nContent-Length: {}\r\n\
         Cache-Control: no-store\r\n{PRIVATE}Connection: close\r\n\r\n",
        body.len()
    )?;
    if !head_only {
        out.write_all(body)?;
    }
    out.flush()
}

fn redirect(out: &mut TcpStream, location: &str) -> io::Result<()> {
    write!(
        out,
        "HTTP/1.1 307 Temporary Redirect\r\nLocation: {location}\r\nContent-Length: 0\r\n\
         Cache-Control: no-store\r\n{PRIVATE}Connection: close\r\n\r\n"
    )?;
    out.flush()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tokens_are_128_bit_base32() {
        let t = new_token();
        assert_eq!(t.len(), 26);
        assert!(looks_like_token(&t), "{t}");
        assert_ne!(new_token(), t);
        assert_eq!(base32(&[0xff; 5]), "77777777");
    }

    #[test]
    fn targets_and_referers() {
        let t = "abcdefghijklmnopqrstuvwxyz";
        assert_eq!(split_target(&format!("/{t}/")), Some((t, "")));
        assert_eq!(split_target(&format!("/{t}")), Some((t, "")));
        assert_eq!(
            split_target(&format!("/{t}/design/a.html?x=1")),
            Some((t, "design/a.html"))
        );
        assert_eq!(split_target("/fonts/x.woff2"), None);
        assert_eq!(
            referer_token(&format!("https://design.example/{t}/design/a.html")),
            Some(t)
        );
        assert_eq!(referer_token("https://design.example/fonts/x.css"), None);
    }

    fn project(tag: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!("sola-share-{tag}-{}", std::process::id()));
        for d in ["design/home", "design/other", "design/assets", "web", "src"] {
            std::fs::create_dir_all(root.join(d)).unwrap();
        }
        std::fs::write(root.join("design/home/canvas.toml"), "").unwrap();
        std::fs::write(root.join("design/other/canvas.toml"), "").unwrap();
        for f in [
            "design/home/a.html",
            "design/other/b.html",
            "design/tokens.css",
            "design/assets/x.png",
            "web/f.woff2",
            "src/secret.rs",
            "src/app.css",
        ] {
            std::fs::write(root.join(f), "x").unwrap();
        }
        root
    }

    fn get(addr: SocketAddr, path: &str, extra: &str) -> String {
        let mut s = TcpStream::connect(addr).unwrap();
        write!(
            s,
            "GET {path} HTTP/1.1\r\nHost: design.example\r\n{extra}\r\n"
        )
        .unwrap();
        let mut out = String::new();
        s.read_to_string(&mut out).unwrap();
        out
    }

    #[test]
    fn serves_a_canvas_under_its_token() {
        let root = project("serve");
        std::fs::write(
            root.join("design/home/canvas.toml"),
            "title = \"Home\"\n[[artboard]]\nsrc = \"a.html?x=1\"\nwidth = 100\nheight = \"auto\"\n\
             [[note]]\nboard = \"a\"\nat = \".x\"\ntext = \"Look\"\n[[flow]]\ntitle = \"Walk\"\nstart = \"a\"\n",
        )
        .unwrap();
        let state = root.join("share.json");
        let config = ShareConfig {
            listen: "127.0.0.1:0".parse().unwrap(),
            ..ShareConfig::default()
        };
        let share = Share::start_with(config, state.clone()).unwrap();
        share.set_canvases([("p/home".to_string(), root.clone(), root.join("design/home"))]);
        share.set_links("p/home", [root.join("src/app.css")].into());
        let url = share.url("p/home").unwrap();
        let token = url
            .trim_end_matches('/')
            .rsplit('/')
            .next()
            .unwrap()
            .to_string();
        let addr = share.addr;

        assert!(get(addr, &format!("/{token}/"), "").contains("<title>Design</title>"));
        let bare = get(addr, &format!("/{token}"), "");
        assert!(
            bare.starts_with("HTTP/1.1 307") && bare.contains(&format!("Location: /{token}/")),
            "{bare}"
        );
        let layout = get(addr, &format!("/{token}/~canvas.json"), "");
        assert!(layout.contains(r#""notes":[{"at":".x","board":"a","n":1,"text":"Look"}]"#), "{layout}");
        assert!(layout.contains(r#""flows":[{"id":"walk","note":null,"start":"a","title":"Walk"}]"#), "{layout}");
        assert!(
            layout.contains(&format!("\"src\":\"/{token}/design/home/a.html?x=1\"")),
            "{layout}"
        );
        assert!(layout.contains("\"auto\":true"), "{layout}");
        assert!(layout.contains("\"measured\":false"), "{layout}");
        share.set_height("p/home", "a", 649);
        let layout = get(addr, &format!("/{token}/~canvas.json"), "");
        assert!(layout.contains("\"height\":649") && layout.contains("\"measured\":true"), "{layout}");
        let page = get(addr, &format!("/{token}/design/home/a.html"), "");
        assert!(
            page.starts_with("HTTP/1.1 200") && page.contains("Referrer-Policy: same-origin"),
            "{page}"
        );
        assert!(get(addr, &format!("/{token}/src/app.css"), "").starts_with("HTTP/1.1 200"));
        assert!(get(addr, &format!("/{token}/src/secret.rs"), "").starts_with("HTTP/1.1 404"));
        assert!(
            get(addr, &format!("/{token}/design/other/b.html"), "").starts_with("HTTP/1.1 404")
        );
        assert!(
            get(addr, "/aaaaaaaaaaaaaaaaaaaaaaaaaa/design/home/a.html", "")
                .starts_with("HTTP/1.1 404")
        );
        // `/src/app.css` from a shared page goes under its token.
        let referer = format!("Referer: https://design.example/{token}/design/home/a.html\r\n");
        let moved = get(addr, "/src/app.css", &referer);
        assert!(
            moved.contains(&format!("Location: /{token}/src/app.css")),
            "{moved}"
        );
        assert!(get(addr, "/src/app.css", "").starts_with("HTTP/1.1 404"));

        // Rotating kills the old link; tokens survive a restart.
        let new_url = share.rotate("p/home").unwrap();
        assert_ne!(new_url, url);
        assert!(get(addr, &format!("/{token}/"), "").starts_with("HTTP/1.1 404"));
        let saved = std::fs::read_to_string(&state).unwrap();
        assert!(saved.contains(new_url.trim_end_matches('/').rsplit('/').next().unwrap()));
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn a_token_reaches_its_canvas_only() {
        let root = project("allow");
        let site = Site {
            key: "p/home".into(),
            project_root: root.clone(),
            dir: root.join("design/home"),
        };
        let links: BTreeSet<PathBuf> = [root.join("src/app.css")].into();
        let ok = |f: &str, web: Option<&Path>| allowed(&site, web, &links, &root.join(f));
        assert!(ok("design/home/a.html", None));
        assert!(ok("design/tokens.css", None), "shared design file");
        assert!(ok("design/assets/x.png", None), "shared design folder");
        assert!(!ok("design/other/b.html", None), "another canvas");
        assert!(ok("src/app.css", None), "a file the boards loaded");
        assert!(!ok("src/secret.rs", None), "the rest of the project");
        assert!(!ok("web/f.woff2", None));
        assert!(
            ok("web/f.woff2", Some(&root.join("web"))),
            "the canvas web root"
        );
        std::os::unix::fs::symlink(root.join("src/secret.rs"), root.join("design/home/leak.rs"))
            .unwrap();
        assert!(
            !ok("design/home/leak.rs", None),
            "a symlink out of the canvas"
        );
        let _ = std::fs::remove_dir_all(root);
    }
}
