//! Connections to Signal's chat service, made by libsignal-net: the same
//! routes, DNS and TLS the official clients use.

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::Duration;

use http::HeaderName;
use libsignal_net::chat::{
    self, ChatConnection, ChatHeaders, ConnectError, RECOMMENDED_CHAT_WS_CONFIG, Request, Response, SendError,
};
use libsignal_net::connect_state::{
    ConnectState, ConnectionResources, DefaultConnectorFactory, PreconnectingFactory, SUGGESTED_CONNECT_CONFIG,
};
use libsignal_net::env::{Env, PROD, StaticIpOrder, UserAgent};
use libsignal_net::infra::OverrideNagleAlgorithm;
use libsignal_net_infra::EnableDomainFronting;
use libsignal_net_infra::dns::DnsResolver;
use libsignal_net_infra::route::{DirectOrProxyMode, DirectOrProxyProvider};
use libsignal_net_infra::utils::no_network_change_events;

pub use libsignal_net::env::constants::{CHAT_PROVISIONING_PATH, CHAT_WEBSOCKET_PATH};

/// How long one request may take before it is given up.
pub const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);

/// What every connection of this process shares.
pub struct Net {
    env: Env<'static>,
    connect: Mutex<ConnectState<PreconnectingFactory>>,
    dns: DnsResolver,
    user_agent: UserAgent,
}

impl Net {
    pub fn new() -> Self {
        let env = PROD;
        Self {
            connect: ConnectState::new_with_transport_connector(
                SUGGESTED_CONNECT_CONFIG,
                PreconnectingFactory::new(DefaultConnectorFactory, Duration::ZERO),
            ),
            dns: DnsResolver::new_with_static_fallback(
                env.static_fallback(StaticIpOrder::HARDCODED),
                &no_network_change_events(),
            ),
            user_agent: UserAgent::with_libsignal_version(concat!("Sola-Signal/", env!("CARGO_PKG_VERSION"))),
            env,
        }
    }

    /// Open the websocket at `path`. `headers` sign it in (or not); `listener`
    /// hears what the server sends.
    pub async fn connect(
        &self,
        path: &'static str,
        headers: Option<ChatHeaders>,
        listener: chat::ws::EventListener,
        tag: &str,
    ) -> Result<ChatConnection, ConnectError> {
        let chat = &self.env.chat_domain_config.connect;
        let routes = DirectOrProxyProvider {
            inner: chat.route_provider(EnableDomainFronting::No, OverrideNagleAlgorithm::UseSystemDefault),
            mode: DirectOrProxyMode::DirectOnly,
        };
        let resources = ConnectionResources {
            connect_state: &self.connect,
            dns_resolver: &self.dns,
            network_change_event: &no_network_change_events(),
            confirmation_header_name: chat.confirmation_header_name.map(HeaderName::from_static),
        };
        let pending = ChatConnection::start_connect_with(
            resources,
            chat.service,
            routes,
            path,
            &self.user_agent,
            RECOMMENDED_CHAT_WS_CONFIG,
            headers,
            tag,
        )
        .await?;
        Ok(ChatConnection::finish_connect(
            tokio::runtime::Handle::current(),
            pending,
            HashMap::new(),
            false,
            listener,
        ))
    }
}

/// Why a request got no good answer.
#[derive(Debug)]
pub enum Refused {
    /// The server answered, and not with a success.
    Status { what: String, status: http::StatusCode, said: String },
    /// No answer: the connection, or the time allowed.
    Connection { what: String, why: String },
}

impl Refused {
    pub fn status(&self) -> Option<http::StatusCode> {
        match self {
            Refused::Status { status, .. } => Some(*status),
            Refused::Connection { .. } => None,
        }
    }
}

impl std::fmt::Display for Refused {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Refused::Status { what, status, said } => write!(f, "{what}: {status} {said}"),
            Refused::Connection { what, why } => write!(f, "{what}: {why}"),
        }
    }
}

impl std::error::Error for Refused {}

/// One request on a connection, its body JSON if it has one. An answer that
/// is not a success is an error that says what the server said.
pub async fn request(
    chat: &ChatConnection,
    method: http::Method,
    path: &str,
    extra: Option<(HeaderName, http::HeaderValue)>,
    body: Option<&serde_json::Value>,
) -> Result<Response, Refused> {
    // The path, without what it asks about.
    let what = format!("{method} {}", path.split('?').next().unwrap_or(path));
    let mut headers = http::HeaderMap::new();
    if body.is_some() {
        headers.insert(http::header::CONTENT_TYPE, http::HeaderValue::from_static("application/json"));
    }
    headers.extend(extra);
    let request = Request {
        method,
        path: path.parse().map_err(|e| Refused::Connection { what: what.clone(), why: format!("{e}") })?,
        headers,
        body: body.map(|body| body.to_string().into_bytes().into()),
    };
    let response: Response = chat
        .send(request, REQUEST_TIMEOUT)
        .await
        .map_err(|e: SendError| Refused::Connection { what: what.clone(), why: e.to_string() })?;
    if !response.status.is_success() {
        let said = response.body.as_deref().map(String::from_utf8_lossy).unwrap_or_default().into_owned();
        return Err(Refused::Status { what, status: response.status, said });
    }
    Ok(response)
}

/// The headers that sign a connection in as this device.
pub fn sign_in(account: &crate::account::Account) -> ChatHeaders {
    chat::AuthenticatedChatHeaders {
        aci: account.aci,
        device_id: account.device_id,
        password: account.password.clone(),
        receive_stories: false.into(),
        languages: chat::LanguageList::parse(&["en"]).expect("a language tag"),
    }
    .into()
}
