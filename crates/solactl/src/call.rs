//! Invoke a sola-call method and print the reply.

use std::io::{self, Write};
use std::time::Duration;

use sola_call::{CallError, invoke};

/// `solactl browser | head` must not panic on SIGPIPE/EPIPE.
pub(crate) fn print_stdout(s: &str) {
    let mut w = io::stdout();
    match writeln!(w, "{s}") {
        Ok(()) => {}
        Err(e) if e.kind() == io::ErrorKind::BrokenPipe => {}
        Err(e) => {
            let _ = writeln!(io::stderr(), "solactl: {e}");
        }
    }
}

/// 0 success · 1 remote error · 2 timeout · 3 local
pub fn run(owner: &str, method: &str, params: serde_json::Value, timeout_secs: u64) -> i32 {
    match invoke(owner, method, params, Duration::from_secs(timeout_secs)) {
        Ok(data) => {
            if !data.is_null() {
                match serde_json::to_string_pretty(&data) {
                    Ok(s) => print_stdout(&s),
                    Err(_) => print_stdout(&data.to_string()),
                }
            }
            0
        }
        Err(CallError::Remote(e)) => {
            eprintln!("solactl: {e}");
            1
        }
        Err(CallError::Timeout) => {
            eprintln!("solactl: timeout waiting for {owner}.{method}");
            2
        }
        Err(e) => {
            eprintln!("solactl: {e}");
            3
        }
    }
}
