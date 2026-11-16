//! Plain HTTPS to Signal's file servers and its group and storage service,
//! which are not reached through the chat socket. Signal's own roots, as
//! libsignal carries them; blocking, so called from a blocking task.

use std::sync::{Arc, OnceLock};
use std::time::Duration;

use anyhow::{Context, Result, anyhow};
use libsignal_net::certs::{RootCertificates, SIGNAL_ROOT_CERTIFICATES};
use ureq::tls::{Certificate, RootCerts, TlsConfig};

/// What one answer may weigh: Signal's own limit for a file is 100 MB.
const LIMIT: u64 = 200 * 1024 * 1024;

fn agent() -> &'static ureq::Agent {
    static AGENT: OnceLock<ureq::Agent> = OnceLock::new();
    AGENT.get_or_init(|| {
        let _ = rustls::crypto::ring::default_provider().install_default();
        let roots: Vec<Certificate<'static>> = match SIGNAL_ROOT_CERTIFICATES {
            RootCertificates::FromStaticDers(ders) => ders.iter().map(|der| Certificate::from_der(der)).collect(),
            _ => Vec::new(),
        };
        ureq::Agent::config_builder()
            .timeout_connect(Some(Duration::from_secs(30)))
            .timeout_global(Some(Duration::from_secs(600)))
            .user_agent(concat!("Sola-Signal/", env!("CARGO_PKG_VERSION")))
            .http_status_as_error(false)
            .tls_config(TlsConfig::builder().root_certs(RootCerts::Specific(Arc::new(roots))).build())
            .build()
            .into()
    })
}

pub struct Answer {
    pub status: u16,
    pub location: Option<String>,
    pub body: Vec<u8>,
}

impl Answer {
    /// The body, if the status is a success.
    pub fn ok(self, what: &str) -> Result<Vec<u8>> {
        match self.status {
            200..=299 => Ok(self.body),
            status => Err(anyhow!("{what}: HTTP {status}")),
        }
    }
}

/// One request. `body`: `None` for a GET.
pub fn request(method: &str, url: &str, headers: &[(String, String)], body: Option<&[u8]>) -> Result<Answer> {
    let what = url.split('?').next().unwrap_or(url).to_owned();
    let mut response = match (method, body) {
        ("GET", _) => {
            let mut request = agent().get(url);
            for (name, value) in headers {
                request = request.header(name, value);
            }
            request.call()
        }
        (method, body) => {
            let mut request = match method {
                "PUT" => agent().put(url),
                "PATCH" => agent().patch(url),
                _ => agent().post(url),
            };
            for (name, value) in headers {
                request = request.header(name, value);
            }
            request.send(body.unwrap_or_default())
        }
    }
    .with_context(|| what.clone())?;
    let location = response.headers().get("location").and_then(|value| value.to_str().ok()).map(str::to_owned);
    let body = response.body_mut().with_config().limit(LIMIT).read_to_vec().with_context(|| what)?;
    Ok(Answer { status: response.status().as_u16(), location, body })
}

/// The same from async code.
pub async fn send(method: &'static str, url: String, headers: Vec<(String, String)>, body: Option<Vec<u8>>) -> Result<Answer> {
    tokio::task::spawn_blocking(move || request(method, &url, &headers, body.as_deref())).await?
}

/// `Authorization: Basic` for a name and a password.
pub fn basic(name: &str, password: &str) -> (String, String) {
    use base64::Engine;
    let token = base64::engine::general_purpose::STANDARD.encode(format!("{name}:{password}"));
    ("Authorization".into(), format!("Basic {token}"))
}
