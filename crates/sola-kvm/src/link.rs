//! Paired KVM traffic: Noise IK (`Noise_IK_25519_ChaChaPoly_BLAKE2s`)
//! between this machine's static key and its peer's, swapped once by
//! `sola-kvm pair` as SSH swaps host keys. The server is the initiator. Every
//! input datagram and clipboard frame is encrypted and authenticated with
//! keys from a handshake; anything not from the paired key is dropped.
//!
//! Input (UDP), WireGuard's way: the server sends an `Init` (Noise's first
//! message, carrying its clock in nanoseconds), the peer answers with a
//! `Reply`, and data travels under the session's keys with an explicit
//! counter as the nonce. The peer takes an `Init` only when its time is
//! later than the last one it took, and a counter once (a window of 64 lets
//! UDP reorder), so nothing replays; only the server's own clock has to move
//! forward. The server starts a new session every [`REKEY`] and stops
//! sending on one older than [`REJECT`].
//!
//! Datagram: `KVMN` | version 3 | type | …
//! - `Init`: sender index (4) | Noise message 1
//! - `Reply`: sender index (4) | receiver index (4) | Noise message 2
//! - `Data`: receiver index (4) | counter (8) | ciphertext and tag
//!
//! Clipboard (TCP): the connecting side runs the handshake as the initiator
//! on the stream, then frames travel as records (`u16` length, then a Noise
//! message); a frame larger than one record spans several.
//!
//! Self-contained (no `crate::` paths): the Mac agent includes this file.

use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use snow::{HandshakeState, StatelessTransportState, TransportState};

pub const PARAMS: &str = "Noise_IK_25519_ChaChaPoly_BLAKE2s";
pub const MAGIC: [u8; 4] = *b"KVMN";
pub const VERSION: u8 = 3;
const INIT: u8 = 1;
const REPLY: u8 = 2;
const DATA: u8 = 3;
const HEADER: usize = 6;
const TAG: usize = 16;
/// Resend an unanswered `Init` this often.
pub const RETRY: Duration = Duration::from_secs(1);
/// Start a new session this often.
pub const REKEY: Duration = Duration::from_secs(120);
/// Neither side uses a session older than this.
pub const REJECT: Duration = Duration::from_secs(180);
const REORDER: u64 = 64;
/// The most plaintext one clipboard record carries (a Noise message is at
/// most 65535 bytes, with the tag and a continuation byte).
const CHUNK: usize = 65535 - TAG - 1;

// ── Keys ─────────────────────────────────────────────────────────────────

/// This machine's static key pair and its peer's public key.
#[derive(Clone, PartialEq, Eq)]
pub struct Keys {
    pub private: [u8; 32],
    pub public: [u8; 32],
    pub peer: [u8; 32],
}

impl std::fmt::Debug for Keys {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Keys")
            .field("public", &hex(&self.public))
            .field("peer", &hex(&self.peer))
            .finish_non_exhaustive()
    }
}

/// `$XDG_CONFIG_HOME/sola-kvm` or `~/.config/sola-kvm`: `key` (this
/// machine's private key, its owner's alone), `key.pub`, and `peer.pub`.
pub fn dir() -> PathBuf {
    std::env::var_os("XDG_CONFIG_HOME")
        .filter(|d| !d.is_empty())
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))
        .unwrap_or_else(|| PathBuf::from("."))
        .join("sola-kvm")
}

/// This machine's key pair, `None` before the first pairing.
pub fn own(dir: &Path) -> io::Result<Option<([u8; 32], [u8; 32])>> {
    let Some(private) = read_key(&dir.join("key"))? else {
        return Ok(None);
    };
    let public = read_key(&dir.join("key.pub"))?
        .ok_or_else(|| bad(format!("{} is missing", dir.join("key.pub").display())))?;
    Ok(Some((private, public)))
}

/// This machine's key pair, made and saved the first time.
pub fn own_or_new(dir: &Path) -> io::Result<([u8; 32], [u8; 32])> {
    if let Some(pair) = own(dir)? {
        return Ok(pair);
    }
    let (private, public) = new_pair()?;
    write_key(&dir.join("key"), &private, 0o600)?;
    write_key(&dir.join("key.pub"), &public, 0o644)?;
    Ok((private, public))
}

/// A new X25519 key pair: `(private, public)`.
pub fn new_pair() -> io::Result<([u8; 32], [u8; 32])> {
    let kp = snow::Builder::new(params()).generate_keypair().map_err(noise_err)?;
    let private: [u8; 32] = kp.private[..].try_into().map_err(|_| bad("key length".into()))?;
    let public: [u8; 32] = kp.public[..].try_into().map_err(|_| bad("key length".into()))?;
    Ok((private, public))
}

/// Keep `peer` as the paired machine's public key.
pub fn save_peer(dir: &Path, peer: &[u8; 32]) -> io::Result<()> {
    write_key(&dir.join("peer.pub"), peer, 0o644)?;
    // A shared key from before public keys is no longer used.
    let _ = std::fs::remove_file(dir.join("pair.key"));
    Ok(())
}

/// Forget the peer; `false` when there was none.
pub fn forget_peer(dir: &Path) -> io::Result<bool> {
    let _ = std::fs::remove_file(dir.join("pair.key"));
    match std::fs::remove_file(dir.join("peer.pub")) {
        Ok(()) => Ok(true),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(false),
        Err(e) => Err(e),
    }
}

/// The pairing; `None` when this machine is not paired. Files that are not
/// keys, and a shared key from an older sola-kvm, are errors: the caller
/// must not start then.
pub fn load(dir: &Path) -> io::Result<Option<Keys>> {
    let Some(peer) = read_key(&dir.join("peer.pub"))? else {
        if dir.join("pair.key").exists() {
            return Err(bad(format!(
                "{} is a shared key from an older sola-kvm; pair again (`sola-kvm pair`)",
                dir.join("pair.key").display()
            )));
        }
        return Ok(None);
    };
    let (private, public) =
        own(dir)?.ok_or_else(|| bad(format!("{} is missing", dir.join("key").display())))?;
    Ok(Some(Keys {
        private,
        public,
        peer,
    }))
}

pub fn hex(key: &[u8; 32]) -> String {
    key.iter().map(|b| format!("{b:02x}")).collect()
}

pub fn parse_hex(text: &str) -> Option<[u8; 32]> {
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

fn read_key(path: &Path) -> io::Result<Option<[u8; 32]>> {
    match std::fs::read_to_string(path) {
        Ok(text) => parse_hex(&text)
            .map(Some)
            .ok_or_else(|| bad(format!("{} is not a key (64 hex digits)", path.display()))),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e),
    }
}

fn write_key(path: &Path, key: &[u8; 32], mode: u32) -> io::Result<()> {
    use std::os::unix::fs::OpenOptionsExt;
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let tmp = path.with_extension("new");
    let mut f = std::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(mode)
        .open(&tmp)?;
    writeln!(f, "{}", hex(key))?;
    f.sync_all()?;
    std::fs::rename(&tmp, path)
}

// ── Input: the datagrams ─────────────────────────────────────────────────

/// Why a datagram was dropped.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Dropped {
    Short,
    /// Not this link's (an unpaired sender, or an older sola-kvm).
    NotOurs,
    /// A handshake from a key that is not the peer's.
    Stranger,
    /// Does not open with the session's keys.
    Forged,
    /// Already taken.
    Replay,
    /// An `Init` no later than the last one taken.
    Stale,
    /// For a session this side does not have (or no longer has).
    NoSession,
}

impl std::fmt::Display for Dropped {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Short => "too short",
            Self::NotOurs => "not this link's (is the sender paired, and on this version?)",
            Self::Stranger => "a handshake from a key that is not the paired machine's",
            Self::Forged => "does not open with the session's keys",
            Self::Replay => "already taken (replay)",
            Self::Stale => "a handshake no later than the last one (replay, or the server's clock went back)",
            Self::NoSession => "for no current session",
        })
    }
}

/// The server's side: handshakes and sealing.
pub struct Initiator {
    keys: Keys,
    pending: Option<Pending>,
    session: Option<Out>,
}

struct Pending {
    index: u32,
    hs: HandshakeState,
    sent: Instant,
}

struct Out {
    transport: StatelessTransportState,
    receiver: u32,
    counter: u64,
    since: Instant,
}

impl Initiator {
    pub fn new(keys: Keys) -> Self {
        Self {
            keys,
            pending: None,
            session: None,
        }
    }

    /// An `Init` to send now, when one is due: no session yet (every
    /// [`RETRY`] until answered), or the session is [`REKEY`] old.
    pub fn due(&mut self, now: Instant) -> Option<Vec<u8>> {
        if self.pending.as_ref().is_some_and(|p| now - p.sent < RETRY) {
            return None;
        }
        if self.session.as_ref().is_some_and(|s| now - s.since < REKEY) {
            return None;
        }
        let mut hs = initiator(&self.keys).ok()?;
        let index = random_u32();
        let mut msg = vec![0u8; 256];
        let n = hs.write_message(&now_nanos().to_le_bytes(), &mut msg).ok()?;
        let mut out = header(INIT);
        out.extend_from_slice(&index.to_le_bytes());
        out.extend_from_slice(&msg[..n]);
        self.pending = Some(Pending {
            index,
            hs,
            sent: now,
        });
        Some(out)
    }

    /// A datagram from the peer: its `Reply` completes the handshake.
    pub fn take(&mut self, bytes: &[u8], now: Instant) -> Result<(), Dropped> {
        if kind(bytes)? != REPLY || bytes.len() < HEADER + 8 {
            return Err(Dropped::NotOurs);
        }
        let sender = u32_at(bytes, HEADER);
        let receiver = u32_at(bytes, HEADER + 4);
        if self.pending.as_ref().map(|p| p.index) != Some(receiver) {
            return Err(Dropped::NoSession);
        }
        let mut p = self.pending.take().expect("pending");
        let mut buf = vec![0u8; 256];
        p.hs
            .read_message(&bytes[HEADER + 8..], &mut buf)
            .map_err(|_| Dropped::Forged)?;
        let transport = p.hs.into_stateless_transport_mode().map_err(|_| Dropped::Forged)?;
        self.session = Some(Out {
            transport,
            receiver: sender,
            counter: 0,
            since: now,
        });
        Ok(())
    }

    /// Whether data can go: a session no older than [`REJECT`].
    pub fn is_up(&self, now: Instant) -> bool {
        self.session.as_ref().is_some_and(|s| now - s.since < REJECT)
    }

    /// `plain` as a `Data` datagram; `None` while the link is down.
    pub fn seal(&mut self, plain: &[u8], now: Instant) -> Option<Vec<u8>> {
        if !self.is_up(now) {
            return None;
        }
        let s = self.session.as_mut()?;
        let counter = s.counter;
        s.counter += 1;
        let mut out = header(DATA);
        out.extend_from_slice(&s.receiver.to_le_bytes());
        out.extend_from_slice(&counter.to_le_bytes());
        let at = out.len();
        out.resize(at + plain.len() + TAG, 0);
        let n = s.transport.write_message(counter, plain, &mut out[at..]).ok()?;
        out.truncate(at + n);
        Some(out)
    }
}

/// The peer's side: answers handshakes and opens data.
pub struct Responder {
    keys: Keys,
    /// The newest `Init` time taken (the server's clock, nanoseconds).
    last_init: u64,
    /// The current session first, then the one before (data in flight
    /// across a rekey).
    sessions: Vec<In>,
}

struct In {
    index: u32,
    transport: StatelessTransportState,
    window: Window,
    since: Instant,
}

/// What a datagram came to.
#[derive(Debug, PartialEq, Eq)]
pub enum Taken {
    /// A handshake answer: send it back to the sender.
    Reply(Vec<u8>),
    /// Data, opened.
    Data(Vec<u8>),
}

impl Responder {
    pub fn new(keys: Keys) -> Self {
        Self {
            keys,
            last_init: 0,
            sessions: Vec::new(),
        }
    }

    pub fn take(&mut self, bytes: &[u8], now: Instant) -> Result<Taken, Dropped> {
        match kind(bytes)? {
            INIT => self.take_init(bytes, now).map(Taken::Reply),
            DATA => self.open(bytes, now).map(Taken::Data),
            _ => Err(Dropped::NotOurs),
        }
    }

    fn take_init(&mut self, bytes: &[u8], now: Instant) -> Result<Vec<u8>, Dropped> {
        if bytes.len() < HEADER + 4 {
            return Err(Dropped::Short);
        }
        let sender = u32_at(bytes, HEADER);
        let mut hs = responder(&self.keys).map_err(|_| Dropped::Forged)?;
        let mut payload = [0u8; 64];
        let n = hs
            .read_message(&bytes[HEADER + 4..], &mut payload)
            .map_err(|_| Dropped::Forged)?;
        if hs.get_remote_static() != Some(&self.keys.peer[..]) {
            return Err(Dropped::Stranger);
        }
        if n != 8 {
            return Err(Dropped::Forged);
        }
        let time = u64::from_le_bytes(payload[..8].try_into().expect("8"));
        if time <= self.last_init {
            return Err(Dropped::Stale);
        }
        let index = random_u32();
        let mut msg = vec![0u8; 256];
        let m = hs.write_message(&[], &mut msg).map_err(|_| Dropped::Forged)?;
        let transport = hs.into_stateless_transport_mode().map_err(|_| Dropped::Forged)?;
        self.last_init = time;
        self.sessions.insert(
            0,
            In {
                index,
                transport,
                window: Window::default(),
                since: now,
            },
        );
        self.sessions.truncate(2);
        let mut out = header(REPLY);
        out.extend_from_slice(&index.to_le_bytes());
        out.extend_from_slice(&sender.to_le_bytes());
        out.extend_from_slice(&msg[..m]);
        Ok(out)
    }

    fn open(&mut self, bytes: &[u8], now: Instant) -> Result<Vec<u8>, Dropped> {
        if bytes.len() < HEADER + 12 + TAG {
            return Err(Dropped::Short);
        }
        let receiver = u32_at(bytes, HEADER);
        let counter = u64::from_le_bytes(bytes[HEADER + 4..HEADER + 12].try_into().expect("8"));
        let s = self
            .sessions
            .iter_mut()
            .find(|s| s.index == receiver && now - s.since < REJECT)
            .ok_or(Dropped::NoSession)?;
        let mut plain = vec![0u8; bytes.len()];
        let n = s
            .transport
            .read_message(counter, &bytes[HEADER + 12..], &mut plain)
            .map_err(|_| Dropped::Forged)?;
        if !s.window.note(counter) {
            return Err(Dropped::Replay);
        }
        plain.truncate(n);
        Ok(plain)
    }
}

/// Counters taken: the highest, and which of the [`REORDER`] below it
/// (bit `i` = `top - i`).
#[derive(Default)]
struct Window {
    top: Option<u64>,
    mask: u64,
}

impl Window {
    /// Take `counter` once.
    fn note(&mut self, counter: u64) -> bool {
        let Some(top) = self.top else {
            self.top = Some(counter);
            self.mask = 1;
            return true;
        };
        if counter > top {
            let ahead = counter - top;
            self.mask = if ahead >= REORDER { 0 } else { self.mask << ahead };
            self.mask |= 1;
            self.top = Some(counter);
            return true;
        }
        let behind = top - counter;
        if behind >= REORDER || self.mask & (1 << behind) != 0 {
            return false;
        }
        self.mask |= 1 << behind;
        true
    }
}

fn header(kind: u8) -> Vec<u8> {
    let mut out = Vec::with_capacity(160);
    out.extend_from_slice(&MAGIC);
    out.push(VERSION);
    out.push(kind);
    out
}

fn kind(bytes: &[u8]) -> Result<u8, Dropped> {
    if bytes.len() < HEADER {
        return Err(Dropped::Short);
    }
    if bytes[..4] != MAGIC || bytes[4] != VERSION {
        return Err(Dropped::NotOurs);
    }
    Ok(bytes[5])
}

fn u32_at(bytes: &[u8], at: usize) -> u32 {
    u32::from_le_bytes(bytes[at..at + 4].try_into().expect("4"))
}

// ── Clipboard: the stream ────────────────────────────────────────────────

/// One paired clipboard connection's Noise session.
pub struct Conn {
    noise: TransportState,
    /// Bytes read and not yet a whole record.
    inbuf: Vec<u8>,
    /// A frame's records so far.
    frame: Vec<u8>,
}

impl Conn {
    /// Run the handshake as the connecting side. Reads block for as long
    /// as the stream's timeout allows.
    pub fn connect<S: Read + Write>(s: &mut S, keys: &Keys) -> io::Result<Self> {
        let mut hs = initiator(keys).map_err(noise_err)?;
        let mut buf = vec![0u8; 256];
        let n = hs.write_message(&[], &mut buf).map_err(noise_err)?;
        write_record(s, &buf[..n])?;
        let reply = read_record(s)?;
        hs.read_message(&reply, &mut buf).map_err(|_| {
            bad("the handshake reply does not open (is the peer paired with this machine?)".into())
        })?;
        Ok(Self::new(hs.into_transport_mode().map_err(noise_err)?))
    }

    /// Run the handshake as the accepting side; a connection from any key
    /// but the peer's is refused.
    pub fn accept<S: Read + Write>(s: &mut S, keys: &Keys) -> io::Result<Self> {
        let mut hs = responder(keys).map_err(noise_err)?;
        let first = read_record(s)?;
        let mut buf = vec![0u8; 256];
        hs.read_message(&first, &mut buf)
            .map_err(|_| bad("the handshake does not open (not paired with this machine)".into()))?;
        if hs.get_remote_static() != Some(&keys.peer[..]) {
            return Err(bad("a handshake from a key that is not the paired machine's".into()));
        }
        let n = hs.write_message(&[], &mut buf).map_err(noise_err)?;
        write_record(s, &buf[..n])?;
        Ok(Self::new(hs.into_transport_mode().map_err(noise_err)?))
    }

    fn new(noise: TransportState) -> Self {
        Self {
            noise,
            inbuf: Vec::new(),
            frame: Vec::new(),
        }
    }

    /// Write `plain` as one frame.
    pub fn write_frame(&mut self, w: &mut dyn Write, plain: &[u8]) -> io::Result<()> {
        let mut out = Vec::with_capacity(plain.len() + 64);
        let mut chunks: Vec<&[u8]> = plain.chunks(CHUNK).collect();
        if chunks.is_empty() {
            chunks.push(&[]);
        }
        let last = chunks.len() - 1;
        let mut record = Vec::with_capacity(CHUNK + 1);
        let mut sealed = vec![0u8; CHUNK + 1 + TAG];
        for (i, chunk) in chunks.into_iter().enumerate() {
            record.clear();
            record.push(u8::from(i != last));
            record.extend_from_slice(chunk);
            let n = self.noise.write_message(&record, &mut sealed).map_err(noise_err)?;
            out.extend_from_slice(&(n as u16).to_be_bytes());
            out.extend_from_slice(&sealed[..n]);
        }
        w.write_all(&out)
    }

    /// Read one frame of at most `max` bytes. A read that times out keeps
    /// what came so far, and the next call goes on from there.
    pub fn read_frame(&mut self, r: &mut dyn Read, max: u32) -> io::Result<Vec<u8>> {
        let mut plain = vec![0u8; 65535];
        loop {
            if self.inbuf.len() >= 2 {
                let len = u16::from_be_bytes([self.inbuf[0], self.inbuf[1]]) as usize;
                if self.inbuf.len() >= 2 + len {
                    let n = self
                        .noise
                        .read_message(&self.inbuf[2..2 + len], &mut plain)
                        .map_err(|_| bad("a clipboard record does not open".into()))?;
                    self.inbuf.drain(..2 + len);
                    if n == 0 {
                        return Err(bad("an empty clipboard record".into()));
                    }
                    self.frame.extend_from_slice(&plain[1..n]);
                    if self.frame.len() > max as usize + 64 {
                        self.frame.clear();
                        return Err(bad(format!("a clipboard frame over {max} bytes")));
                    }
                    if plain[0] == 0 {
                        return Ok(std::mem::take(&mut self.frame));
                    }
                    continue;
                }
            }
            let mut buf = [0u8; 16384];
            match r.read(&mut buf) {
                Ok(0) => return Err(io::ErrorKind::UnexpectedEof.into()),
                Ok(n) => self.inbuf.extend_from_slice(&buf[..n]),
                Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
                Err(e) => return Err(e),
            }
        }
    }
}

fn write_record(w: &mut dyn Write, msg: &[u8]) -> io::Result<()> {
    let mut out = Vec::with_capacity(2 + msg.len());
    out.extend_from_slice(&(msg.len() as u16).to_be_bytes());
    out.extend_from_slice(msg);
    w.write_all(&out)?;
    w.flush()
}

fn read_record(r: &mut dyn Read) -> io::Result<Vec<u8>> {
    let mut len = [0u8; 2];
    r.read_exact(&mut len)?;
    let mut msg = vec![0u8; u16::from_be_bytes(len) as usize];
    r.read_exact(&mut msg)?;
    Ok(msg)
}

// ── Noise ────────────────────────────────────────────────────────────────

fn params() -> snow::params::NoiseParams {
    PARAMS.parse().expect("noise params")
}

fn initiator(keys: &Keys) -> Result<HandshakeState, snow::Error> {
    snow::Builder::new(params())
        .local_private_key(&keys.private)?
        .remote_public_key(&keys.peer)?
        .build_initiator()
}

fn responder(keys: &Keys) -> Result<HandshakeState, snow::Error> {
    snow::Builder::new(params())
        .local_private_key(&keys.private)?
        .build_responder()
}

fn random_u32() -> u32 {
    let mut b = [0u8; 4];
    getrandom::getrandom(&mut b).expect("getrandom");
    u32::from_le_bytes(b)
}

fn now_nanos() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0)
}

fn noise_err(e: snow::Error) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, format!("noise: {e}"))
}

fn bad(msg: String) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, msg)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A paired server and peer, and a stranger who knows the peer's
    /// public key.
    fn pair() -> (Keys, Keys, Keys) {
        let (sp, spub) = new_pair().unwrap();
        let (pp, ppub) = new_pair().unwrap();
        let (xp, xpub) = new_pair().unwrap();
        let server = Keys { private: sp, public: spub, peer: ppub };
        let peer = Keys { private: pp, public: ppub, peer: spub };
        let stranger = Keys { private: xp, public: xpub, peer: ppub };
        (server, peer, stranger)
    }

    fn link(server: &Keys, peer: &Keys) -> (Initiator, Responder) {
        link_at(server, peer, Instant::now())
    }

    fn link_at(server: &Keys, peer: &Keys, now: Instant) -> (Initiator, Responder) {
        let mut i = Initiator::new(server.clone());
        let mut r = Responder::new(peer.clone());
        let init = i.due(now).unwrap();
        let Ok(Taken::Reply(reply)) = r.take(&init, now) else { panic!("no reply") };
        i.take(&reply, now).unwrap();
        (i, r)
    }

    #[test]
    fn data_opens_once_after_the_handshake() {
        let (s, p, _) = pair();
        let now = Instant::now();
        let mut alone = Initiator::new(s.clone());
        assert_eq!(alone.seal(b"x", now), None, "nothing goes before the handshake");
        let (mut i, mut r) = link(&s, &p);
        let d = i.seal(b"key down", now).unwrap();
        assert!(!d.windows(8).any(|w| w == b"key down"));
        assert_eq!(r.take(&d, now), Ok(Taken::Data(b"key down".to_vec())));
        assert_eq!(r.take(&d, now), Err(Dropped::Replay));
    }

    #[test]
    fn a_stranger_or_a_flipped_bit_is_dropped() {
        let (s, p, x) = pair();
        let now = Instant::now();
        let mut r = Responder::new(p.clone());
        let mut stranger = Initiator::new(x);
        assert_eq!(r.take(&stranger.due(now).unwrap(), now), Err(Dropped::Stranger));
        let (mut i, mut r) = link(&s, &p);
        let mut d = i.seal(b"x", now).unwrap();
        *d.last_mut().unwrap() ^= 1;
        assert_eq!(r.take(&d, now), Err(Dropped::Forged));
        let mut d = i.seal(b"x", now).unwrap();
        d[HEADER + 4] ^= 1; // the counter is the nonce
        assert_eq!(r.take(&d, now), Err(Dropped::Forged));
        assert_eq!(r.take(b"KVM1....plain", now), Err(Dropped::NotOurs));
    }

    #[test]
    fn a_replayed_init_is_stale() {
        let (s, p, _) = pair();
        let now = Instant::now();
        let mut i = Initiator::new(s);
        let mut r = Responder::new(p);
        let init = i.due(now).unwrap();
        assert!(matches!(r.take(&init, now), Ok(Taken::Reply(_))));
        assert_eq!(r.take(&init, now), Err(Dropped::Stale));
    }

    #[test]
    fn reordered_data_opens_within_the_window() {
        let (s, p, _) = pair();
        let now = Instant::now();
        let (mut i, mut r) = link(&s, &p);
        let a = i.seal(b"a", now).unwrap();
        let b = i.seal(b"b", now).unwrap();
        let c = i.seal(b"c", now).unwrap();
        assert!(r.take(&c, now).is_ok());
        assert!(r.take(&a, now).is_ok());
        assert!(r.take(&b, now).is_ok());
        assert_eq!(r.take(&a, now), Err(Dropped::Replay));
        let far: Vec<_> = (0..70).map(|_| i.seal(b"m", now).unwrap()).collect();
        assert!(r.take(far.last().unwrap(), now).is_ok());
        assert_eq!(r.take(&far[0], now), Err(Dropped::Replay));
    }

    #[test]
    fn a_rekey_keeps_the_last_session_and_old_ones_expire() {
        let (s, p, _) = pair();
        let t0 = Instant::now();
        let (mut i, mut r) = link_at(&s, &p, t0);
        let old = i.seal(b"old", t0).unwrap();
        let t1 = t0 + REKEY;
        let init = i.due(t1).expect("rekey due");
        let Ok(Taken::Reply(reply)) = r.take(&init, t1) else { panic!() };
        i.take(&reply, t1).unwrap();
        assert_eq!(r.take(&old, t1), Ok(Taken::Data(b"old".to_vec())));
        let new = i.seal(b"new", t1).unwrap();
        assert_eq!(r.take(&new, t1), Ok(Taken::Data(b"new".to_vec())));
        let late = t1 + REJECT;
        assert!(!i.is_up(late));
        assert_eq!(i.seal(b"x", late), None);
        assert_eq!(r.take(&new, late), Err(Dropped::NoSession));
    }

    #[test]
    fn an_unanswered_init_is_sent_again_after_retry() {
        let (s, _, _) = pair();
        let now = Instant::now();
        let mut i = Initiator::new(s);
        assert!(i.due(now).is_some());
        assert!(i.due(now).is_none());
        assert!(i.due(now + RETRY).is_some());
    }

    #[test]
    fn clipboard_frames_round_trip_and_span_records() {
        use std::os::unix::net::UnixStream;
        let (s, p, x) = pair();
        let (mut a, mut b) = UnixStream::pair().unwrap();
        let peer = std::thread::spawn(move || {
            let mut c = Conn::accept(&mut b, &p).unwrap();
            let big = c.read_frame(&mut b, 1 << 20).unwrap();
            let small = c.read_frame(&mut b, 1 << 20).unwrap();
            c.write_frame(&mut b, b"ack").unwrap();
            (big, small)
        });
        let mut c = Conn::connect(&mut a, &s).unwrap();
        let big: Vec<u8> = (0..200_000u32).map(|i| i as u8).collect();
        c.write_frame(&mut a, &big).unwrap();
        c.write_frame(&mut a, b"").unwrap();
        assert_eq!(c.read_frame(&mut a, 1024).unwrap(), b"ack");
        let (got_big, got_small) = peer.join().unwrap();
        assert_eq!(got_big, big);
        assert!(got_small.is_empty());

        let (mut a, mut b) = UnixStream::pair().unwrap();
        let peer = std::thread::spawn(move || Conn::accept(&mut b, &s).is_err());
        let _ = Conn::connect(&mut a, &x);
        assert!(peer.join().unwrap(), "a stranger's connection is refused");
    }

    #[test]
    fn keys_save_load_and_forget() {
        let dir = std::env::temp_dir().join(format!("sola-kvm-link-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        assert_eq!(load(&dir).unwrap(), None);
        let (private, public) = own_or_new(&dir).unwrap();
        assert_eq!(own_or_new(&dir).unwrap(), (private, public));
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(dir.join("key")).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o600);
        assert_eq!(load(&dir).unwrap(), None, "a key pair alone is not a pairing");
        save_peer(&dir, &[9; 32]).unwrap();
        let keys = load(&dir).unwrap().unwrap();
        assert_eq!((keys.public, keys.peer), (public, [9; 32]));
        assert!(forget_peer(&dir).unwrap());
        assert_eq!(load(&dir).unwrap(), None);
        std::fs::write(dir.join("pair.key"), "00".repeat(32)).unwrap();
        assert!(load(&dir).is_err(), "an old shared key stops the start");
        let _ = std::fs::remove_dir_all(dir);
    }
}
