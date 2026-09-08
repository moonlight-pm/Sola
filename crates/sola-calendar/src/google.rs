//! Google Calendar API v3 + Authorization Code PKCE.

use std::net::SocketAddr;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result, anyhow, bail};
use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use chrono::{DateTime, NaiveDate, Utc};
use rand::RngCore;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpListener;
use tokio::sync::watch;

use crate::model::{CalEvent, Calendar, CalKind};

pub const REDIRECT_PORT: u16 = 8765;
pub const REDIRECT_PATH: &str = "/oauth";
pub const SCOPES: &str = "https://www.googleapis.com/auth/calendar";
const AUTHORIZE_URL: &str = "https://accounts.google.com/o/oauth2/v2/auth";
const TOKEN_URL: &str = "https://oauth2.googleapis.com/token";
const API: &str = "https://www.googleapis.com/calendar/v3";
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

pub async fn wait_for_code(
    expected_state: &str,
    mut cancel: watch::Receiver<bool>,
) -> Result<String> {
    let address: SocketAddr = ([127, 0, 0, 1], REDIRECT_PORT).into();
    let listener = TcpListener::bind(address)
        .await
        .with_context(|| format!("unable to listen on {address} for the Google redirect"))?;
    let deadline = tokio::time::sleep(LOGIN_TIMEOUT);
    tokio::pin!(deadline);
    loop {
        let (mut stream, _) = tokio::select! {
            accepted = listener.accept() => accepted.context("redirect listener failed")?,
            _ = cancel.changed() => {
                if *cancel.borrow() { bail!("sign-in cancelled"); }
                continue;
            }
            _ = &mut deadline => bail!("sign-in timed out; try again"),
        };
        let mut reader = BufReader::new(&mut stream);
        let mut request_line = String::new();
        if reader.read_line(&mut request_line).await.is_err() {
            continue;
        }
        let outcome = parse_request_line(&request_line, expected_state);
        let (status, body) = match &outcome {
            Ok(_) => ("200 OK", success_page()),
            Err(error) => ("400 Bad Request", failure_page(&error.to_string())),
        };
        let response = format!(
            "HTTP/1.1 {status}\r\nContent-Type: text/html; charset=utf-8\r\nCache-Control: no-store\r\nConnection: close\r\nContent-Length: {}\r\n\r\n{body}",
            body.len()
        );
        let _ = stream.write_all(response.as_bytes()).await;
        let _ = stream.shutdown().await;
        match outcome {
            Ok(code) => return Ok(code),
            Err(error) => {
                tracing::debug!("ignored Google redirect request: {error}");
                continue;
            }
        }
    }
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
    "<html><body style=\"font-family:sans-serif;background:#0c0e12;color:#e8eaed;padding:48px\"><h1>Signed in</h1><p>You can close this tab and return to Calendar.</p></body></html>".into()
}

fn failure_page(error: &str) -> String {
    format!(
        "<html><body style=\"font-family:sans-serif;background:#0c0e12;color:#e8eaed;padding:48px\"><h1>Sign-in did not finish</h1><p>{}</p></body></html>",
        html_escape(error)
    )
}

fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

pub async fn exchange_code(
    http: &reqwest::Client,
    client_id: &str,
    code: &str,
    verifier: &str,
) -> Result<TokenResponse> {
    token_request(
        http,
        &[
            ("client_id", client_id),
            ("grant_type", "authorization_code"),
            ("code", code),
            ("redirect_uri", &redirect_uri()),
            ("code_verifier", verifier),
        ],
    )
    .await
}

pub async fn refresh(
    http: &reqwest::Client,
    client_id: &str,
    refresh_token: &str,
) -> Result<TokenResponse> {
    token_request(
        http,
        &[
            ("client_id", client_id),
            ("grant_type", "refresh_token"),
            ("refresh_token", refresh_token),
        ],
    )
    .await
}

async fn token_request(http: &reqwest::Client, form: &[(&str, &str)]) -> Result<TokenResponse> {
    let response = http.post(TOKEN_URL).form(form).send().await?;
    let status = response.status();
    let text = response.text().await.unwrap_or_default();
    if !status.is_success() {
        bail!("token request failed ({status}): {text}");
    }
    serde_json::from_str(&text).context("token JSON")
}

pub fn expiry_unix(expires_in: Option<u64>) -> u64 {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    now + expires_in.unwrap_or(3600).saturating_sub(60)
}

pub fn access_expired(expiry_unix: u64) -> bool {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    now + 30 >= expiry_unix
}

#[derive(Debug, Deserialize)]
struct CalendarList {
    #[serde(default)]
    items: Vec<CalendarListEntry>,
}

#[derive(Debug, Deserialize)]
struct CalendarListEntry {
    id: String,
    #[serde(default)]
    summary: String,
    #[serde(default)]
    #[serde(rename = "backgroundColor")]
    background_color: Option<String>,
    #[serde(default)]
    #[serde(rename = "accessRole")]
    access_role: Option<String>,
}

pub async fn list_calendars(
    http: &reqwest::Client,
    token: &str,
    account_id: &str,
) -> Result<Vec<Calendar>> {
    let url = format!("{API}/users/me/calendarList");
    let list: CalendarList = authed_get(http, token, &url).await?;
    Ok(list
        .items
        .into_iter()
        .map(|item| {
            let read_only = matches!(
                item.access_role.as_deref(),
                Some("reader") | Some("freeBusyReader")
            );
            Calendar {
                id: format!("g-{account_id}-{}", sanitize_id(&item.id)),
                name: if item.summary.is_empty() {
                    item.id.clone()
                } else {
                    item.summary
                },
                color: item.background_color.unwrap_or_else(|| "#7aa2f7".into()),
                kind: CalKind::Google,
                account_id: Some(account_id.into()),
                visible: true,
                read_only,
                remote_id: Some(item.id),
                href: None,
            }
        })
        .collect())
}

#[derive(Debug, Deserialize)]
struct EventList {
    #[serde(default)]
    items: Vec<ApiEvent>,
    #[serde(default, rename = "nextPageToken")]
    next_page_token: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
struct ApiEvent {
    #[serde(default)]
    id: String,
    #[serde(default)]
    summary: Option<String>,
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    location: Option<String>,
    #[serde(default)]
    status: Option<String>,
    start: Option<ApiWhen>,
    end: Option<ApiWhen>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
struct ApiWhen {
    #[serde(default, rename = "dateTime")]
    date_time: Option<String>,
    #[serde(default)]
    date: Option<String>,
}

pub async fn list_events(
    http: &reqwest::Client,
    token: &str,
    calendar: &Calendar,
    window: (NaiveDate, NaiveDate),
) -> Result<Vec<CalEvent>> {
    let Some(remote) = calendar.remote_id.as_deref() else {
        return Ok(Vec::new());
    };
    let time_min = window.0.and_hms_opt(0, 0, 0).unwrap().and_utc();
    let time_max = (window.1 + chrono::Duration::days(1))
        .and_hms_opt(0, 0, 0)
        .unwrap()
        .and_utc();
    let mut page: Option<String> = None;
    let mut out = Vec::new();
    loop {
        let mut url = format!(
            "{API}/calendars/{}/events?singleEvents=true&maxResults=2500&timeMin={}&timeMax={}",
            urlencoding::encode(remote),
            urlencoding::encode(&time_min.to_rfc3339()),
            urlencoding::encode(&time_max.to_rfc3339()),
        );
        if let Some(token) = &page {
            url.push_str("&pageToken=");
            url.push_str(&urlencoding::encode(token));
        }
        let list: EventList = authed_get(http, token, &url).await?;
        for item in list.items {
            if item.status.as_deref() == Some("cancelled") {
                continue;
            }
            if let Some(ev) = api_to_event(item, calendar) {
                out.push(ev);
            }
        }
        match list.next_page_token {
            Some(t) if !t.is_empty() => page = Some(t),
            _ => break,
        }
    }
    Ok(out)
}

pub async fn insert_event(
    http: &reqwest::Client,
    token: &str,
    calendar: &Calendar,
    event: &CalEvent,
) -> Result<CalEvent> {
    let remote = calendar
        .remote_id
        .as_deref()
        .ok_or_else(|| anyhow!("missing Google calendar id"))?;
    let url = format!("{API}/calendars/{}/events", urlencoding::encode(remote));
    let body = event_to_api(event);
    let created: ApiEvent = authed_json(http, token, reqwest::Method::POST, &url, &body).await?;
    api_to_event(created, calendar).ok_or_else(|| anyhow!("Google returned an empty event"))
}

pub async fn patch_event(
    http: &reqwest::Client,
    token: &str,
    calendar: &Calendar,
    event: &CalEvent,
) -> Result<CalEvent> {
    let remote_cal = calendar
        .remote_id
        .as_deref()
        .ok_or_else(|| anyhow!("missing Google calendar id"))?;
    let remote_ev = event
        .remote_id
        .as_deref()
        .ok_or_else(|| anyhow!("missing Google event id"))?;
    let url = format!(
        "{API}/calendars/{}/events/{}",
        urlencoding::encode(remote_cal),
        urlencoding::encode(remote_ev)
    );
    let body = event_to_api(event);
    let updated: ApiEvent = authed_json(http, token, reqwest::Method::PATCH, &url, &body).await?;
    api_to_event(updated, calendar).ok_or_else(|| anyhow!("Google returned an empty event"))
}

pub async fn delete_event(
    http: &reqwest::Client,
    token: &str,
    calendar: &Calendar,
    event: &CalEvent,
) -> Result<()> {
    let remote_cal = calendar
        .remote_id
        .as_deref()
        .ok_or_else(|| anyhow!("missing Google calendar id"))?;
    let remote_ev = event
        .remote_id
        .as_deref()
        .ok_or_else(|| anyhow!("missing Google event id"))?;
    let url = format!(
        "{API}/calendars/{}/events/{}",
        urlencoding::encode(remote_cal),
        urlencoding::encode(remote_ev)
    );
    let response = http
        .delete(&url)
        .bearer_auth(token)
        .send()
        .await?;
    if !response.status().is_success() && response.status().as_u16() != 404 {
        let text = response.text().await.unwrap_or_default();
        bail!("delete failed: {text}");
    }
    Ok(())
}

pub async fn user_email(http: &reqwest::Client, token: &str) -> Result<String> {
    let url = format!("{API}/users/me/calendarList/primary");
    #[derive(Deserialize)]
    struct Primary {
        #[serde(default)]
        id: String,
    }
    let primary: Primary = authed_get(http, token, &url).await?;
    Ok(primary.id)
}

async fn authed_get<T: for<'de> Deserialize<'de>>(
    http: &reqwest::Client,
    token: &str,
    url: &str,
) -> Result<T> {
    let response = http.get(url).bearer_auth(token).send().await?;
    let status = response.status();
    let text = response.text().await.unwrap_or_default();
    if !status.is_success() {
        bail!("GET {url} failed ({status}): {text}");
    }
    serde_json::from_str(&text).with_context(|| format!("decode {url}"))
}

async fn authed_json<T: for<'de> Deserialize<'de>, B: Serialize>(
    http: &reqwest::Client,
    token: &str,
    method: reqwest::Method,
    url: &str,
    body: &B,
) -> Result<T> {
    let response = http
        .request(method, url)
        .bearer_auth(token)
        .json(body)
        .send()
        .await?;
    let status = response.status();
    let text = response.text().await.unwrap_or_default();
    if !status.is_success() {
        bail!("request {url} failed ({status}): {text}");
    }
    serde_json::from_str(&text).with_context(|| format!("decode {url}"))
}

fn event_to_api(event: &CalEvent) -> ApiEvent {
    let (start, end) = if event.all_day {
        let start = event
            .start_date
            .unwrap_or_else(|| event.start.with_timezone(&chrono::Local).date_naive());
        let end = event.end_date.unwrap_or(start + chrono::Duration::days(1));
        (
            ApiWhen {
                date_time: None,
                date: Some(start.to_string()),
            },
            ApiWhen {
                date_time: None,
                date: Some(end.to_string()),
            },
        )
    } else {
        (
            ApiWhen {
                date_time: Some(event.start.to_rfc3339()),
                date: None,
            },
            ApiWhen {
                date_time: Some(event.end.to_rfc3339()),
                date: None,
            },
        )
    };
    ApiEvent {
        id: event.remote_id.clone().unwrap_or_default(),
        summary: Some(event.title.clone()),
        description: Some(event.notes.clone()),
        location: Some(event.location.clone()),
        status: None,
        start: Some(start),
        end: Some(end),
    }
}

fn api_to_event(item: ApiEvent, calendar: &Calendar) -> Option<CalEvent> {
    let start_w = item.start?;
    let end_w = item.end.clone().unwrap_or_else(|| start_w.clone());
    let (all_day, start, end, start_date, end_date) = parse_when(&start_w, &end_w)?;
    Some(CalEvent {
        id: format!("{}:{}", calendar.id, item.id),
        calendar_id: calendar.id.clone(),
        title: item.summary.unwrap_or_default(),
        notes: item.description.unwrap_or_default(),
        location: item.location.unwrap_or_default(),
        all_day,
        start,
        end,
        start_date,
        end_date,
        remote_id: Some(item.id),
        href: None,
        etag: None,
        read_only: calendar.read_only,
        rrule: None,
        master_id: None,
    })
}

fn parse_when(
    start: &ApiWhen,
    end: &ApiWhen,
) -> Option<(
    bool,
    DateTime<Utc>,
    DateTime<Utc>,
    Option<NaiveDate>,
    Option<NaiveDate>,
)> {
    if let (Some(s), Some(e)) = (&start.date, &end.date) {
        let sd = NaiveDate::parse_from_str(s, "%Y-%m-%d").ok()?;
        let ed = NaiveDate::parse_from_str(e, "%Y-%m-%d").ok()?;
        let start_utc = sd
            .and_hms_opt(0, 0, 0)?
            .and_local_timezone(chrono::Local)
            .single()
            .map(|d| d.with_timezone(&Utc))
            .unwrap_or_else(|| sd.and_hms_opt(0, 0, 0).unwrap().and_utc());
        let end_utc = ed
            .and_hms_opt(0, 0, 0)?
            .and_local_timezone(chrono::Local)
            .single()
            .map(|d| d.with_timezone(&Utc))
            .unwrap_or_else(|| ed.and_hms_opt(0, 0, 0).unwrap().and_utc());
        return Some((true, start_utc, end_utc, Some(sd), Some(ed)));
    }
    let start_utc = DateTime::parse_from_rfc3339(start.date_time.as_deref()?)
        .ok()?
        .with_timezone(&Utc);
    let end_utc = DateTime::parse_from_rfc3339(end.date_time.as_deref()?)
        .ok()?
        .with_timezone(&Utc);
    Some((false, start_utc, end_utc, None, None))
}

fn sanitize_id(s: &str) -> String {
    s.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() {
                c
            } else {
                '-'
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_all_day_when() {
        let start = ApiWhen {
            date_time: None,
            date: Some("2026-09-08".into()),
        };
        let end = ApiWhen {
            date_time: None,
            date: Some("2026-09-09".into()),
        };
        let (all_day, _, _, sd, ed) = parse_when(&start, &end).unwrap();
        assert!(all_day);
        assert_eq!(sd.unwrap().to_string(), "2026-09-08");
        assert_eq!(ed.unwrap().to_string(), "2026-09-09");
    }
}
