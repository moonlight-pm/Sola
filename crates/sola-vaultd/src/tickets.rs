//! Delivery tickets (P4). The shell asks for an item's value to go to an
//! app's focused field; the daemon keeps the value against a one-time
//! ticket bound to that app's pid, and the app redeems it on this socket.
//! The ticket rides the bus; the value never does. Tickets last 60 s and
//! are spent on the first good redeem.

use std::collections::HashMap;
use std::time::{Duration, Instant};

use zeroize::Zeroizing;

/// How long a ticket stands.
pub const TICKET_TTL: Duration = Duration::from_secs(60);

struct Ticket {
    pid: u32,
    value: Zeroizing<String>,
    until: Instant,
}

/// Why a redeem gave nothing.
#[derive(Debug, PartialEq, Eq)]
pub enum RedeemError {
    /// No such ticket, or it ran out or was spent.
    Unknown,
    /// The ticket was made for another process. It stands for its own.
    WrongProcess,
}

#[derive(Default)]
pub struct Tickets {
    open: HashMap<String, Ticket>,
}

impl Tickets {
    /// A new ticket (128 random bits, hex) for `value`, redeemable by
    /// process `pid` until `now` + [`TICKET_TTL`].
    pub fn issue(&mut self, pid: u32, value: Zeroizing<String>, now: Instant) -> String {
        self.expire(now);
        let mut bytes = [0u8; 16];
        getrandom::getrandom(&mut bytes).expect("os random");
        let ticket: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
        self.open.insert(
            ticket.clone(),
            Ticket {
                pid,
                value,
                until: now + TICKET_TTL,
            },
        );
        ticket
    }

    /// The value, once, to the process the ticket was made for.
    pub fn redeem(&mut self, ticket: &str, pid: u32, now: Instant) -> Result<Zeroizing<String>, RedeemError> {
        self.expire(now);
        match self.open.get(ticket) {
            None => Err(RedeemError::Unknown),
            // Not spent: a process that read the ticket off the bus cannot
            // take it from the one it was made for.
            Some(t) if t.pid != pid => Err(RedeemError::WrongProcess),
            Some(_) => Ok(self.open.remove(ticket).expect("present").value),
        }
    }

    /// Drop every ticket (the vault locked or signed out).
    pub fn clear(&mut self) {
        self.open.clear();
    }

    fn expire(&mut self, now: Instant) {
        self.open.retain(|_, t| t.until > now);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn value(s: &str) -> Zeroizing<String> {
        Zeroizing::new(s.to_string())
    }

    #[test]
    fn a_ticket_is_128_bit_hex_and_unique() {
        let mut t = Tickets::default();
        let now = Instant::now();
        let a = t.issue(7, value("x"), now);
        let b = t.issue(7, value("x"), now);
        assert_eq!(a.len(), 32);
        assert!(a.chars().all(|c| c.is_ascii_hexdigit()));
        assert_ne!(a, b);
    }

    #[test]
    fn redeemed_once_by_its_own_process() {
        let mut t = Tickets::default();
        let now = Instant::now();
        let ticket = t.issue(7, value("hunter2"), now);
        assert_eq!(t.redeem(&ticket, 8, now), Err(RedeemError::WrongProcess));
        assert_eq!(t.redeem(&ticket, 7, now).unwrap().as_str(), "hunter2");
        assert_eq!(t.redeem(&ticket, 7, now), Err(RedeemError::Unknown));
    }

    #[test]
    fn a_ticket_runs_out_after_a_minute() {
        let mut t = Tickets::default();
        let now = Instant::now();
        let ticket = t.issue(7, value("x"), now);
        let later = now + TICKET_TTL + Duration::from_millis(1);
        assert_eq!(t.redeem(&ticket, 7, later), Err(RedeemError::Unknown));
    }

    #[test]
    fn clear_drops_every_ticket() {
        let mut t = Tickets::default();
        let now = Instant::now();
        let ticket = t.issue(7, value("x"), now);
        t.clear();
        assert_eq!(t.redeem(&ticket, 7, now), Err(RedeemError::Unknown));
        assert_eq!(t.redeem("nonsense", 7, now), Err(RedeemError::Unknown));
    }
}
