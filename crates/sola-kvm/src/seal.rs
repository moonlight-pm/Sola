//! Paired KVM traffic (`sola-kvm pair`): every input datagram and clipboard
//! frame sealed with the pair key, XChaCha20-Poly1305.
//!
//! Envelope: `KVMS` | version 2 | channel | sender id (16) | counter (8, LE) |
//! time (8, LE, unix ms) | ciphertext and tag. The header is the associated
//! data; the nonce is the sender id and the counter, and each process takes a
//! random id, so nonces never repeat under one key. A receiver drops an
//! envelope that does not open, that is more than [`WINDOW`] from its own
//! clock, or whose counter it has already seen from that sender (a window of
//! 64 lets UDP reorder).
//!
//! Self-contained (no `crate::` paths): the Mac agent includes this file.

use std::collections::HashMap;
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use chacha20poly1305::aead::{Aead, KeyInit, Payload};
use chacha20poly1305::{XChaCha20Poly1305, XNonce};

pub const MAGIC: [u8; 4] = *b"KVMS";
pub const VERSION: u8 = 2;
const HEADER: usize = 4 + 1 + 1 + 16 + 8 + 8;
const TAG: usize = 16;
/// The most an envelope's clock may differ from ours.
pub const WINDOW: Duration = Duration::from_secs(30);
const REORDER: u64 = 64;

/// What an envelope carries; part of its header, so one cannot stand in for
/// the other.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum Channel {
    Input = 1,
    Clipboard = 2,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OpenError {
    Short,
    /// Not an envelope: an unpaired sender.
    NotSealed,
    WrongChannel,
    /// Its clock is this far from ours.
    Clock { skew_ms: i64 },
    /// Does not open with the pair key.
    Forged,
    Replay,
}

impl std::fmt::Display for OpenError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Short => write!(f, "too short to be sealed"),
            Self::NotSealed => write!(f, "not sealed (is the sender paired?)"),
            Self::WrongChannel => write!(f, "sealed for another channel"),
            Self::Clock { skew_ms } => {
                write!(f, "clock {skew_ms} ms from ours (check time sync on both)")
            }
            Self::Forged => write!(f, "does not open with the pair key"),
            Self::Replay => write!(f, "already seen (replay)"),
        }
    }
}

/// One channel's sealing and opening under the pair key.
pub struct Seal {
    cipher: XChaCha20Poly1305,
    channel: Channel,
    id: [u8; 16],
    counter: AtomicU64,
    seen: Mutex<HashMap<[u8; 16], Seen>>,
}

/// A sender's counters as received: the highest, and which of the
/// [`REORDER`] below it have come (bit `i` = `top - i`).
struct Seen {
    top: u64,
    mask: u64,
    at: Instant,
}

impl Seal {
    pub fn new(key: &[u8; 32], channel: Channel) -> Self {
        let mut id = [0u8; 16];
        fill_random(&mut id);
        Self {
            cipher: XChaCha20Poly1305::new(key.into()),
            channel,
            id,
            counter: AtomicU64::new(0),
            seen: Mutex::new(HashMap::new()),
        }
    }

    pub fn seal(&self, plain: &[u8]) -> Vec<u8> {
        self.seal_at(plain, now_ms())
    }

    fn seal_at(&self, plain: &[u8], time_ms: u64) -> Vec<u8> {
        let counter = self.counter.fetch_add(1, Ordering::Relaxed);
        let mut out = Vec::with_capacity(HEADER + plain.len() + TAG);
        out.extend_from_slice(&MAGIC);
        out.push(VERSION);
        out.push(self.channel as u8);
        out.extend_from_slice(&self.id);
        out.extend_from_slice(&counter.to_le_bytes());
        out.extend_from_slice(&time_ms.to_le_bytes());
        let sealed = self
            .cipher
            .encrypt(&nonce(&self.id, counter), Payload { msg: plain, aad: &out })
            .expect("xchacha20poly1305 seal");
        out.extend_from_slice(&sealed);
        out
    }

    pub fn open(&self, bytes: &[u8]) -> Result<Vec<u8>, OpenError> {
        self.open_at(bytes, now_ms())
    }

    fn open_at(&self, bytes: &[u8], now: u64) -> Result<Vec<u8>, OpenError> {
        if bytes.len() < 4 || bytes[..4] != MAGIC || bytes.get(4) != Some(&VERSION) {
            return Err(if bytes.len() < 4 {
                OpenError::Short
            } else {
                OpenError::NotSealed
            });
        }
        if bytes.len() < HEADER + TAG {
            return Err(OpenError::Short);
        }
        if bytes[5] != self.channel as u8 {
            return Err(OpenError::WrongChannel);
        }
        let id: [u8; 16] = bytes[6..22].try_into().expect("16");
        let counter = u64::from_le_bytes(bytes[22..30].try_into().expect("8"));
        let time_ms = u64::from_le_bytes(bytes[30..38].try_into().expect("8"));
        let skew_ms = time_ms as i64 - now as i64;
        if skew_ms.unsigned_abs() > WINDOW.as_millis() as u64 {
            return Err(OpenError::Clock { skew_ms });
        }
        let plain = self
            .cipher
            .decrypt(
                &nonce(&id, counter),
                Payload {
                    msg: &bytes[HEADER..],
                    aad: &bytes[..HEADER],
                },
            )
            .map_err(|_| OpenError::Forged)?;
        self.note(id, counter)?;
        Ok(plain)
    }

    /// Accept `counter` from `id` once.
    fn note(&self, id: [u8; 16], counter: u64) -> Result<(), OpenError> {
        let mut seen = self.seen.lock().unwrap_or_else(|e| e.into_inner());
        if seen.len() > 16 {
            seen.retain(|_, s| s.at.elapsed() < WINDOW * 2);
        }
        let now = Instant::now();
        let Some(s) = seen.get_mut(&id) else {
            seen.insert(
                id,
                Seen {
                    top: counter,
                    mask: 1,
                    at: now,
                },
            );
            return Ok(());
        };
        if counter > s.top {
            let ahead = counter - s.top;
            s.mask = if ahead >= REORDER { 0 } else { s.mask << ahead };
            s.mask |= 1;
            s.top = counter;
        } else {
            let behind = s.top - counter;
            if behind >= REORDER || s.mask & (1 << behind) != 0 {
                return Err(OpenError::Replay);
            }
            s.mask |= 1 << behind;
        }
        s.at = now;
        Ok(())
    }

    /// Write `plain` sealed as one frame on a stream (length, then envelope).
    pub fn write_frame(&self, w: &mut dyn Write, plain: &[u8]) -> io::Result<()> {
        let sealed = self.seal(plain);
        let mut frame = Vec::with_capacity(4 + sealed.len());
        frame.extend_from_slice(&(sealed.len() as u32).to_le_bytes());
        frame.extend_from_slice(&sealed);
        w.write_all(&frame)
    }

    /// Read one sealed frame of at most `max` plaintext bytes and open it.
    pub fn read_frame(&self, r: &mut dyn Read, max: u32) -> io::Result<Vec<u8>> {
        let mut len = [0u8; 4];
        r.read_exact(&mut len)?;
        let len = u32::from_le_bytes(len) as usize;
        if len > max as usize + HEADER + TAG + 64 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("sealed frame of {len} bytes is over the limit"),
            ));
        }
        let mut buf = vec![0u8; len];
        r.read_exact(&mut buf)?;
        self.open(&buf)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e.to_string()))
    }
}

fn nonce(id: &[u8; 16], counter: u64) -> XNonce {
    let mut n = [0u8; 24];
    n[..16].copy_from_slice(id);
    n[16..].copy_from_slice(&counter.to_le_bytes());
    n.into()
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

fn fill_random(buf: &mut [u8]) {
    getrandom::getrandom(buf).expect("getrandom");
}

// ── The pair key ─────────────────────────────────────────────────────────

/// `$XDG_CONFIG_HOME/sola-kvm/pair.key` or `~/.config/sola-kvm/pair.key`.
pub fn key_path() -> PathBuf {
    let dir = std::env::var_os("XDG_CONFIG_HOME")
        .filter(|d| !d.is_empty())
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))
        .unwrap_or_else(|| PathBuf::from("."));
    dir.join("sola-kvm").join("pair.key")
}

pub fn new_key() -> [u8; 32] {
    let mut key = [0u8; 32];
    fill_random(&mut key);
    key
}

/// The pair key at `path`; `None` when there is none (unpaired). A file
/// that is not a key is an error: the caller must not run unpaired then.
pub fn load_key(path: &Path) -> io::Result<Option<[u8; 32]>> {
    match std::fs::read_to_string(path) {
        Ok(text) => parse_key(&text).map(Some).ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                format!("{} is not a pair key (64 hex digits)", path.display()),
            )
        }),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e),
    }
}

/// Save `key` at `path`, readable by its owner only.
pub fn save_key(path: &Path, key: &[u8; 32]) -> io::Result<()> {
    use std::os::unix::fs::OpenOptionsExt;
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let tmp = path.with_extension("key.new");
    let mut f = std::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(&tmp)?;
    writeln!(f, "{}", key_hex(key))?;
    f.sync_all()?;
    std::fs::rename(&tmp, path)
}

pub fn key_hex(key: &[u8; 32]) -> String {
    key.iter().map(|b| format!("{b:02x}")).collect()
}

pub fn parse_key(text: &str) -> Option<[u8; 32]> {
    let hex = text.trim();
    if hex.len() != 64 || !hex.is_ascii() {
        return None;
    }
    let mut key = [0u8; 32];
    for (i, b) in key.iter_mut().enumerate() {
        *b = u8::from_str_radix(&hex[i * 2..i * 2 + 2], 16).ok()?;
    }
    Some(key)
}

#[cfg(test)]
mod tests {
    use super::*;

    const KEY: [u8; 32] = [7; 32];

    #[test]
    fn a_sealed_packet_opens_once() {
        let tx = Seal::new(&KEY, Channel::Input);
        let rx = Seal::new(&KEY, Channel::Input);
        let env = tx.seal(b"key down");
        assert_eq!(rx.open(&env).unwrap(), b"key down");
        assert_eq!(rx.open(&env), Err(OpenError::Replay));
    }

    #[test]
    fn another_key_or_a_flipped_bit_does_not_open() {
        let tx = Seal::new(&KEY, Channel::Input);
        let rx = Seal::new(&[8; 32], Channel::Input);
        assert_eq!(rx.open(&tx.seal(b"x")), Err(OpenError::Forged));
        let rx = Seal::new(&KEY, Channel::Input);
        let mut env = tx.seal(b"x");
        env[30] ^= 1; // the time, in the associated data
        assert!(matches!(
            rx.open(&env),
            Err(OpenError::Forged) | Err(OpenError::Clock { .. })
        ));
        let mut env = tx.seal(b"x");
        *env.last_mut().unwrap() ^= 1;
        assert_eq!(rx.open(&env), Err(OpenError::Forged));
    }

    #[test]
    fn unsealed_stale_and_cross_channel_packets_are_dropped() {
        let tx = Seal::new(&KEY, Channel::Input);
        let rx = Seal::new(&KEY, Channel::Input);
        assert_eq!(rx.open(b"KVM1....plain"), Err(OpenError::NotSealed));
        let old = tx.seal_at(b"x", now_ms() - 60_000);
        assert!(matches!(rx.open(&old), Err(OpenError::Clock { .. })));
        let clip = Seal::new(&KEY, Channel::Clipboard);
        assert_eq!(rx.open(&clip.seal(b"x")), Err(OpenError::WrongChannel));
    }

    #[test]
    fn reordered_packets_open_within_the_window() {
        let tx = Seal::new(&KEY, Channel::Input);
        let rx = Seal::new(&KEY, Channel::Input);
        let a = tx.seal(b"a");
        let b = tx.seal(b"b");
        let c = tx.seal(b"c");
        assert!(rx.open(&c).is_ok());
        assert!(rx.open(&a).is_ok());
        assert!(rx.open(&b).is_ok());
        assert_eq!(rx.open(&a), Err(OpenError::Replay));
        let far: Vec<_> = (0..70).map(|_| tx.seal(b"m")).collect();
        assert!(rx.open(far.last().unwrap()).is_ok());
        assert_eq!(rx.open(&far[0]), Err(OpenError::Replay));
    }

    #[test]
    fn frames_round_trip_on_a_stream() {
        let tx = Seal::new(&KEY, Channel::Clipboard);
        let rx = Seal::new(&KEY, Channel::Clipboard);
        let mut wire = Vec::new();
        tx.write_frame(&mut wire, b"hello").unwrap();
        tx.write_frame(&mut wire, b"world").unwrap();
        let mut r = io::Cursor::new(wire);
        assert_eq!(rx.read_frame(&mut r, 1024).unwrap(), b"hello");
        assert_eq!(rx.read_frame(&mut r, 1024).unwrap(), b"world");
    }

    #[test]
    fn the_key_file_round_trips_and_a_bad_one_is_an_error() {
        let dir = std::env::temp_dir().join(format!("sola-kvm-seal-{}", std::process::id()));
        let path = dir.join("pair.key");
        assert_eq!(load_key(&path).unwrap(), None);
        let key = new_key();
        save_key(&path, &key).unwrap();
        assert_eq!(load_key(&path).unwrap(), Some(key));
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(&path).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o600);
        std::fs::write(&path, "not a key\n").unwrap();
        assert!(load_key(&path).is_err());
        let _ = std::fs::remove_dir_all(dir);
    }
}
