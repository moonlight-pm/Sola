use std::net::TcpStream;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::JoinHandle;
use std::time::Duration;

use imap::extensions::idle::SetReadTimeout;
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

/// Start a background IDLE watcher on INBOX.
///
/// Opens a separate IMAP connection and enters IDLE mode.
/// When new mail arrives (EXISTS response), calls `on_new` with the new message count
/// and a mutable reference to an `ImapClient` that can be used for mail operations
/// (e.g., listing messages, moving them per rules).
/// Reconnects automatically on errors.
pub fn start_idle<F>(config: Account, on_new: F) -> IdleHandle
where
    F: Fn(u32, &mut ImapClient) + Send + 'static,
{
    let stop_flag = Arc::new(AtomicBool::new(false));
    let flag = stop_flag.clone();

    let thread = std::thread::spawn(move || {
        while !flag.load(Ordering::Relaxed) {
            match run_idle_loop(&config, &on_new, &flag) {
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
    on_new: &F,
    stop_flag: &Arc<AtomicBool>,
) -> anyhow::Result<()>
where
    F: Fn(u32, &mut ImapClient),
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
        on_new(scan_count, &mut ops_client);
        // Re-select to get accurate count after any moves
        let updated = session.select("INBOX")?;
        last_exists = updated.exists;
    }

    while !stop_flag.load(Ordering::Relaxed) {
        // Enter IDLE mode with a timeout (29 minutes per RFC, we use 5 minutes)
        let mut idle = session.idle()?;
        idle.set_keepalive(Duration::from_secs(300));

        match idle.wait_keepalive() {
            Ok(reason) => {
                debug!("IDLE woke: {reason:?}");
            }
            Err(e) => {
                error!("IDLE wait error: {e}");
                return Err(anyhow::anyhow!("IDLE wait: {e}"));
            }
        }

        // Check if new messages arrived
        let mailbox = session.select("INBOX")?;
        if mailbox.exists > last_exists {
            let new_count = mailbox.exists - last_exists;
            debug!("IDLE: {new_count} new messages in INBOX");
            on_new(new_count, &mut ops_client);
            // Re-select to get accurate count after moves by ops_client
            let updated = session.select("INBOX")?;
            last_exists = updated.exists;
        } else {
            last_exists = mailbox.exists;
        }
    }

    session.logout()?;
    Ok(())
}
