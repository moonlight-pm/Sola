use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

const TOKEN_ENDPOINT: &str = "https://platform.claude.com/v1/oauth/token";
const CLIENT_ID: &str = "9d1c250a-e61b-44d9-88ed-5944d1962f5e";
const REFRESH_BUFFER_MS: u64 = 5 * 60 * 1000;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct OAuthTokens {
    access_token: String,
    refresh_token: String,
    expires_at: u64,
    scopes: Vec<String>,
    #[serde(default)]
    subscription_type: Option<String>,
    #[serde(default)]
    rate_limit_tier: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CredentialsFile {
    claude_ai_oauth: OAuthTokens,
}

#[derive(Debug, Serialize, Deserialize)]
struct RefreshRequest {
    grant_type: String,
    refresh_token: String,
    client_id: String,
}

#[derive(Debug, Deserialize)]
struct RefreshResponse {
    access_token: String,
    refresh_token: Option<String>,
    expires_in: u64,
}

pub struct AuthManager {
    tokens: OAuthTokens,
    credentials_path: PathBuf,
    http: reqwest::Client,
}

impl AuthManager {
    pub fn load() -> Result<Self> {
        let home = std::env::var("HOME").context("HOME not set")?;
        let credentials_path = PathBuf::from(home).join(".claude/.credentials.json");
        let contents = std::fs::read_to_string(&credentials_path)
            .context("Failed to read ~/.claude/.credentials.json")?;
        let creds: CredentialsFile =
            serde_json::from_str(&contents).context("Failed to parse credentials")?;

        Ok(Self {
            tokens: creds.claude_ai_oauth,
            credentials_path,
            http: reqwest::Client::new(),
        })
    }

    pub fn access_token(&self) -> &str {
        &self.tokens.access_token
    }

    fn now_ms() -> u64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64
    }

    pub fn needs_refresh(&self) -> bool {
        Self::now_ms() + REFRESH_BUFFER_MS >= self.tokens.expires_at
    }

    pub async fn ensure_valid(&mut self) -> Result<()> {
        if self.needs_refresh() {
            self.refresh().await?;
        }
        Ok(())
    }

    pub async fn refresh(&mut self) -> Result<()> {
        tracing::info!("Refreshing OAuth token");

        let body = RefreshRequest {
            grant_type: "refresh_token".into(),
            refresh_token: self.tokens.refresh_token.clone(),
            client_id: CLIENT_ID.into(),
        };

        let resp = self
            .http
            .post(TOKEN_ENDPOINT)
            .json(&body)
            .timeout(std::time::Duration::from_secs(15))
            .send()
            .await
            .context("Token refresh request failed")?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            anyhow::bail!("Token refresh failed: {} {}", status, body);
        }

        let refresh_resp: RefreshResponse =
            resp.json().await.context("Failed to parse refresh response")?;

        self.tokens.access_token = refresh_resp.access_token;
        if let Some(new_refresh) = refresh_resp.refresh_token {
            self.tokens.refresh_token = new_refresh;
        }
        self.tokens.expires_at = Self::now_ms() + (refresh_resp.expires_in * 1000);

        self.save().context("Failed to save refreshed credentials")?;
        tracing::info!("OAuth token refreshed successfully");
        Ok(())
    }

    fn save(&self) -> Result<()> {
        let creds = CredentialsFile {
            claude_ai_oauth: self.tokens.clone(),
        };
        let json = serde_json::to_string_pretty(&creds)?;
        std::fs::write(&self.credentials_path, json)?;
        Ok(())
    }
}
