//! UDP send / receive helpers for sola-kvm packets.

use std::net::{SocketAddr, ToSocketAddrs, UdpSocket};
use std::time::Duration;

use crate::protocol::{self, DecodeError, EncodeError, Packet};

/// Outbound sender (novus server → ember client).
pub struct Sender {
    sock: UdpSocket,
    peer: SocketAddr,
    seq: u32,
}

impl Sender {
    /// Bind an ephemeral local port and target `peer` (`host:port`).
    pub fn connect(peer: &str) -> Result<Self, UdpError> {
        let peer = resolve_one(peer)?;
        let sock = UdpSocket::bind("0.0.0.0:0").map_err(UdpError::Io)?;
        // Connected UDP so send() doesn't need the address each time.
        sock.connect(peer).map_err(UdpError::Io)?;
        Ok(Self {
            sock,
            peer,
            seq: 0,
        })
    }

    pub fn peer(&self) -> SocketAddr {
        self.peer
    }

    pub fn seq(&self) -> u32 {
        self.seq
    }

    /// Encode and send one packet; advances sequence on success.
    pub fn send(&mut self, packet: &Packet) -> Result<u32, UdpError> {
        let seq = self.seq;
        let bytes = protocol::encode(seq, packet).map_err(UdpError::Encode)?;
        self.sock.send(&bytes).map_err(UdpError::Io)?;
        self.seq = self.seq.wrapping_add(1);
        Ok(seq)
    }
}

/// Inbound listener (ember client, or local dump/test).
pub struct Listener {
    sock: UdpSocket,
}

impl Listener {
    /// Bind `0.0.0.0:port` (or a full bind address like `127.0.0.1:4242`).
    pub fn bind(addr: &str) -> Result<Self, UdpError> {
        let sock = UdpSocket::bind(addr).map_err(UdpError::Io)?;
        Ok(Self { sock })
    }

    pub fn local_addr(&self) -> Result<SocketAddr, UdpError> {
        self.sock.local_addr().map_err(UdpError::Io)
    }

    pub fn set_read_timeout(&self, timeout: Option<Duration>) -> Result<(), UdpError> {
        self.sock.set_read_timeout(timeout).map_err(UdpError::Io)
    }

    /// Receive one datagram and decode. Returns `(src, seq, packet)`.
    pub fn recv(&self) -> Result<(SocketAddr, u32, Packet), UdpError> {
        let mut buf = [0u8; 256];
        let (n, src) = self.sock.recv_from(&mut buf).map_err(UdpError::Io)?;
        let (seq, packet) = protocol::decode(&buf[..n]).map_err(UdpError::Decode)?;
        Ok((src, seq, packet))
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
}

impl std::fmt::Display for UdpError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(e) => write!(f, "udp io: {e}"),
            Self::Resolve(s) => write!(f, "could not resolve peer: {s}"),
            Self::Encode(e) => write!(f, "encode: {e}"),
            Self::Decode(e) => write!(f, "decode: {e}"),
        }
    }
}

impl std::error::Error for UdpError {}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::{Edge, Packet};

    #[test]
    fn localhost_roundtrip() {
        let listener = Listener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let mut sender = Sender::connect(&addr.to_string()).unwrap();

        let pkt = Packet::Enter {
            edge: Edge::Right,
            x: 10,
            y: 20,
        };
        let seq = sender.send(&pkt).unwrap();
        assert_eq!(seq, 0);

        let (src, got_seq, got) = listener.recv().unwrap();
        assert_eq!(got_seq, 0);
        assert_eq!(got, pkt);
        assert_eq!(src.ip(), std::net::IpAddr::V4(std::net::Ipv4Addr::LOCALHOST));
    }
}
