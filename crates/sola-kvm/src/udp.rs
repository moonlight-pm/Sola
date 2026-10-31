//! UDP send / receive for sola-kvm packets, over the paired link
//! ([`crate::link`]): the server's [`Sender`] runs the handshake, the
//! peer's [`Listener`] answers it.

use std::net::{SocketAddr, ToSocketAddrs, UdpSocket};
use std::os::fd::{AsRawFd, RawFd};
use std::time::{Duration, Instant};

use tracing::{debug, info, warn};

use crate::link::{Dropped, Initiator, Keys, Responder, Taken};
use crate::protocol::{self, DecodeError, EncodeError, Packet};

/// Outbound sender (server → peer).
pub struct Sender {
    sock: UdpSocket,
    peer: SocketAddr,
    seq: u32,
    link: Initiator,
    /// Whether the link was up at the last [`Sender::pump`] (for the log).
    up: bool,
}

impl Sender {
    /// Bind an ephemeral local port, target `peer` (`host:port`), and start
    /// the handshake with the paired machine there.
    pub fn connect(peer: &str, keys: Keys) -> Result<Self, UdpError> {
        let peer = resolve_one(peer)?;
        let sock = UdpSocket::bind("0.0.0.0:0").map_err(UdpError::Io)?;
        // Connected UDP: send() needs no address, and recv() takes only the
        // peer's datagrams.
        sock.connect(peer).map_err(UdpError::Io)?;
        sock.set_nonblocking(true).map_err(UdpError::Io)?;
        crate::priority::boost_udp_socket(&sock);
        let mut sender = Self {
            sock,
            peer,
            seq: 0,
            link: Initiator::new(keys),
            up: false,
        };
        sender.pump();
        Ok(sender)
    }

    pub fn peer(&self) -> SocketAddr {
        self.peer
    }

    pub fn seq(&self) -> u32 {
        self.seq
    }

    /// The socket, to wait on with the other inputs.
    pub fn fd(&self) -> RawFd {
        self.sock.as_raw_fd()
    }

    /// Whether packets can go: the handshake is done and fresh.
    pub fn is_up(&self) -> bool {
        self.link.is_up(Instant::now())
    }

    /// Take the peer's handshake replies and send a handshake when one is
    /// due. Call often (each loop turn); it never blocks.
    pub fn pump(&mut self) {
        let mut buf = [0u8; 512];
        loop {
            match self.sock.recv(&mut buf) {
                Ok(n) => {
                    if let Err(reason) = self.link.take(&buf[..n], Instant::now()) {
                        debug!(peer = %self.peer, %reason, "link: reply dropped");
                    }
                }
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => break,
                // A refused send comes back as an error on the next recv.
                Err(e) => {
                    debug!(peer = %self.peer, %e, "link: recv");
                    break;
                }
            }
        }
        if let Some(init) = self.link.due(Instant::now()) {
            if let Err(e) = self.sock.send(&init) {
                debug!(peer = %self.peer, %e, "link: handshake send");
            }
        }
        let up = self.is_up();
        if up != self.up {
            if up {
                info!(peer = %self.peer, "link up: the paired peer answered");
            } else {
                warn!(peer = %self.peer, "link down: the paired peer is not answering");
            }
            self.up = up;
        }
    }

    /// Encode and send one packet; advances the sequence on success. Fails
    /// while the link is down.
    pub fn send(&mut self, packet: &Packet) -> Result<u32, UdpError> {
        let seq = self.seq;
        let plain = protocol::encode(seq, packet).map_err(UdpError::Encode)?;
        let sealed = self
            .link
            .seal(&plain, Instant::now())
            .ok_or(UdpError::LinkDown)?;
        self.sock.send(&sealed).map_err(UdpError::Io)?;
        self.seq = self.seq.wrapping_add(1);
        Ok(seq)
    }

    /// Pump until the link is up or `timeout` passes (one-shot tools).
    pub fn wait_up(&mut self, timeout: Duration) -> bool {
        let until = Instant::now() + timeout;
        while Instant::now() < until {
            self.pump();
            if self.is_up() {
                return true;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        false
    }
}

/// Inbound listener (the peer).
pub struct Listener {
    sock: UdpSocket,
    link: Responder,
}

impl Listener {
    /// Bind `0.0.0.0:port` (or a full bind address like `127.0.0.1:4242`)
    /// and take only the paired server's traffic.
    pub fn bind(addr: &str, keys: Keys) -> Result<Self, UdpError> {
        let sock = UdpSocket::bind(addr).map_err(UdpError::Io)?;
        Ok(Self {
            sock,
            link: Responder::new(keys),
        })
    }

    pub fn local_addr(&self) -> Result<SocketAddr, UdpError> {
        self.sock.local_addr().map_err(UdpError::Io)
    }

    pub fn set_read_timeout(&self, timeout: Option<Duration>) -> Result<(), UdpError> {
        self.sock.set_read_timeout(timeout).map_err(UdpError::Io)
    }

    /// Receive one packet and decode it. Returns `(src, seq, packet)`.
    /// Handshakes are answered on the way.
    pub fn recv(&mut self) -> Result<(SocketAddr, u32, Packet), UdpError> {
        let mut buf = [0u8; 512];
        loop {
            let (n, src) = self.sock.recv_from(&mut buf).map_err(UdpError::Io)?;
            match self.link.take(&buf[..n], Instant::now()) {
                Ok(Taken::Reply(reply)) => {
                    debug!(%src, "link: handshake from the paired server");
                    if let Err(e) = self.sock.send_to(&reply, src) {
                        warn!(%src, %e, "link: handshake reply");
                    }
                }
                Ok(Taken::Data(plain)) => {
                    let (seq, packet) = protocol::decode(&plain).map_err(UdpError::Decode)?;
                    return Ok((src, seq, packet));
                }
                Err(reason) => return Err(UdpError::Dropped { src, reason }),
            }
        }
    }
}

fn resolve_one(peer: &str) -> Result<SocketAddr, UdpError> {
    peer.to_socket_addrs()
        .map_err(UdpError::Io)?
        .next()
        .ok_or_else(|| UdpError::Resolve(peer.to_string()))
}

#[derive(Debug)]
pub enum UdpError {
    Io(std::io::Error),
    Resolve(String),
    Encode(EncodeError),
    Decode(DecodeError),
    /// Not from the paired machine, or not fit to take.
    Dropped { src: SocketAddr, reason: Dropped },
    /// No handshake with the peer yet (or it went stale).
    LinkDown,
}

impl std::fmt::Display for UdpError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(e) => write!(f, "udp io: {e}"),
            Self::Resolve(s) => write!(f, "could not resolve peer: {s}"),
            Self::Encode(e) => write!(f, "encode: {e}"),
            Self::Decode(e) => write!(f, "decode: {e}"),
            Self::Dropped { src, reason } => write!(f, "dropped from {src}: {reason}"),
            Self::LinkDown => write!(f, "link down: the paired peer is not answering"),
        }
    }
}

impl std::error::Error for UdpError {}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::Packet;

    fn keys() -> (Keys, Keys, Keys) {
        let (sp, spub) = crate::link::new_pair().unwrap();
        let (pp, ppub) = crate::link::new_pair().unwrap();
        let (xp, xpub) = crate::link::new_pair().unwrap();
        (
            Keys { private: sp, public: spub, peer: ppub },
            Keys { private: pp, public: ppub, peer: spub },
            Keys { private: xp, public: xpub, peer: ppub },
        )
    }

    /// Run the listener on a thread for `n` packets.
    fn listen(mut listener: Listener, n: usize) -> std::thread::JoinHandle<Vec<Packet>> {
        listener.set_read_timeout(Some(Duration::from_millis(50))).unwrap();
        std::thread::spawn(move || {
            let mut got = Vec::new();
            let until = Instant::now() + Duration::from_secs(if n == 0 { 1 } else { 5 });
            // `n == 0`: listen until the time is up (nothing should come).
            while (n == 0 || got.len() < n) && Instant::now() < until {
                if let Ok((_, _, p)) = listener.recv() {
                    got.push(p);
                }
            }
            got
        })
    }

    #[test]
    fn localhost_roundtrip_after_the_handshake() {
        let (server, peer, _) = keys();
        let listener = Listener::bind("127.0.0.1:0", peer).unwrap();
        let addr = listener.local_addr().unwrap().to_string();
        let got = listen(listener, 1);
        let mut sender = Sender::connect(&addr, server).unwrap();
        let pkt = Packet::Key {
            keycode: 30,
            pressed: 1,
        };
        assert!(matches!(sender.send(&pkt), Err(UdpError::LinkDown)));
        assert!(sender.wait_up(Duration::from_secs(3)), "handshake answered");
        sender.send(&pkt).unwrap();
        assert_eq!(got.join().unwrap(), vec![pkt]);
    }

    #[test]
    fn a_stranger_never_gets_a_link() {
        let (_, peer, stranger) = keys();
        let listener = Listener::bind("127.0.0.1:0", peer).unwrap();
        let addr = listener.local_addr().unwrap().to_string();
        let got = listen(listener, 0);
        let mut sender = Sender::connect(&addr, stranger).unwrap();
        assert!(!sender.wait_up(Duration::from_millis(500)));
        assert!(got.join().unwrap().is_empty());
    }
}
