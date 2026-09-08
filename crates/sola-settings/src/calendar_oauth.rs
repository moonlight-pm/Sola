//! Google Calendar PKCE for Settings (blocking; runs on a worker thread).

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use anyhow::{Result, anyhow, bail};
use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use rand::RngCore;
use serde::Deserialize;
use sha2::{Digest, Sha256};
use sola_bus::topics::google_calendar_client_secret;

pub const REDIRECT_PORT: u16 = 8765;
pub const REDIRECT_PATH: &str = "/oauth";
const SCOPES: &str = "https://www.googleapis.com/auth/calendar";
const AUTHORIZE_URL: &str = "https://accounts.google.com/o/oauth2/v2/auth";
const TOKEN_URL: &str = "https://oauth2.googleapis.com/token";
const LOGIN_TIMEOUT: Duration = Duration::from_secs(10 * 60);

pub fn redirect_uri() -> String {
    format!("http://127.0.0.1:{REDIRECT_PORT}{REDIRECT_PATH}")
}

pub struct Flow {
    pub verifier: String,
    pub state: String,
    pub url: String,
}

pub fn begin(client_id: &str) -> Result<Flow> {
    let client_id = client_id.trim();
    if client_id.is_empty() {
        bail!("a Google OAuth Desktop client ID is required");
    }
    let verifier = random_token(48);
    let challenge = URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()));
    let state = random_token(18);
    let url = format!(
        "{AUTHORIZE_URL}?client_id={}&response_type=code&redirect_uri={}&code_challenge_method=S256&code_challenge={challenge}&state={state}&scope={}&access_type=offline&prompt=consent",
        urlencoding::encode(client_id),
        urlencoding::encode(&redirect_uri()),
        urlencoding::encode(SCOPES),
    );
    Ok(Flow {
        verifier,
        state,
        url,
    })
}

fn random_token(bytes: usize) -> String {
    let mut buffer = vec![0u8; bytes];
    rand::rng().fill_bytes(&mut buffer);
    URL_SAFE_NO_PAD.encode(buffer)
}

#[derive(Clone, Debug, Deserialize)]
pub struct TokenResponse {
    pub access_token: String,
    #[serde(default)]
    pub refresh_token: Option<String>,
    #[serde(default)]
    pub expires_in: Option<u64>,
}

pub fn sign_in(client_id: String) -> Result<(TokenResponse, String)> {
    let flow = begin(&client_id)?;
    let listener = bind_listener()?;
    sola_core::open_url(&flow.url).map_err(|e| anyhow!(e))?;
    let code = wait_for_code(listener, &flow.state)?;
    let tok = exchange_code(&client_id, &code, &flow.verifier)?;
    let email = user_email(&tok.access_token).unwrap_or_else(|_| "Google".into());
    Ok((tok, email))
}

fn bind_listener() -> Result<TcpListener> {
    let listener = TcpListener::bind(("127.0.0.1", REDIRECT_PORT))
        .map_err(|e| anyhow!("unable to listen on 127.0.0.1:{REDIRECT_PORT}: {e}"))?;
    listener.set_nonblocking(true)?;
    Ok(listener)
}

fn wait_for_code(listener: TcpListener, expected_state: &str) -> Result<String> {
    let deadline = Instant::now() + LOGIN_TIMEOUT;
    loop {
        if Instant::now() > deadline {
            bail!("sign-in timed out; try again");
        }
        match listener.accept() {
            Ok((mut stream, _)) => {
                if let Ok(code) = handle_redirect(&mut stream, expected_state) {
                    return Ok(code);
                }
            }
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                std::thread::sleep(Duration::from_millis(50));
            }
            Err(e) => bail!("redirect listener failed: {e}"),
        }
    }
}

fn handle_redirect(stream: &mut TcpStream, expected_state: &str) -> Result<String> {
    stream.set_read_timeout(Some(Duration::from_secs(5)))?;
    let mut buf = [0u8; 4096];
    let n = stream.read(&mut buf).unwrap_or(0);
    let req = String::from_utf8_lossy(&buf[..n]);
    let line = req.lines().next().unwrap_or("");
    let outcome = parse_request_line(line, expected_state);
    let (status, body) = match &outcome {
        Ok(_) => ("200 OK", success_page()),
        Err(error) => ("400 Bad Request", failure_page(&error.to_string())),
    };
    let response = format!(
        "HTTP/1.1 {status}\r\nContent-Type: text/html; charset=utf-8\r\nCache-Control: no-store\r\nConnection: close\r\nContent-Length: {}\r\n\r\n{body}",
        body.len()
    );
    let _ = stream.write_all(response.as_bytes());
    let _ = stream.flush();
    outcome
}

fn parse_request_line(line: &str, expected_state: &str) -> Result<String> {
    let target = line
        .split_whitespace()
        .nth(1)
        .ok_or_else(|| anyhow!("malformed request"))?;
    let (path, query) = target.split_once('?').unwrap_or((target, ""));
    if path != REDIRECT_PATH {
        bail!("unexpected path {path}");
    }
    let mut code = None;
    let mut state = None;
    let mut error = None;
    for pair in query.split('&') {
        let (key, value) = pair.split_once('=').unwrap_or((pair, ""));
        let value = urlencoding::decode(value)
            .map(|v| v.into_owned())
            .unwrap_or_else(|_| value.to_string());
        match key {
            "code" => code = Some(value),
            "state" => state = Some(value),
            "error" => error = Some(value),
            _ => {}
        }
    }
    if let Some(error) = error {
        bail!("Google refused the sign-in: {error}");
    }
    if state.as_deref() != Some(expected_state) {
        bail!("state mismatch");
    }
    code.ok_or_else(|| anyhow!("Google did not return an authorization code"))
}

fn success_page() -> String {
    "<html><body style=\"font-family:sans-serif;background:#0c0e12;color:#e8eaed;padding:48px\"><h1>Signed in</h1><p>You can close this tab and return to Settings.</p></body></html>".into()
}

fn failure_page(error: &str) -> String {
    format!(
        "<html><body style=\"font-family:sans-serif;background:#0c0e12;color:#e8eaed;padding:48px\"><h1>Sign-in did not finish</h1><p>{}</p></body></html>",
        error.replace('&', "&amp;").replace('<', "&lt;")
    )
}

fn exchange_code(client_id: &str, code: &str, verifier: &str) -> Result<TokenResponse> {
    let secret = google_calendar_client_secret();
    let body = format!(
        "client_id={}&client_secret={}&grant_type=authorization_code&code={}&redirect_uri={}&code_verifier={}",
        urlencoding::encode(client_id),
        urlencoding::encode(&secret),
        urlencoding::encode(code),
        urlencoding::encode(&redirect_uri()),
        urlencoding::encode(verifier),
    );
    token_request(&body)
}

fn token_request(body: &str) -> Result<TokenResponse> {
    let mut response = ureq::post(TOKEN_URL)
        .header("Content-Type", "application/x-www-form-urlencoded")
        .config()
        .http_status_as_error(false)
        .build()
        .send(body)
        .map_err(|e| anyhow!("token request failed: {e}"))?;
    let status = response.status();
    let text = response
        .body_mut()
        .read_to_string()
        .map_err(|e| anyhow!("token body: {e}"))?;
    if !status.is_success() {
        bail!("token request failed ({status}): {text}");
    }
    serde_json::from_str(&text).map_err(|e| anyhow!("token JSON: {e}"))
}

fn user_email(access_token: &str) -> Result<String> {
    let mut response = ureq::get("https://www.googleapis.com/calendar/v3/users/me/calendarList/primary")
        .header("Authorization", &format!("Bearer {access_token}"))
        .call()
        .map_err(|e| anyhow!("primary calendar: {e}"))?;
    let text = response
        .body_mut()
        .read_to_string()
        .unwrap_or_default();
    let v: serde_json::Value = serde_json::from_str(&text)?;
    Ok(v["id"].as_str().unwrap_or("Google").to_string())
}

pub fn expiry_unix(expires_in: Option<u64>) -> u64 {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    now + expires_in.unwrap_or(3600).saturating_sub(60)
}
