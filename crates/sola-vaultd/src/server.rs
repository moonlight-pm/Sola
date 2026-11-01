//! The private socket (`$XDG_RUNTIME_DIR/sola-vault.sock`, `0600`): one
//! task per connection, JSON lines in and out. Each client is identified
//! once, when it connects (P3), and asks only what its role allows. After
//! `subscribe` the connection carries events.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use sola_vault_proto::{
    Envelope, ErrorCode, Event, FillMaterial, IdentityFill, Reply, Request, Response, Status, VERSION,
    VaultError,
};
use tokio::io::{AsyncBufRead, AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::net::unix::OwnedWriteHalf;
use tokio::net::{UnixListener, UnixStream};
use tokio::sync::{broadcast, mpsc, oneshot, watch};
use tracing::{debug, info, warn};
use zeroize::Zeroize;

use crate::actor::Input;
use crate::peer::{self, Peer};

/// A request line longer than this closes the connection.
const MAX_LINE: usize = 4 << 20;

pub struct Ctx {
    pub to_vault: mpsc::UnboundedSender<Input>,
    pub status: watch::Receiver<Status>,
    pub events: broadcast::Sender<Event>,
    pub roots: Vec<PathBuf>,
}

/// Is another daemon answering on `path`?
pub fn live(path: &Path) -> bool {
    std::os::unix::net::UnixStream::connect(path).is_ok()
}

/// Bind the socket (a stale one removed first), mode `0600`.
pub fn bind(path: &Path) -> std::io::Result<UnixListener> {
    use std::os::unix::fs::PermissionsExt;
    match std::fs::remove_file(path) {
        Ok(()) => info!(path = %path.display(), "vault: removed a stale socket"),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => return Err(e),
    }
    let listener = UnixListener::bind(path)?;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
    info!(path = %path.display(), "vault: listening");
    Ok(listener)
}

pub async fn serve(listener: UnixListener, ctx: Arc<Ctx>) {
    loop {
        match listener.accept().await {
            Ok((stream, _)) => {
                let ctx = ctx.clone();
                tokio::spawn(async move { connection(stream, ctx).await });
            }
            Err(e) => {
                warn!(error = %e, "vault: accept failed");
                tokio::time::sleep(std::time::Duration::from_millis(200)).await;
            }
        }
    }
}

async fn connection(stream: UnixStream, ctx: Arc<Ctx>) {
    let pid = match stream.peer_cred().map(|c| c.pid()) {
        Ok(Some(pid)) if pid > 0 => pid as u32,
        other => {
            warn!(?other, "vault: no peer pid; closing");
            return;
        }
    };
    let peer = peer::identify(pid, &ctx.roots);
    debug!(pid, exe = %peer.exe, role = ?peer.role, "vault: connection");
    let (read, mut write) = stream.into_split();
    let mut reader = BufReader::new(read);
    let mut line = Vec::new();
    let mut said_hello = false;
    loop {
        line.zeroize();
        line.clear();
        match read_line(&mut reader, &mut line).await {
            Ok(0) => return,
            Ok(_) => {}
            Err(e) => {
                warn!(pid, exe = %peer.exe, error = %e, "vault: connection read failed");
                return;
            }
        }
        let env: Envelope<Request> = match serde_json::from_slice(&line) {
            Ok(env) => env,
            Err(e) => {
                let bad = VaultError::new(ErrorCode::Invalid, format!("Not a request ({e})."));
                if send(&mut write, 0, Response::Err(bad)).await.is_err() {
                    return;
                }
                continue;
            }
        };
        line.zeroize();
        let id = env.id;
        let req = env.body;
        if !peer::allows(peer.role, &req) {
            warn!(pid, exe = %peer.exe, role = ?peer.role, request = method(&req), "vault: refused");
            let denied = VaultError::new(ErrorCode::Denied, "This program may not ask the vault that.");
            if send(&mut write, id, Response::Err(denied)).await.is_err() {
                return;
            }
            continue;
        }
        if !said_hello && !matches!(req, Request::Hello { .. }) {
            let first = VaultError::new(ErrorCode::Invalid, "Say hello first.");
            if send(&mut write, id, Response::Err(first)).await.is_err() {
                return;
            }
            continue;
        }
        let response = match req {
            Request::Hello { version, client } => {
                said_hello = true;
                debug!(pid, exe = %peer.exe, %client, version, "vault: hello");
                Response::Ok(Reply::Hello { version: VERSION })
            }
            Request::Status => Response::Ok(Reply::Status(ctx.status.borrow().clone())),
            Request::Subscribe => {
                if send(&mut write, id, Response::Ok(Reply::Done)).await.is_ok() {
                    subscription(reader, write, &ctx).await;
                }
                return;
            }
            req => ask_vault(&ctx, req, &peer).await,
        };
        if send(&mut write, id, response).await.is_err() {
            return;
        }
    }
}

async fn ask_vault(ctx: &Ctx, req: Request, peer: &Peer) -> Response {
    let (reply, answer) = oneshot::channel();
    let input = Input::Request {
        req,
        peer: peer.clone(),
        reply,
    };
    let gone = || Response::Err(VaultError::new(ErrorCode::Unavailable, "The vault stopped."));
    if ctx.to_vault.send(input).is_err() {
        return gone();
    }
    answer.await.unwrap_or_else(|_| gone())
}

/// Events until the client goes or the daemon stops: the status now,
/// then every change, and item changes.
async fn subscription(mut reader: BufReader<tokio::net::unix::OwnedReadHalf>, mut write: OwnedWriteHalf, ctx: &Ctx) {
    let mut status = ctx.status.clone();
    let mut events = ctx.events.subscribe();
    let now = Event::Status(status.borrow_and_update().clone());
    if send_event(&mut write, &now).await.is_err() {
        return;
    }
    let mut sink = [0u8; 256];
    loop {
        let event = tokio::select! {
            changed = status.changed() => match changed {
                Ok(()) => Event::Status(status.borrow_and_update().clone()),
                Err(_) => return,
            },
            event = events.recv() => match event {
                Ok(event) => event,
                // Too far behind to list them: say everything changed.
                Err(broadcast::error::RecvError::Lagged(_)) => Event::ItemsChanged { ids: Vec::new() },
                Err(broadcast::error::RecvError::Closed) => return,
            },
            read = reader.read(&mut sink) => match read {
                Ok(0) | Err(_) => return,
                Ok(_) => continue,
            },
        };
        if send_event(&mut write, &event).await.is_err() {
            return;
        }
    }
}

async fn send(write: &mut OwnedWriteHalf, id: u64, body: Response) -> std::io::Result<()> {
    let line = serde_json::to_vec(&Envelope { id, body: &body });
    scrub(body);
    let mut line = line.map_err(std::io::Error::other)?;
    line.push(b'\n');
    let sent = write.write_all(&line).await;
    line.zeroize();
    sent
}

async fn send_event(write: &mut OwnedWriteHalf, event: &Event) -> std::io::Result<()> {
    let mut line = serde_json::to_vec(event).map_err(std::io::Error::other)?;
    line.push(b'\n');
    write.write_all(&line).await
}

/// Clear the secrets a reply carries once it is on the wire. (Records
/// and drafts clear themselves when dropped.)
fn scrub(response: Response) {
    let Response::Ok(reply) = response else { return };
    match reply {
        Reply::Value(mut s) | Reply::Generated(mut s) | Reply::Exported { data: mut s } => s.zeroize(),
        Reply::Code { mut code, .. } => code.zeroize(),
        Reply::Fill(material) => scrub_fill(material),
        _ => {}
    }
}

fn scrub_fill(material: FillMaterial) {
    let wipe = |s: Option<String>| {
        if let Some(mut s) = s {
            s.zeroize();
        }
    };
    match material {
        FillMaterial::Login {
            username,
            password,
            code,
            ..
        } => [username, password, code].into_iter().for_each(wipe),
        FillMaterial::Card { number, code, .. } => [number, code].into_iter().for_each(wipe),
        FillMaterial::Identity(IdentityFill {
            ssn,
            passport_number,
            license_number,
            ..
        }) => [ssn, passport_number, license_number].into_iter().for_each(wipe),
    }
}

/// One line (without its newline) into `buf`; 0 at the end. Refuses a
/// line longer than [`MAX_LINE`].
async fn read_line<R: AsyncBufRead + Unpin>(reader: &mut R, buf: &mut Vec<u8>) -> std::io::Result<usize> {
    loop {
        let chunk = reader.fill_buf().await?;
        if chunk.is_empty() {
            return Ok(buf.len());
        }
        let (take, done) = match chunk.iter().position(|b| *b == b'\n') {
            Some(i) => (i + 1, true),
            None => (chunk.len(), false),
        };
        buf.extend_from_slice(&chunk[..take]);
        reader.consume(take);
        if buf.len() > MAX_LINE {
            buf.zeroize();
            return Err(std::io::Error::other("request too long"));
        }
        if done {
            buf.pop();
            // A blank line is not an end.
            if buf.is_empty() {
                continue;
            }
            return Ok(buf.len());
        }
    }
}

/// A request's method, for the log (never its parameters).
fn method(req: &Request) -> &'static str {
    match req {
        Request::Hello { .. } => "hello",
        Request::Status => "status",
        Request::Subscribe => "subscribe",
        Request::Unlock { .. } => "unlock",
        Request::Login { .. } => "login",
        Request::List { .. } => "list",
        Request::Matches { .. } => "matches",
        Request::Get { .. } => "get",
        Request::Code { .. } => "code",
        Request::FillMaterial { .. } => "fill_material",
        Request::Fields { .. } => "fields",
        Request::Deliver { .. } => "deliver",
        Request::Redeem { .. } => "redeem",
        Request::Export { .. } => "export",
        _ => "other",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn lines_are_read_whole_and_capped() {
        let mut r = BufReader::new(&b"{\"a\":1}\n\n{\"b\":2}"[..]);
        let mut buf = Vec::new();
        assert_eq!(read_line(&mut r, &mut buf).await.unwrap(), 7);
        assert_eq!(buf, b"{\"a\":1}");
        buf.clear();
        assert_eq!(read_line(&mut r, &mut buf).await.unwrap(), 7);
        assert_eq!(buf, b"{\"b\":2}");
        buf.clear();
        assert_eq!(read_line(&mut r, &mut buf).await.unwrap(), 0);

        let long = vec![b'x'; MAX_LINE + 10];
        let mut r = BufReader::new(&long[..]);
        assert!(read_line(&mut r, &mut Vec::new()).await.is_err());
    }

    #[test]
    fn the_log_names_the_method_only() {
        let req = Request::Unlock {
            password: "hunter2".into(),
        };
        assert_eq!(method(&req), "unlock");
    }
}
