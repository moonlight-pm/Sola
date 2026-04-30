//! `solactl logs` — tail an app's log file at /opt/sola/log/<app>.log.
//!
//! Without `--follow`, prints the existing file content and exits.
//! With `--follow`, continues to print new lines until interrupted.

use std::io::{BufRead, BufReader, Read, Seek, SeekFrom};
use std::thread::sleep;
use std::time::Duration;

const LOG_DIR: &str = "/opt/sola/log";

pub fn run(app: Option<&str>, follow: bool) -> i32 {
    let app = app.unwrap_or("sola");
    let path = format!("{LOG_DIR}/{app}.log");

    let mut file = match std::fs::File::open(&path) {
        Ok(f) => f,
        Err(e) => {
            eprintln!("solactl: cannot open {path}: {e}");
            return 3;
        }
    };

    // Print everything currently in the file.
    let mut reader = BufReader::new(&mut file);
    let mut line = String::new();
    loop {
        line.clear();
        match reader.read_line(&mut line) {
            Ok(0) => break,
            Ok(_) => {
                print!("{line}");
            }
            Err(e) => {
                eprintln!("solactl: read error on {path}: {e}");
                return 3;
            }
        }
    }

    if !follow {
        return 0;
    }

    // Drop the BufReader so we can keep using `file` directly. Track the
    // position we left off at; poll for growth and print new lines.
    drop(reader);
    let mut pos = file.seek(SeekFrom::End(0)).unwrap_or(0);
    loop {
        sleep(Duration::from_millis(200));
        let len = match file.metadata() {
            Ok(m) => m.len(),
            Err(_) => continue,
        };
        if len < pos {
            // File was rotated/truncated; reset to beginning.
            pos = 0;
        }
        if len == pos {
            continue;
        }
        if file.seek(SeekFrom::Start(pos)).is_err() {
            continue;
        }
        let mut buf = Vec::with_capacity((len - pos) as usize);
        if (&file).take(len - pos).read_to_end(&mut buf).is_err() {
            continue;
        }
        if let Ok(s) = std::str::from_utf8(&buf) {
            print!("{s}");
        } else {
            // Best-effort lossy print on partial/invalid utf-8.
            print!("{}", String::from_utf8_lossy(&buf));
        }
        pos = len;
    }
}
