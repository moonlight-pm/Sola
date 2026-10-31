//! Oath's login service (`svc:login`): the desk user's password and
//! auto-login, and the word that the desk ended on purpose. Oath's contract
//! is its `docs/specs/login.md`: a Unix socket at [`SOCKET`], one JSON object
//! per line each way, a refusal as `{"ok":false,"error":"…"}`.
//!
//! Without the socket (not Oath, or an Oath without `svc:login`) every call
//! is [`Error::Unavailable`]: no password, nothing to lock with.

use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::time::Duration;

use serde::Deserialize;
use serde_json::{Value, json};

/// Where Oath's login service listens.
pub const SOCKET: &str = "/oath/run/login.sock";

/// A wrong password is answered after up to 30 s (Oath doubles the delay);
/// wait past that.
const TIMEOUT: Duration = Duration::from_secs(45);

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Status {
    /// A password is set.
    pub password: bool,
    /// The desk starts signed in (and the console after Exit Sola).
    pub autologin: bool,
    /// Who has the seat: `desk` or `console`.
    #[serde(default)]
    pub seat: String,
}

impl Status {
    /// The desk locks as it starts: a password, and no auto-login.
    pub fn locks_at_start(&self) -> bool {
        self.password && !self.autologin
    }
}

#[derive(Debug)]
pub enum Error {
    /// No login service here.
    Unavailable,
    Io(std::io::Error),
    /// The service said no (`error`): a wrong old password, say.
    Refused(String),
    /// An answer that is not the contract's.
    Protocol(String),
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Unavailable => write!(f, "Oath's login service is not running here"),
            Self::Io(e) => write!(f, "login service: {e}"),
            Self::Refused(why) => write!(f, "{why}"),
            Self::Protocol(why) => write!(f, "login service answered oddly: {why}"),
        }
    }
}

impl std::error::Error for Error {}

/// The service's socket: [`SOCKET`], or `SOLA_LOGIN_SOCK` (tests and a
/// nested session).
pub fn socket_path() -> PathBuf {
    std::env::var_os("SOLA_LOGIN_SOCK")
        .filter(|p| !p.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(SOCKET))
}

/// The service is here.
pub fn available() -> bool {
    socket_path().exists()
}

pub fn status() -> Result<Status, Error> {
    let v = call(json!({ "call": "status" }))?;
    serde_json::from_value(v).map_err(|e| Error::Protocol(e.to_string()))
}

/// Is `password` the desk user's? A wrong one is answered late.
pub fn check(password: &str) -> Result<bool, Error> {
    let v = call(json!({ "call": "check", "password": password }))?;
    v.get("right")
        .and_then(Value::as_bool)
        .ok_or_else(|| Error::Protocol("check without `right`".into()))
}

/// Set the password; an empty `new` removes it. `old` matters only while
/// one is set.
pub fn set(old: &str, new: &str) -> Result<(), Error> {
    call(json!({ "call": "set", "old": old, "new": new })).map(drop)
}

/// Auto-login on or off; `password` matters only while one is set.
pub fn autologin(on: bool, password: &str) -> Result<(), Error> {
    call(json!({ "call": "autologin", "on": on, "password": password })).map(drop)
}

/// The desk ended on purpose (Exit Sola): Oath gives the seat to the
/// console.
pub fn desk_down() -> Result<(), Error> {
    call(json!({ "call": "desk", "up": false })).map(drop)
}

fn call(request: Value) -> Result<Value, Error> {
    let path = socket_path();
    let stream = match UnixStream::connect(&path) {
        Ok(s) => s,
        Err(e)
            if matches!(
                e.kind(),
                std::io::ErrorKind::NotFound | std::io::ErrorKind::ConnectionRefused
            ) =>
        {
            return Err(Error::Unavailable);
        }
        Err(e) => return Err(Error::Io(e)),
    };
    stream.set_read_timeout(Some(TIMEOUT)).map_err(Error::Io)?;
    let mut line = request.to_string();
    line.push('\n');
    (&stream).write_all(line.as_bytes()).map_err(Error::Io)?;
    let mut answer = String::new();
    BufReader::new(&stream)
        .read_line(&mut answer)
        .map_err(Error::Io)?;
    let v: Value =
        serde_json::from_str(answer.trim()).map_err(|e| Error::Protocol(e.to_string()))?;
    match v.get("ok").and_then(Value::as_bool) {
        Some(true) => Ok(v),
        Some(false) => Err(Error::Refused(
            v.get("error")
                .and_then(Value::as_str)
                .unwrap_or("refused")
                .to_string(),
        )),
        None => Err(Error::Protocol(format!("no `ok` in {answer}"))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::net::UnixListener;
    use std::sync::Mutex;

    /// `SOLA_LOGIN_SOCK` is process-wide: one test at a time.
    static ENV: Mutex<()> = Mutex::new(());

    /// A stand-in service: password "pw", auto-login on.
    fn serve(dir: &std::path::Path) -> PathBuf {
        let path = dir.join("login.sock");
        let listener = UnixListener::bind(&path).unwrap();
        std::thread::spawn(move || {
            let mut password = Some("pw".to_string());
            for conn in listener.incoming().flatten() {
                let mut reader = BufReader::new(&conn);
                let mut line = String::new();
                while reader.read_line(&mut line).unwrap_or(0) > 0 {
                    let req: Value = serde_json::from_str(line.trim()).unwrap();
                    let answer = match req["call"].as_str().unwrap() {
                        "status" => json!({"ok": true, "password": password.is_some(), "autologin": true, "seat": "desk"}),
                        "check" => json!({"ok": true, "right": password.as_deref() == req["password"].as_str()}),
                        "set" if password.as_deref().is_some_and(|p| Some(p) != req["old"].as_str()) => {
                            json!({"ok": false, "error": "wrong password"})
                        }
                        "set" => {
                            let new = req["new"].as_str().unwrap();
                            password = (!new.is_empty()).then(|| new.to_string());
                            json!({"ok": true})
                        }
                        _ => json!({"ok": true}),
                    };
                    (&conn).write_all(format!("{answer}\n").as_bytes()).unwrap();
                    line.clear();
                }
            }
        });
        path
    }

    #[test]
    fn the_calls_follow_oaths_contract() {
        let _env = ENV.lock().unwrap_or_else(|e| e.into_inner());
        let dir = std::env::temp_dir().join(format!("sola-login-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        unsafe { std::env::set_var("SOLA_LOGIN_SOCK", serve(&dir)) };

        let s = status().unwrap();
        assert!(s.password && s.autologin && !s.locks_at_start());
        assert_eq!(s.seat, "desk");
        assert!(check("pw").unwrap());
        assert!(!check("nope").unwrap());
        assert!(matches!(set("nope", "new"), Err(Error::Refused(why)) if why == "wrong password"));
        set("pw", "").unwrap();
        assert!(!status().unwrap().password);
        desk_down().unwrap();

        unsafe { std::env::set_var("SOLA_LOGIN_SOCK", dir.join("gone.sock")) };
        assert!(matches!(status(), Err(Error::Unavailable)));
        assert!(!available());
        unsafe { std::env::remove_var("SOLA_LOGIN_SOCK") };
        let _ = std::fs::remove_dir_all(dir);
    }
}
