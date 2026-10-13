//! Loopback http origin for boards (D5, 2026-10-12).
//!
//! Boards load from `http://<board>--<canvas>.<project>.localhost:<port>/…`
//! instead of `file://`. The project root is `/`, so root-absolute URLs,
//! fetch, ES modules, and iframes behave as on a real site, and every board
//! has its own host — Chromium keeps page zoom per host, so each board gets
//! its own raster scale.
//!
//! Bound to 127.0.0.1 on a random port. GET / HEAD only; files under a
//! registered project root; no dotfile segments (`.git`, `.env`); no
//! directory listing (`index.html` only). The `Host` must name a registered
//! project, which also stops DNS rebinding; no CORS header, so other origins
//! cannot read responses.

use std::collections::HashMap;
use std::io::{self, BufRead, BufReader, Read, Write};
use std::net::{Ipv4Addr, TcpListener, TcpStream};
use std::path::{Component, Path, PathBuf};
use std::sync::{Arc, RwLock};
use std::time::Duration;

/// Longest request head we read.
const MAX_HEAD: usize = 16 * 1024;
const READ_TIMEOUT: Duration = Duration::from_secs(10);

/// Project label (`<project>` in the host) → project root.
type Roots = Arc<RwLock<HashMap<String, PathBuf>>>;

pub struct Server {
    port: u16,
    roots: Roots,
}

impl Server {
    pub fn start() -> io::Result<Self> {
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0))?;
        let port = listener.local_addr()?.port();
        let roots: Roots = Arc::default();
        let shared = roots.clone();
        std::thread::Builder::new()
            .name("design-serve".into())
            .spawn(move || {
                for stream in listener.incoming().flatten() {
                    let roots = shared.clone();
                    std::thread::spawn(move || {
                        if let Err(e) = handle(stream, &roots) {
                            tracing::debug!(error = %e, "design serve connection");
                        }
                    });
                }
            })?;
        tracing::info!(port, "design origin listening on 127.0.0.1");
        Ok(Self { port, roots })
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
    /// `board` of `canvas` in `project`. `None` when `file` is outside `root`
    /// (the board stays on `file://`).
    pub fn url(
        &self,
        project: &str,
        root: &Path,
        file: &Path,
        suffix: &str,
        board: &str,
        canvas: &str,
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
        let host = format!("{}.{}.localhost", dns_label(&format!("{board}--{canvas}")), dns_label(project));
        Some(format!("http://{host}:{}{path}{suffix}", self.port))
    }

    /// The file a URL of this origin names (`None` for anything else).
    pub fn local_path(&self, url: &str) -> Option<PathBuf> {
        let rest = url.strip_prefix("http://")?;
        let (authority, path) = rest.split_at(rest.find('/')?);
        let (host, port) = authority.rsplit_once(':')?;
        if port.parse::<u16>().ok()? != self.port {
            return None;
        }
        let root = project_root(&self.roots, host)?;
        resolve(&root, path).ok()
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

/// `<board>.<project>.localhost[:port]` → that project's root.
fn project_root(roots: &Roots, host: &str) -> Option<PathBuf> {
    let host = host.split(':').next()?.to_ascii_lowercase();
    let labels: Vec<&str> = host.split('.').collect();
    let n = labels.len();
    if n < 3 || labels[n - 1] != "localhost" {
        return None;
    }
    roots.read().ok()?.get(labels[n - 2]).cloned()
}

#[derive(Debug, PartialEq)]
enum Refusal {
    Bad,
    Hidden,
    Missing,
}

/// Request path (`/a/b.css?x`) → file under `root`. No `..`, no dotfile
/// segments, directories serve their `index.html`.
fn resolve(root: &Path, raw: &str) -> Result<PathBuf, Refusal> {
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

fn mime(path: &Path) -> &'static str {
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

fn handle(stream: TcpStream, roots: &Roots) -> io::Result<()> {
    stream.set_read_timeout(Some(READ_TIMEOUT))?;
    let mut reader = BufReader::new(stream.try_clone()?).take(MAX_HEAD as u64);
    let mut line = String::new();
    reader.read_line(&mut line)?;
    let mut parts = line.split_whitespace();
    let (method, target) = (parts.next().unwrap_or(""), parts.next().unwrap_or(""));
    let mut host = String::new();
    loop {
        let mut h = String::new();
        if reader.read_line(&mut h)? == 0 || h == "\r\n" || h == "\n" {
            break;
        }
        if let Some((k, v)) = h.split_once(':') {
            if k.eq_ignore_ascii_case("host") {
                host = v.trim().to_string();
            }
        }
    }
    let mut out = stream;
    if method != "GET" && method != "HEAD" {
        return status(&mut out, 405, "Method Not Allowed");
    }
    let Some(root) = project_root(roots, &host) else {
        return status(&mut out, 404, "Not Found");
    };
    let file = match resolve(&root, target) {
        Ok(f) => f,
        Err(Refusal::Bad) => return status(&mut out, 400, "Bad Request"),
        Err(Refusal::Hidden) => return status(&mut out, 403, "Forbidden"),
        Err(Refusal::Missing) => return status(&mut out, 404, "Not Found"),
    };
    let mut body = match std::fs::File::open(&file) {
        Ok(f) => f,
        Err(_) => return status(&mut out, 404, "Not Found"),
    };
    let len = body.metadata()?.len();
    write!(
        out,
        "HTTP/1.1 200 OK\r\nContent-Type: {}\r\nContent-Length: {len}\r\n\
         Cache-Control: no-store\r\nConnection: close\r\n\r\n",
        mime(&file)
    )?;
    if method == "GET" {
        io::copy(&mut body, &mut out)?;
    }
    out.flush()
}

fn status(out: &mut TcpStream, code: u16, reason: &str) -> io::Result<()> {
    write!(
        out,
        "HTTP/1.1 {code} {reason}\r\nContent-Type: text/plain; charset=utf-8\r\n\
         Content-Length: {}\r\nConnection: close\r\n\r\n{reason}\n",
        reason.len() + 1
    )?;
    out.flush()
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
        assert!(server.url("proj-x", &root, Path::new("/elsewhere/a.html"), "", "b", "c").is_none());
        let _ = std::fs::remove_dir_all(root);
    }
}
