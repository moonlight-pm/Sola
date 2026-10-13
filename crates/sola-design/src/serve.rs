//! Loopback http origin for boards (D5, 2026-10-12).
//!
//! Boards load from `http://<board>--<canvas>.<project>.localhost:<port>/…`
//! instead of `file://`. The project root is `/`, so root-absolute URLs,
//! fetch, ES modules, and iframes behave as on a real site, and every board
//! has its own host — Chromium keeps page zoom per host, so each board gets
//! its own raster scale.
//!
//! A canvas can name a web root inside the project (`root` in
//! `canvas.toml`): its boards look a path up there first, then at the
//! project root — so a product's `/fonts/x.woff2` works while the boards'
//! own `/design/<canvas>/…` URLs still resolve.
//!
//! Bound to 127.0.0.1 on a random port. GET / HEAD only, single byte
//! ranges (media seeking); files under a registered project root; no
//! dotfile segments (`.git`, `.env`); no directory listing (`index.html`
//! only). The `Host` must name a registered project, which also stops DNS
//! rebinding; no CORS header, so other origins cannot read responses.

use std::collections::HashMap;
use std::io::{self, BufRead, BufReader, Read, Seek, SeekFrom, Write};
use std::net::{Ipv4Addr, TcpListener, TcpStream};
use std::path::{Component, Path, PathBuf};
use std::sync::{Arc, RwLock};
use std::time::Duration;

/// Longest request head we read.
const MAX_HEAD: usize = 16 * 1024;
const READ_TIMEOUT: Duration = Duration::from_secs(10);

/// Project label (`<project>` in the host) → project root.
type Roots = Arc<RwLock<HashMap<String, PathBuf>>>;
/// Board host (`<board>--<canvas>.<project>`, no `.localhost`) → its
/// canvas's web root.
type WebRoots = Arc<RwLock<HashMap<String, PathBuf>>>;

pub struct Server {
    port: u16,
    roots: Roots,
    web_roots: WebRoots,
}

impl Server {
    pub fn start() -> io::Result<Self> {
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0))?;
        let port = listener.local_addr()?.port();
        let roots: Roots = Arc::default();
        let web_roots: WebRoots = Arc::default();
        let (shared, shared_web) = (roots.clone(), web_roots.clone());
        std::thread::Builder::new()
            .name("design-serve".into())
            .spawn(move || {
                for stream in listener.incoming().flatten() {
                    let roots = shared.clone();
                    let web_roots = shared_web.clone();
                    std::thread::spawn(move || {
                        if let Err(e) = handle(stream, &roots, &web_roots) {
                            tracing::debug!(error = %e, "design serve connection");
                        }
                    });
                }
            })?;
        tracing::info!(port, "design origin listening on 127.0.0.1");
        Ok(Self {
            port,
            roots,
            web_roots,
        })
    }

    /// Replace the served projects (`id → root`).
    pub fn set_projects<'a>(&self, projects: impl IntoIterator<Item = (&'a str, &'a Path)>) {
        let map = projects
            .into_iter()
            .map(|(id, root)| (dns_label(id), root.to_path_buf()))
            .collect();
        if let Ok(mut roots) = self.roots.write() {
            *roots = map;
        }
    }

    /// Board URL for `file` (with its `?query#fragment` suffix) shown as
    /// `board` of `canvas` in `project`, whose root-absolute paths look in
    /// `web_root` first. `None` when `file` is outside `root` (the board
    /// stays on `file://`).
    #[allow(clippy::too_many_arguments)]
    pub fn url(
        &self,
        project: &str,
        root: &Path,
        file: &Path,
        suffix: &str,
        board: &str,
        canvas: &str,
        web_root: Option<&Path>,
    ) -> Option<String> {
        let rel = file.strip_prefix(root).ok()?;
        let mut path = String::new();
        for part in rel.components() {
            let Component::Normal(seg) = part else {
                return None;
            };
            path.push('/');
            path.push_str(&escape(&seg.to_string_lossy()));
        }
        let site = format!("{}.{}", dns_label(&format!("{board}--{canvas}")), dns_label(project));
        if let Ok(mut map) = self.web_roots.write() {
            match web_root {
                Some(dir) => map.insert(site.clone(), dir.to_path_buf()),
                None => map.remove(&site),
            };
        }
        Some(format!("http://{site}.localhost:{}{path}{suffix}", self.port))
    }

    /// The file a URL of this origin names (`None` for anything else).
    pub fn local_path(&self, url: &str) -> Option<PathBuf> {
        let rest = url.strip_prefix("http://")?;
        let (authority, path) = rest.split_at(rest.find('/')?);
        let (host, port) = authority.rsplit_once(':')?;
        if port.parse::<u16>().ok()? != self.port {
            return None;
        }
        let (root, web_root) = roots_for(&self.roots, &self.web_roots, host)?;
        lookup(&root, web_root.as_deref(), path).ok()
    }
}

/// Lowercase ASCII letters / digits / `-`, ≤ 63 bytes: one DNS label.
pub fn dns_label(s: &str) -> String {
    let mut out: String = s
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c.to_ascii_lowercase() } else { '-' })
        .collect();
    out.truncate(63);
    let trimmed = out.trim_matches('-');
    if trimmed.is_empty() { "x".into() } else { trimmed.to_string() }
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

fn unescape(s: &str) -> Option<String> {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' {
            let hex = s.get(i + 1..i + 3)?;
            out.push(u8::from_str_radix(hex, 16).ok()?);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8(out).ok()
}

/// `<board>.<project>.localhost[:port]` → that project's root, and the
/// board's canvas web root if it has one.
fn roots_for(roots: &Roots, web_roots: &WebRoots, host: &str) -> Option<(PathBuf, Option<PathBuf>)> {
    let host = host.split(':').next()?.to_ascii_lowercase();
    let labels: Vec<&str> = host.split('.').collect();
    let n = labels.len();
    if n < 3 || labels[n - 1] != "localhost" {
        return None;
    }
    let root = roots.read().ok()?.get(labels[n - 2]).cloned()?;
    let site = format!("{}.{}", labels[n - 3], labels[n - 2]);
    let web_root = web_roots.read().ok()?.get(&site).cloned();
    Some((root, web_root))
}

/// [`resolve`] in the canvas web root, then in the project root.
pub(crate) fn lookup(root: &Path, web_root: Option<&Path>, raw: &str) -> Result<PathBuf, Refusal> {
    match web_root.map(|w| resolve(w, raw)) {
        Some(Ok(file)) => Ok(file),
        Some(Err(Refusal::Missing)) | None => resolve(root, raw),
        Some(Err(refused)) => Err(refused),
    }
}

#[derive(Debug, PartialEq)]
pub(crate) enum Refusal {
    Bad,
    Hidden,
    Missing,
}

/// Request path (`/a/b.css?x`) → file under `root`. No `..`, no dotfile
/// segments, directories serve their `index.html`.
pub(crate) fn resolve(root: &Path, raw: &str) -> Result<PathBuf, Refusal> {
    let path = raw.split(['?', '#']).next().unwrap_or_default();
    let path = unescape(path).ok_or(Refusal::Bad)?;
    if !path.starts_with('/') {
        return Err(Refusal::Bad);
    }
    let mut file = root.to_path_buf();
    for seg in path.split('/').filter(|s| !s.is_empty()) {
        if seg == ".." || seg == "." || seg.contains('\0') {
            return Err(Refusal::Bad);
        }
        if seg.starts_with('.') {
            return Err(Refusal::Hidden);
        }
        file.push(seg);
    }
    if file.is_dir() {
        file.push("index.html");
    }
    if file.is_file() { Ok(file) } else { Err(Refusal::Missing) }
}

pub(crate) fn mime(path: &Path) -> &'static str {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    match ext.as_str() {
        "html" | "htm" => "text/html; charset=utf-8",
        "css" => "text/css; charset=utf-8",
        "js" | "mjs" => "text/javascript; charset=utf-8",
        "json" | "map" => "application/json",
        "svg" => "image/svg+xml",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "avif" => "image/avif",
        "ico" => "image/x-icon",
        "woff" => "font/woff",
        "woff2" => "font/woff2",
        "ttf" => "font/ttf",
        "otf" => "font/otf",
        "wasm" => "application/wasm",
        "txt" | "md" => "text/plain; charset=utf-8",
        "xml" => "application/xml",
        "pdf" => "application/pdf",
        "mp4" => "video/mp4",
        "webm" => "video/webm",
        "mp3" => "audio/mpeg",
        "wav" => "audio/wav",
        _ => "application/octet-stream",
    }
}

/// `Range: bytes=…` against a `len`-byte file: `None` serves the whole file
/// (no header, a multi-range, or a unit we do not speak), `Some(Err(()))`
/// is unsatisfiable (416), else the inclusive byte span.
pub(crate) fn byte_range(header: &str, len: u64) -> Option<Result<(u64, u64), ()>> {
    let spec = header.trim().strip_prefix("bytes=")?;
    if spec.contains(',') {
        return None;
    }
    let (a, b) = spec.split_once('-')?;
    let (a, b) = (a.trim(), b.trim());
    let span = if a.is_empty() {
        let n: u64 = b.parse().ok()?;
        if n == 0 {
            return Some(Err(()));
        }
        (len.saturating_sub(n), len.checked_sub(1)?)
    } else {
        let start: u64 = a.parse().ok()?;
        let end = if b.is_empty() { len.saturating_sub(1) } else { b.parse::<u64>().ok()?.min(len.saturating_sub(1)) };
        (start, end)
    };
    Some(if span.0 < len && span.0 <= span.1 { Ok(span) } else { Err(()) })
}

fn handle(stream: TcpStream, roots: &Roots, web_roots: &WebRoots) -> io::Result<()> {
    let req = read_request(&stream)?;
    let (method, target, host, range) = (req.method.as_str(), req.target.as_str(), req.host, req.range);
    let mut out = stream;
    if method != "GET" && method != "HEAD" {
        return status(&mut out, 405, "Method Not Allowed");
    }
    let Some((root, web_root)) = roots_for(roots, web_roots, &host) else {
        return status(&mut out, 404, "Not Found");
    };
    let file = match lookup(&root, web_root.as_deref(), target) {
        Ok(f) => f,
        Err(Refusal::Bad) => return status(&mut out, 400, "Bad Request"),
        Err(Refusal::Hidden) => return status(&mut out, 403, "Forbidden"),
        Err(Refusal::Missing) => return status(&mut out, 404, "Not Found"),
    };
    send_file(&mut out, method == "HEAD", &file, range.as_deref(), "")
}

/// Answer with `file` (whole, or the byte range asked for). `extra` is more
/// header lines, each ending in `\r\n`.
pub(crate) fn send_file(
    out: &mut TcpStream,
    head_only: bool,
    file: &Path,
    range: Option<&str>,
    extra: &str,
) -> io::Result<()> {
    let mut body = match std::fs::File::open(file) {
        Ok(f) => f,
        Err(_) => return status(out, 404, "Not Found"),
    };
    let len = body.metadata()?.len();
    let kind = mime(file);
    match range.and_then(|r| byte_range(r, len)) {
        None => {
            write!(
                out,
                "HTTP/1.1 200 OK\r\nContent-Type: {kind}\r\nContent-Length: {len}\r\n\
                 Accept-Ranges: bytes\r\nCache-Control: no-store\r\n{extra}Connection: close\r\n\r\n"
            )?;
            if !head_only {
                io::copy(&mut body, out)?;
            }
        }
        Some(Ok((start, end))) => {
            let n = end - start + 1;
            write!(
                out,
                "HTTP/1.1 206 Partial Content\r\nContent-Type: {kind}\r\nContent-Length: {n}\r\n\
                 Content-Range: bytes {start}-{end}/{len}\r\nAccept-Ranges: bytes\r\n\
                 Cache-Control: no-store\r\n{extra}Connection: close\r\n\r\n"
            )?;
            if !head_only {
                body.seek(SeekFrom::Start(start))?;
                io::copy(&mut body.take(n), out)?;
            }
        }
        Some(Err(())) => {
            write!(
                out,
                "HTTP/1.1 416 Range Not Satisfiable\r\nContent-Range: bytes */{len}\r\n\
                 {extra}Content-Length: 0\r\nConnection: close\r\n\r\n"
            )?;
        }
    }
    out.flush()
}

pub(crate) fn status(out: &mut TcpStream, code: u16, reason: &str) -> io::Result<()> {
    write!(
        out,
        "HTTP/1.1 {code} {reason}\r\nContent-Type: text/plain; charset=utf-8\r\n\
         Content-Length: {}\r\nConnection: close\r\n\r\n{reason}\n",
        reason.len() + 1
    )?;
    out.flush()
}

/// Request line + the headers we read. `None` for a closed connection.
pub(crate) struct Request {
    pub method: String,
    pub target: String,
    pub host: String,
    pub range: Option<String>,
    pub referer: Option<String>,
}

/// Read one request head (at most [`MAX_HEAD`] bytes).
pub(crate) fn read_request(stream: &TcpStream) -> io::Result<Request> {
    stream.set_read_timeout(Some(READ_TIMEOUT))?;
    let mut reader = BufReader::new(stream.try_clone()?).take(MAX_HEAD as u64);
    let mut line = String::new();
    reader.read_line(&mut line)?;
    let mut parts = line.split_whitespace();
    let mut req = Request {
        method: parts.next().unwrap_or("").to_string(),
        target: parts.next().unwrap_or("").to_string(),
        host: String::new(),
        range: None,
        referer: None,
    };
    loop {
        let mut h = String::new();
        if reader.read_line(&mut h)? == 0 || h == "\r\n" || h == "\n" {
            break;
        }
        if let Some((k, v)) = h.split_once(':') {
            let v = v.trim().to_string();
            if k.eq_ignore_ascii_case("host") {
                req.host = v;
            } else if k.eq_ignore_ascii_case("range") {
                req.range = Some(v);
            } else if k.eq_ignore_ascii_case("referer") {
                req.referer = Some(v);
            }
        }
    }
    Ok(req)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_root(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "sola-design-serve-{tag}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(dir.join("design/home")).unwrap();
        std::fs::create_dir_all(dir.join(".git")).unwrap();
        std::fs::write(dir.join("design/home/a page.html"), "<p>hi</p>").unwrap();
        std::fs::write(dir.join("design/home/index.html"), "idx").unwrap();
        std::fs::write(dir.join(".git/config"), "secret").unwrap();
        dir
    }

    #[test]
    fn labels_are_dns_safe() {
        assert_eq!(dns_label("Light--Foundation"), "light--foundation");
        assert_eq!(dns_label("proj-exit"), "proj-exit");
        assert_eq!(dns_label("a b/c"), "a-b-c");
        assert_eq!(dns_label("--"), "x");
        assert_eq!(dns_label(&"z".repeat(80)).len(), 63);
    }

    #[test]
    fn resolve_refuses_traversal_and_dotfiles() {
        let root = temp_root("resolve");
        assert_eq!(
            resolve(&root, "/design/home/a%20page.html?x=1").unwrap(),
            root.join("design/home/a page.html")
        );
        assert_eq!(resolve(&root, "/design/home/").unwrap(), root.join("design/home/index.html"));
        assert_eq!(resolve(&root, "/design/../.git/config"), Err(Refusal::Bad));
        assert_eq!(resolve(&root, "/.git/config"), Err(Refusal::Hidden));
        assert_eq!(resolve(&root, "/nope.css"), Err(Refusal::Missing));
        assert_eq!(resolve(&root, "relative"), Err(Refusal::Bad));
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn byte_ranges() {
        assert_eq!(byte_range("bytes=0-99", 1000), Some(Ok((0, 99))));
        assert_eq!(byte_range("bytes=900-", 1000), Some(Ok((900, 999))));
        assert_eq!(byte_range("bytes=-100", 1000), Some(Ok((900, 999))));
        assert_eq!(byte_range("bytes=990-2000", 1000), Some(Ok((990, 999))));
        assert_eq!(byte_range("bytes=1000-", 1000), Some(Err(())));
        assert_eq!(byte_range("bytes=5-1", 1000), Some(Err(())));
        assert_eq!(byte_range("bytes=0-1,5-6", 1000), None);
        assert_eq!(byte_range("items=0-1", 1000), None);
    }

    fn get_raw(port: u16, host: &str, path: &str, extra: &str) -> String {
        let mut s = TcpStream::connect((Ipv4Addr::LOCALHOST, port)).unwrap();
        write!(s, "GET {path} HTTP/1.1\r\nHost: {host}\r\n{extra}\r\n").unwrap();
        let mut out = String::new();
        s.read_to_string(&mut out).unwrap();
        out
    }

    #[test]
    fn ranges_and_web_roots() {
        let root = temp_root("range");
        std::fs::create_dir_all(root.join("web/public/fonts")).unwrap();
        std::fs::write(root.join("web/public/fonts/a.txt"), "0123456789").unwrap();
        std::fs::write(root.join("web/public/shadow.txt"), "web").unwrap();
        std::fs::write(root.join("shadow.txt"), "project").unwrap();
        let server = Server::start().unwrap();
        server.set_projects([("proj-x", root.as_path())]);
        let page = root.join("design/home/index.html");
        let web = root.join("web/public");
        server.url("proj-x", &root, &page, "", "light", "home", Some(&web)).unwrap();
        server.url("proj-x", &root, &page, "", "plain", "home", None).unwrap();
        let light = format!("light--home.proj-x.localhost:{}", server.port);
        let plain = format!("plain--home.proj-x.localhost:{}", server.port);

        let part = get_raw(server.port, &light, "/fonts/a.txt", "Range: bytes=2-4\r\n");
        assert!(part.starts_with("HTTP/1.1 206"), "{part}");
        assert!(part.contains("Content-Range: bytes 2-4/10") && part.ends_with("234"), "{part}");
        let all = get_raw(server.port, &light, "/fonts/a.txt", "");
        assert!(all.contains("Accept-Ranges: bytes") && all.ends_with("0123456789"), "{all}");
        let bad = get_raw(server.port, &light, "/fonts/a.txt", "Range: bytes=50-\r\n");
        assert!(bad.starts_with("HTTP/1.1 416"), "{bad}");

        // The web root wins; the project root still serves the board.
        assert!(get_raw(server.port, &light, "/shadow.txt", "").ends_with("web"));
        assert!(get_raw(server.port, &plain, "/shadow.txt", "").ends_with("project"));
        assert!(get_raw(server.port, &light, "/design/home/", "").ends_with("idx"));
        assert!(get_raw(server.port, &plain, "/fonts/a.txt", "").starts_with("HTTP/1.1 404"));
        let url = format!("http://{light}/fonts/a.txt");
        assert_eq!(server.local_path(&url), Some(web.join("fonts/a.txt")));
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn serves_registered_hosts_only() {
        let root = temp_root("serve");
        let server = Server::start().unwrap();
        server.set_projects([("proj-x", root.as_path())]);
        let url = server
            .url(
                "proj-x",
                &root,
                &root.join("design/home/a page.html"),
                "?theme=dark",
                "light",
                "home",
                None,
            )
            .unwrap();
        assert!(url.starts_with("http://light--home.proj-x.localhost:"), "{url}");
        assert!(url.ends_with("/design/home/a%20page.html?theme=dark"), "{url}");
        assert_eq!(server.local_path(&url), Some(root.join("design/home/a page.html")));

        let get = |host: &str, path: &str| -> String {
            let mut s = TcpStream::connect((Ipv4Addr::LOCALHOST, server.port)).unwrap();
            write!(s, "GET {path} HTTP/1.1\r\nHost: {host}\r\n\r\n").unwrap();
            let mut out = String::new();
            s.read_to_string(&mut out).unwrap();
            out
        };
        let host = format!("light--home.proj-x.localhost:{}", server.port);
        let ok = get(&host, "/design/home/a%20page.html");
        assert!(ok.starts_with("HTTP/1.1 200"), "{ok}");
        assert!(ok.contains("text/html") && ok.ends_with("<p>hi</p>"), "{ok}");
        assert!(get(&host, "/.git/config").starts_with("HTTP/1.1 403"));
        assert!(get("evil.example:80", "/design/home/index.html").starts_with("HTTP/1.1 404"));
        assert!(get(&host, "/design/home/missing.css").starts_with("HTTP/1.1 404"));
        assert!(server.url("proj-x", &root, Path::new("/elsewhere/a.html"), "", "b", "c", None).is_none());
        let _ = std::fs::remove_dir_all(root);
    }
}
