//! INBOX IDLE watcher on its own connection.

use std::net::{Shutdown, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::Duration;

use imap::extensions::idle::{SetReadTimeout, WaitOutcome};
use tracing::{debug, warn};

use super::account::Account;
use super::imap::{TlsStream, open_tls};

/// Re-SELECT at least this often even when the server stays quiet: some
/// servers never push EXPUNGE for another session's MOVE.
const WAIT: Duration = Duration::from_secs(30);
const RETRY: Duration = Duration::from_secs(10);

/// Newtype so we can implement SetReadTimeout (orphan rule).
struct IdleTlsStream(TlsStream);

impl std::io::Read for IdleTlsStream {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        self.0.read(buf)
    }
}

impl std::io::Write for IdleTlsStream {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.0.write(buf)
    }
    fn flush(&mut self) -> std::io::Result<()> {
        self.0.flush()
    }
}

impl SetReadTimeout for IdleTlsStream {
    fn set_read_timeout(&mut self, timeout: Option<Duration>) -> imap::Result<()> {
        self.0
            .sock
            .set_read_timeout(timeout)
            .map_err(imap::Error::Io)
    }
}

/// What changed on INBOX.
#[derive(Debug, Clone, Copy)]
pub enum IdleChange {
    /// EXISTS increased.
    Arrived { new_count: u32 },
    /// EXISTS decreased (expunge / move from another client).
    Removed,
    /// The watcher reconnected after an error; mail may have arrived
    /// while it was down.
    Reconnected,
}

/// Running watcher. [`IdleHandle::stop`] returns promptly: it shuts the
/// socket down under the blocked IDLE wait instead of waiting it out.
pub struct IdleHandle {
    stop: Arc<AtomicBool>,
    socket: Arc<Mutex<Option<TcpStream>>>,
    thread: Option<JoinHandle<()>>,
}

impl IdleHandle {
    pub fn stop(mut self) {
        self.signal();
        if let Some(handle) = self.thread.take() {
            let _ = handle.join();
        }
    }

    fn signal(&self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(sock) = self
            .socket
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .as_ref()
        {
            let _ = sock.shutdown(Shutdown::Both);
        }
    }
}

impl Drop for IdleHandle {
    fn drop(&mut self) {
        self.signal();
    }
}

/// Watch INBOX on a dedicated connection. On any wake *or* a [`WAIT`]
/// timeout, re-SELECTs and reports EXISTS changes. Reconnects on errors.
pub fn start_idle<F>(account: Account, on_change: F) -> IdleHandle
where
    F: Fn(IdleChange) + Send + 'static,
{
    let stop = Arc::new(AtomicBool::new(false));
    let socket = Arc::new(Mutex::new(None));
    let (flag, sock) = (Arc::clone(&stop), Arc::clone(&socket));
    let id = account.id();
    let thread = std::thread::Builder::new()
        .name(format!("sola-mail-idle-{id}"))
        .spawn(move || {
            let mut first = true;
            while !flag.load(Ordering::Relaxed) {
                let result = run(&account, &on_change, &flag, &sock, first);
                if flag.load(Ordering::Relaxed) {
                    break;
                }
                if let Err(e) = result {
                    warn!(account = id.as_str(), "IDLE: {e}; reconnecting in 10s");
                }
                first = false;
                let mut waited = Duration::ZERO;
                while waited < RETRY && !flag.load(Ordering::Relaxed) {
                    std::thread::sleep(Duration::from_millis(100));
                    waited += Duration::from_millis(100);
                }
            }
            debug!(account = id.as_str(), "IDLE watcher stopped");
        })
        .expect("spawn IDLE watcher");
    IdleHandle {
        stop,
        socket,
        thread: Some(thread),
    }
}

fn run<F: Fn(IdleChange)>(
    account: &Account,
    on_change: &F,
    stop: &AtomicBool,
    socket: &Mutex<Option<TcpStream>>,
    first: bool,
) -> anyhow::Result<()> {
    let tls = open_tls(account)?;
    *socket.lock().unwrap_or_else(|e| e.into_inner()) = tls.sock.try_clone().ok();
    let mut session = imap::Client::new(IdleTlsStream(tls))
        .login(&account.username, &account.password)
        .map_err(|(e, _)| anyhow::anyhow!("login failed: {e}"))?;

    let mut last_exists = session.select("INBOX")?.exists;
    debug!(
        account = account.id().as_str(),
        last_exists, "IDLE watching INBOX"
    );
    if !first {
        on_change(IdleChange::Reconnected);
    }

    while !stop.load(Ordering::Relaxed) {
        match session.idle()?.wait_with_timeout(WAIT)? {
            WaitOutcome::TimedOut => {}
            WaitOutcome::MailboxChanged => debug!("IDLE woke: mailbox changed"),
        }
        if stop.load(Ordering::Relaxed) {
            break;
        }
        let exists = session.select("INBOX")?.exists;
        if exists > last_exists {
            on_change(IdleChange::Arrived {
                new_count: exists - last_exists,
            });
        } else if exists < last_exists {
            on_change(IdleChange::Removed);
        }
        last_exists = exists;
    }
    let _ = session.logout();
    Ok(())
}
