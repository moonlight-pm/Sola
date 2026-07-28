//! UDP receive loop → inject.

use std::net::UdpSocket;
use std::time::Duration;

use tracing::{debug, error, info, warn};

use crate::inject::{CgInjector, Injector};
use crate::protocol::{self, MAX_PACKET_LEN};

/// Bind UDP and inject forever.
pub fn run(bind: &str) -> Result<(), AgentError> {
    let sock = UdpSocket::bind(bind).map_err(AgentError::Io)?;
    sock.set_read_timeout(Some(Duration::from_secs(1)))
        .map_err(AgentError::Io)?;
    let local = sock.local_addr().ok();
    info!(?local, "sola-kvm-mac listening (UDP)");

    // Surface AX trust failures immediately (cursor warp still works without it;
    // clicks/keys do not).
    crate::inject::check_accessibility_at_startup();

    let mut injector = CgInjector::new();
    let mut buf = [0u8; 256];
    let mut last_seq: Option<u32> = None;
    let mut ok_count: u64 = 0;
    let mut err_count: u64 = 0;

    loop {
        match sock.recv_from(&mut buf) {
            Ok((n, src)) => {
                if n > MAX_PACKET_LEN {
                    warn!(%src, n, "oversized datagram; decoding first bytes only");
                }
                let slice = &buf[..n.min(buf.len())];
                match protocol::decode(slice) {
                    Ok((seq, packet)) => {
                        if let Some(prev) = last_seq {
                            let expected = prev.wrapping_add(1);
                            if seq != expected {
                                // Soft warn only — UDP reordering/loss is possible.
                                warn!(seq, expected, prev, %src, "seq gap");
                            }
                        }
                        last_seq = Some(seq);
                        ok_count += 1;
                        // Motion is high-rate; logging every packet at info stalls inject.
                        // Enter/Leave stay visible at info for session debugging.
                        match &packet {
                            protocol::Packet::Enter { .. } | protocol::Packet::Leave => {
                                info!(%src, seq, ?packet, ok_count, "recv");
                            }
                            protocol::Packet::Motion { .. } => {
                                debug!(%src, seq, ?packet, ok_count, "recv");
                            }
                            _ => {
                                debug!(%src, seq, ?packet, ok_count, "recv");
                            }
                        }
                        injector.handle(&packet);
                    }
                    Err(e) => {
                        err_count += 1;
                        warn!(%src, err_count, error = %e, "decode failed");
                    }
                }
            }
            Err(e)
                if e.kind() == std::io::ErrorKind::WouldBlock
                    || e.kind() == std::io::ErrorKind::TimedOut =>
            {
                // Idle tick; keeps the loop interruptible.
            }
            Err(e) => {
                error!(error = %e, "recv error");
                std::thread::sleep(Duration::from_millis(50));
            }
        }
    }
}

#[derive(Debug)]
pub enum AgentError {
    Io(std::io::Error),
}

impl std::fmt::Display for AgentError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(e) => write!(f, "io: {e}"),
        }
    }
}

impl std::error::Error for AgentError {}

/// Resolve a bind string (tests).
#[cfg(test)]
pub fn parse_bind(s: &str) -> Result<std::net::SocketAddr, String> {
    s.parse().map_err(|e| format!("{s}: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::{encode, Edge, Packet};
    use std::net::UdpSocket;
    use std::thread;
    use std::time::Duration;

    #[test]
    fn parse_default_bind() {
        let a = parse_bind("0.0.0.0:4242").unwrap();
        assert_eq!(a.port(), 4242);
    }

    #[test]
    fn localhost_udp_decode_path() {
        let server = UdpSocket::bind("127.0.0.1:0").unwrap();
        let addr = server.local_addr().unwrap();
        server
            .set_read_timeout(Some(Duration::from_millis(500)))
            .unwrap();

        let client = UdpSocket::bind("127.0.0.1:0").unwrap();
        let packet = Packet::Enter {
            edge: Edge::Right,
            x: 100,
            y: 200,
        };
        let bytes = encode::encode(7, &packet);
        client.send_to(&bytes, addr).unwrap();

        let mut buf = [0u8; 64];
        let (n, src) = server.recv_from(&mut buf).unwrap();
        assert_eq!(src.ip().to_string(), "127.0.0.1");
        let (seq, got) = protocol::decode(&buf[..n]).unwrap();
        assert_eq!(seq, 7);
        assert_eq!(got, packet);

        thread::sleep(Duration::from_millis(1));
    }
}
