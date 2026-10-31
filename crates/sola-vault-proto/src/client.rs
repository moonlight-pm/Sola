//! A blocking client for the vault's socket. Callers that must not block
//! (an iced update) run it on a thread or in a blocking task.

use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::time::Duration;

use crate::wire::{Envelope, ErrorCode, Event, Reply, Request, Response, VaultError};
use crate::{VERSION, socket_path};

/// Long enough for a login (KDF, two round trips) or a full sync.
const CALL_TIMEOUT: Duration = Duration::from_secs(120);

/// One connection: requests answered in order.
pub struct Client {
    reader: BufReader<UnixStream>,
    writer: UnixStream,
    next: u64,
}

impl Client {
    /// Connect and say hello. `client` names the caller in the daemon's log.
    pub fn connect(client: &str) -> Result<Self, VaultError> {
        let stream = UnixStream::connect(socket_path()).map_err(|e| {
            VaultError::new(ErrorCode::Unavailable, format!("The vault is not running ({e})."))
        })?;
        stream.set_read_timeout(Some(CALL_TIMEOUT)).ok();
        let writer = stream.try_clone().map_err(io_err)?;
        let mut c = Self {
            reader: BufReader::new(stream),
            writer,
            next: 1,
        };
        c.call(Request::Hello {
            version: VERSION,
            client: client.to_string(),
        })?;
        Ok(c)
    }

    /// Send `req` and wait for its answer.
    pub fn call(&mut self, req: Request) -> Result<Reply, VaultError> {
        let id = self.next;
        self.next += 1;
        let mut line = serde_json::to_string(&Envelope { id, body: req })
            .map_err(|e| VaultError::new(ErrorCode::Internal, e.to_string()))?;
        line.push('\n');
        self.writer.write_all(line.as_bytes()).map_err(io_err)?;
        loop {
            let mut buf = String::new();
            if self.reader.read_line(&mut buf).map_err(io_err)? == 0 {
                return Err(VaultError::new(
                    ErrorCode::Unavailable,
                    "The vault closed the connection.",
                ));
            }
            let env: Envelope<Response> = serde_json::from_str(buf.trim_end())
                .map_err(|e| VaultError::new(ErrorCode::Internal, e.to_string()))?;
            zero(&mut buf);
            if env.id != id {
                continue;
            }
            return match env.body {
                Response::Ok(reply) => Ok(reply),
                Response::Err(e) => Err(e),
            };
        }
    }

    /// Turn this connection into an event stream.
    pub fn into_events(mut self) -> Result<Events, VaultError> {
        self.call(Request::Subscribe)?;
        self.reader.get_ref().set_read_timeout(None).ok();
        Ok(Events {
            reader: self.reader,
        })
    }
}

/// Connect, ask once, close.
pub fn call(client: &str, req: Request) -> Result<Reply, VaultError> {
    Client::connect(client)?.call(req)
}

/// The daemon's events, until it goes away.
pub fn subscribe(client: &str) -> Result<Events, VaultError> {
    Client::connect(client)?.into_events()
}

pub struct Events {
    reader: BufReader<UnixStream>,
}

impl Iterator for Events {
    type Item = Event;

    fn next(&mut self) -> Option<Event> {
        loop {
            let mut buf = String::new();
            if self.reader.read_line(&mut buf).ok()? == 0 {
                return None;
            }
            match serde_json::from_str(buf.trim_end()) {
                Ok(ev) => return Some(ev),
                // A newer daemon's event this build does not know.
                Err(_) => continue,
            }
        }
    }
}

fn io_err(e: std::io::Error) -> VaultError {
    VaultError::new(ErrorCode::Unavailable, format!("The vault's socket failed ({e})."))
}

/// Replies can carry secrets: clear the line buffer.
fn zero(buf: &mut String) {
    use zeroize::Zeroize;
    buf.zeroize();
}
