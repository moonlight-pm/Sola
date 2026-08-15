//! Unix-socket server for `sat`. Runs on a thread; replies via channel.

use std::io::{Read, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::sync::{Mutex, OnceLock, mpsc};
use std::time::Duration;

use iced::Subscription;
use iced::futures::Stream;
use sola_agent_terminal::cli::{self, Request, Response};

#[derive(Clone)]
pub struct Incoming {
    pub req: Request,
    pub reply: mpsc::Sender<Response>,
}

impl std::fmt::Debug for Incoming {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Incoming").field("req", &self.req).finish()
    }
}

static TX: OnceLock<mpsc::Sender<Incoming>> = OnceLock::new();
static RX: Mutex<Option<mpsc::Receiver<Incoming>>> = Mutex::new(None);

fn ensure_channel() {
    TX.get_or_init(|| {
        let (tx, rx) = mpsc::channel();
        *RX.lock().unwrap() = Some(rx);
        tx
    });
}

pub fn start() {
    let path = cli::socket_path();
    let listener = match bind(&path) {
        Ok(l) => l,
        Err(e) => {
            tracing::warn!(path = %path.display(), "sat socket bind failed: {e}");
            return;
        }
    };
    ensure_channel();
    let tx = TX.get().unwrap().clone();
    std::thread::Builder::new()
        .name("sola-at-cli".into())
        .spawn(move || serve(listener, tx))
        .ok();
    tracing::info!(path = %path.display(), "sat socket listening");
}

fn bind(path: &std::path::Path) -> std::io::Result<UnixListener> {
    let _ = std::fs::remove_file(path);
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let listener = UnixListener::bind(path)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600));
    }
    Ok(listener)
}

fn serve(listener: UnixListener, tx: mpsc::Sender<Incoming>) {
    let _ = listener.set_nonblocking(false);
    for stream in listener.incoming() {
        let Ok(stream) = stream else {
            continue;
        };
        handle_one(stream, &tx);
    }
}

fn handle_one(mut stream: UnixStream, tx: &mpsc::Sender<Incoming>) {
    let _ = stream.set_read_timeout(Some(Duration::from_millis(2000)));
    let _ = stream.set_write_timeout(Some(Duration::from_millis(2000)));
    let mut buf = Vec::new();
    let mut tmp = [0u8; 4096];
    loop {
        match stream.read(&mut tmp) {
            Ok(0) => break,
            Ok(n) => {
                buf.extend_from_slice(&tmp[..n]);
                if buf.contains(&b'\n') || buf.len() > 256 * 1024 {
                    break;
                }
            }
            Err(_) => break,
        }
    }
    let req = match serde_json::from_slice::<Request>(buf.trim_ascii()) {
        Ok(r) => r,
        Err(e) => {
            let _ = write_response(&mut stream, &Response::err(format!("bad request: {e}")));
            return;
        }
    };
    let (rtx, rrx) = mpsc::channel();
    if tx.send(Incoming { req, reply: rtx }).is_err() {
        let _ = write_response(&mut stream, &Response::err("app is shutting down"));
        return;
    }
    let resp = rrx
        .recv_timeout(Duration::from_secs(6))
        .unwrap_or_else(|_| Response::err("app did not reply"));
    let _ = write_response(&mut stream, &resp);
}

fn write_response(stream: &mut UnixStream, resp: &Response) -> std::io::Result<()> {
    let body = serde_json::to_vec(resp)?;
    stream.write_all(&body)?;
    stream.write_all(b"\n")?;
    stream.flush()
}

pub fn subscription() -> Subscription<Incoming> {
    Subscription::run(cli_stream)
}

fn cli_stream() -> impl Stream<Item = Incoming> {
    ensure_channel();
    let rx_opt = RX.lock().unwrap().take();
    let (iced_tx, iced_rx) = iced::futures::channel::mpsc::unbounded();
    match rx_opt {
        Some(std_rx) => {
            std::thread::spawn(move || {
                while !iced_tx.is_closed() {
                    match std_rx.recv() {
                        Ok(ev) => {
                            if iced_tx.unbounded_send(ev).is_err() {
                                break;
                            }
                        }
                        Err(_) => break,
                    }
                }
            });
        }
        None => drop(iced_tx),
    }
    iced_rx
}
