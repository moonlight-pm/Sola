use std::net::TcpStream;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::JoinHandle;
use std::time::Duration;

use imap::extensions::idle::{SetReadTimeout, WaitOutcome};
use tracing::{debug, error, warn};

use super::account::Account;
use super::imap::ImapClient;

/// Newtype wrapper so we can implement SetReadTimeout (orphan rule).
struct IdleTlsStream(rustls_connector::TlsStream<TcpStream>);

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

/// Handle to a running IDLE watcher. Drop to stop.
pub struct IdleHandle {
    stop_flag: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl IdleHandle {
    /// Stop the IDLE watcher and wait for the thread to exit.
    pub fn stop(mut self) {
        self.stop_flag.store(true, Ordering::Relaxed);
        if let Some(handle) = self.thread.take() {
            let _ = handle.join();
        }
    }
}

impl Drop for IdleHandle {
    fn drop(&mut self) {
        self.stop_flag.store(true, Ordering::Relaxed);
    }
}

/// What changed on INBOX while IDLE was waiting.
#[derive(Debug, Clone, Copy)]
pub enum IdleChange {
    /// EXISTS increased — `new_count` is the approximate number of arrivals.
    Arrived { new_count: u32 },
    /// EXISTS decreased (expunge / delete from another client).
    Removed { gone: u32 },
    /// IDLE woke but EXISTS is unchanged (flags, etc.). Still worth a light refresh.
    Touched,
}

/// Start a background IDLE watcher on INBOX.
///
/// Opens a separate IMAP connection and enters IDLE mode. On any wake *or* a
/// 30s wait timeout, re-SELECTs INBOX and reports [`IdleChange`] so the UI can
/// refresh when *other* clients delete/expunge mail — not only when new mail
/// arrives. Timeout matters: some servers never push EXPUNGE for another
/// session's MOVE, and `wait_keepalive` would hide that by recycling IDLE.
///
/// Reconnects automatically on errors.
pub fn start_idle<F>(config: Account, on_change: F) -> IdleHandle
where
    F: Fn(IdleChange, &mut ImapClient) + Send + 'static,
{
    let stop_flag = Arc::new(AtomicBool::new(false));
    let flag = stop_flag.clone();

    let thread = std::thread::spawn(move || {
        while !flag.load(Ordering::Relaxed) {
            match run_idle_loop(&config, &on_change, &flag) {
                Ok(()) => break, // clean shutdown
                Err(e) => {
                    warn!("IDLE connection error: {e}, reconnecting in 10s...");
                    // Wait before reconnecting, checking stop flag
                    for _ in 0..100 {
                        if flag.load(Ordering::Relaxed) {
                            return;
                        }
                        std::thread::sleep(Duration::from_millis(100));
                    }
                }
            }
        }
        debug!("IDLE watcher stopped");
    });

    IdleHandle {
        stop_flag,
        thread: Some(thread),
    }
}

fn run_idle_loop<F>(
    config: &Account,
    on_change: &F,
    stop_flag: &Arc<AtomicBool>,
) -> anyhow::Result<()>
where
    F: Fn(IdleChange, &mut ImapClient),
{
    let addr = format!("{}:{}", config.imap_host, config.imap_port);
    let tcp = TcpStream::connect(&addr)
        .map_err(|e| anyhow::anyhow!("IDLE TCP connect to {addr} failed: {e}"))?;
    let connector = rustls_connector::RustlsConnector::new_with_native_certs()
        .map_err(|e| anyhow::anyhow!("TLS connector init failed: {e}"))?;
    let tls_stream = connector
        .connect(&config.imap_host, tcp)
        .map_err(|e| anyhow::anyhow!("IDLE TLS handshake failed: {e}"))?;
    let client = imap::Client::new(IdleTlsStream(tls_stream));

    let mut session = client
        .login(&config.username, &config.password)
        .map_err(|e| anyhow::anyhow!("IDLE login failed: {}", e.0))?;

    // Create a separate ImapClient for the callback to use for mail operations
    let mut ops_client = ImapClient::connect(config)
        .map_err(|e| anyhow::anyhow!("IDLE ops client connect failed: {e}"))?;

    let mailbox = session.select("INBOX")?;
    let mut last_exists = mailbox.exists;
    debug!("IDLE watcher connected, INBOX has {last_exists} messages");

    // Apply move rules to existing inbox messages on connect/reconnect.
    // IDLE only fires for NEW messages, so without this, messages that arrived
    // while disconnected (or before startup) would never have rules applied.
    if last_exists > 0 {
        // Cap initial scan to avoid slow startup on large mailboxes
        const MAX_INITIAL_SCAN: u32 = 500;
        let scan_count = last_exists.min(MAX_INITIAL_SCAN);
        debug!("IDLE: applying rules to {scan_count} existing INBOX messages");
        on_change(
            IdleChange::Arrived {
                new_count: scan_count,
            },
            &mut ops_client,
        );
        // Re-select to get accurate count after any moves
        let updated = session.select("INBOX")?;
        last_exists = updated.exists;
    }

    while !stop_flag.load(Ordering::Relaxed) {
        // Block on IDLE, but return on timeout so we re-SELECT even when the
        // server never pushes EXPUNGE for another client's MOVE (wait_keepalive
        // would recycle IDLE internally and never tell us).
        let idle = session.idle()?;
        match idle.wait_with_timeout(Duration::from_secs(30)) {
            Ok(WaitOutcome::TimedOut) => {
                debug!("IDLE wait timed out — re-SELECT to catch quiet expunge");
            }
            Ok(WaitOutcome::MailboxChanged) => {
                debug!("IDLE woke: mailbox changed");
            }
            Err(e) => {
                error!("IDLE wait error: {e}");
                return Err(anyhow::anyhow!("IDLE wait: {e}"));
            }
        }

        // Re-SELECT to observe EXISTS (covers arrivals *and* expunge/deletes
        // from other clients — the old code only handled increases).
        let mailbox = session.select("INBOX")?;
        let exists = mailbox.exists;
        if exists > last_exists {
            let new_count = exists - last_exists;
            debug!("IDLE: {new_count} new messages in INBOX ({last_exists} → {exists})");
            on_change(IdleChange::Arrived { new_count }, &mut ops_client);
            let updated = session.select("INBOX")?;
            last_exists = updated.exists;
        } else if exists < last_exists {
            let gone = last_exists - exists;
            debug!("IDLE: {gone} messages removed from INBOX ({last_exists} → {exists})");
            on_change(IdleChange::Removed { gone }, &mut ops_client);
            last_exists = exists;
        } else {
            // Same EXISTS: flag-only or a quiet timeout. Worker no-ops Touched.
            debug!("IDLE: INBOX unchanged at {exists}");
            on_change(IdleChange::Touched, &mut ops_client);
            last_exists = exists;
        }
    }

    session.logout()?;
    Ok(())
}
