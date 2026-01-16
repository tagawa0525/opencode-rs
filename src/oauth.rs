//! OAuth authentication module for subscription-based services.
//!
//! This module implements OAuth flows for:
//! - GitHub Copilot Pro (Device Code Flow)
//! - OpenAI ChatGPT Plus/Pro (PKCE Authorization Code Flow)

use anyhow::{Context, Result};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::time::Duration;
use tokio::time::sleep;

// ============================================================================
// GitHub Copilot Device Code Flow
// ============================================================================

const GITHUB_CLIENT_ID: &str = "Ov23li8tweQw6odWQebz";
const GITHUB_DEVICE_CODE_URL: &str = "https://github.com/login/device/code";
const GITHUB_ACCESS_TOKEN_URL: &str = "https://github.com/login/oauth/access_token";

/// Response from GitHub device code request
#[derive(Debug, Deserialize)]
pub struct DeviceCodeResponse {
    pub device_code: String,
    pub user_code: String,
    pub verification_uri: String,
    pub expires_in: u64,
    pub interval: u64,
}

/// Response from GitHub access token request
#[derive(Debug, Deserialize)]
struct AccessTokenResponse {
    access_token: Option<String>,
    token_type: Option<String>,
    error: Option<String>,
    error_description: Option<String>,
}

/// Request device code for GitHub Copilot
pub async fn copilot_request_device_code() -> Result<DeviceCodeResponse> {
    let client = Client::new();
    
    let response = client
        .post(GITHUB_DEVICE_CODE_URL)
        .header("Accept", "application/json")
        .header("Content-Type", "application/json")
        .header("User-Agent", "opencode-rs/0.1.0")
        .json(&serde_json::json!({
            "client_id": GITHUB_CLIENT_ID,
            "scope": "read:user"
        }))
        .send()
        .await
        .context("Failed to request device code")?;

    if !response.status().is_success() {
        let error = response.text().await.unwrap_or_else(|_| "Unknown error".to_string());
        anyhow::bail!("Device code request failed: {}", error);
    }

    let device_code: DeviceCodeResponse = response
        .json()
        .await
        .context("Failed to parse device code response")?;

    Ok(device_code)
}

/// Poll for access token after user authorizes
pub async fn copilot_poll_for_token(device_code: &str, interval: u64) -> Result<String> {
    let client = Client::new();
    let poll_interval = Duration::from_secs(interval.max(5));

    loop {
        sleep(poll_interval).await;

        let response = client
            .post(GITHUB_ACCESS_TOKEN_URL)
            .header("Accept", "application/json")
            .header("Content-Type", "application/json")
            .header("User-Agent", "opencode-rs/0.1.0")
            .json(&serde_json::json!({
                "client_id": GITHUB_CLIENT_ID,
                "device_code": device_code,
                "grant_type": "urn:ietf:params:oauth:grant-type:device_code"
            }))
            .send()
            .await
            .context("Failed to poll for access token")?;

        let token_response: AccessTokenResponse = response
            .json()
            .await
            .context("Failed to parse token response")?;

        if let Some(access_token) = token_response.access_token {
            return Ok(access_token);
        }

        if let Some(error) = &token_response.error {
            match error.as_str() {
                "authorization_pending" => {
                    // User hasn't authorized yet, continue polling
                    continue;
                }
                "slow_down" => {
                    // We're polling too fast, wait longer
                    sleep(Duration::from_secs(5)).await;
                    continue;
                }
                "expired_token" => {
                    anyhow::bail!("Device code expired. Please try again.");
                }
                "access_denied" => {
                    anyhow::bail!("Access denied by user.");
                }
                _ => {
                    let desc = token_response.error_description.unwrap_or_default();
                    anyhow::bail!("Authentication error: {} - {}", error, desc);
                }
            }
        }
    }
}

// ============================================================================
// OpenAI Codex OAuth PKCE Flow
// ============================================================================

const OPENAI_CLIENT_ID: &str = "app_EMoamEEZ73f0CkXaXp7hrann";
const OPENAI_ISSUER: &str = "https://auth.openai.com";
const OPENAI_OAUTH_PORT: u16 = 1455;

/// PKCE codes for OAuth
#[derive(Debug, Clone)]
pub struct PkceCodes {
    pub verifier: String,
    pub challenge: String,
}

/// OpenAI token response
#[derive(Debug, Deserialize)]
pub struct OpenAITokenResponse {
    pub access_token: String,
    pub refresh_token: Option<String>,
    pub expires_in: Option<u64>,
    pub id_token: Option<String>,
    pub token_type: String,
}

/// Generate PKCE codes
pub fn generate_pkce() -> PkceCodes {
    use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
    use rand::Rng;
    use sha2::{Digest, Sha256};

    // Generate random verifier (43-128 characters)
    let verifier: String = rand::thread_rng()
        .sample_iter(&rand::distributions::Alphanumeric)
        .take(43)
        .map(char::from)
        .collect();

    // Create challenge from verifier
    let mut hasher = Sha256::new();
    hasher.update(verifier.as_bytes());
    let hash = hasher.finalize();
    let challenge = URL_SAFE_NO_PAD.encode(hash);

    PkceCodes { verifier, challenge }
}

/// Generate random state string
pub fn generate_state() -> String {
    use rand::Rng;
    rand::thread_rng()
        .sample_iter(&rand::distributions::Alphanumeric)
        .take(32)
        .map(char::from)
        .collect()
}

/// Build OpenAI authorization URL
pub fn build_openai_auth_url(redirect_uri: &str, pkce: &PkceCodes, state: &str) -> String {
    let params = [
        ("response_type", "code"),
        ("client_id", OPENAI_CLIENT_ID),
        ("redirect_uri", redirect_uri),
        ("scope", "openid profile email offline_access"),
        ("code_challenge", &pkce.challenge),
        ("code_challenge_method", "S256"),
        ("id_token_add_organizations", "true"),
        ("codex_cli_simplified_flow", "true"),
        ("state", state),
        ("originator", "opencode"),
    ];

    let query = params
        .iter()
        .map(|(k, v)| format!("{}={}", k, urlencoding::encode(v)))
        .collect::<Vec<_>>()
        .join("&");

    format!("{}/oauth/authorize?{}", OPENAI_ISSUER, query)
}

/// Exchange authorization code for tokens
pub async fn openai_exchange_code(
    code: &str,
    redirect_uri: &str,
    pkce: &PkceCodes,
) -> Result<OpenAITokenResponse> {
    let client = Client::new();

    let response = client
        .post(format!("{}/oauth/token", OPENAI_ISSUER))
        .header("Content-Type", "application/x-www-form-urlencoded")
        .body(format!(
            "grant_type=authorization_code&code={}&redirect_uri={}&client_id={}&code_verifier={}",
            urlencoding::encode(code),
            urlencoding::encode(redirect_uri),
            OPENAI_CLIENT_ID,
            urlencoding::encode(&pkce.verifier)
        ))
        .send()
        .await
        .context("Failed to exchange code for tokens")?;

    if !response.status().is_success() {
        let error = response.text().await.unwrap_or_else(|_| "Unknown error".to_string());
        anyhow::bail!("Token exchange failed: {}", error);
    }

    let tokens: OpenAITokenResponse = response
        .json()
        .await
        .context("Failed to parse token response")?;

    Ok(tokens)
}

/// Refresh OpenAI access token
pub async fn openai_refresh_token(refresh_token: &str) -> Result<OpenAITokenResponse> {
    let client = Client::new();

    let response = client
        .post(format!("{}/oauth/token", OPENAI_ISSUER))
        .header("Content-Type", "application/x-www-form-urlencoded")
        .body(format!(
            "grant_type=refresh_token&refresh_token={}&client_id={}",
            urlencoding::encode(refresh_token),
            OPENAI_CLIENT_ID
        ))
        .send()
        .await
        .context("Failed to refresh token")?;

    if !response.status().is_success() {
        let error = response.text().await.unwrap_or_else(|_| "Unknown error".to_string());
        anyhow::bail!("Token refresh failed: {}", error);
    }

    let tokens: OpenAITokenResponse = response
        .json()
        .await
        .context("Failed to parse token response")?;

    Ok(tokens)
}

/// Start local server to receive OAuth callback
pub async fn start_oauth_callback_server(
    expected_state: String,
) -> Result<tokio::sync::oneshot::Receiver<String>> {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    let (tx, rx) = tokio::sync::oneshot::channel();
    let listener = TcpListener::bind(format!("127.0.0.1:{}", OPENAI_OAUTH_PORT))
        .await
        .context("Failed to bind OAuth callback server")?;

    tokio::spawn(async move {
        if let Ok((mut socket, _)) = listener.accept().await {
            let mut buffer = [0u8; 4096];
            if let Ok(n) = socket.read(&mut buffer).await {
                let request = String::from_utf8_lossy(&buffer[..n]);
                
                // Parse the GET request for code and state
                if let Some(line) = request.lines().next() {
                    if let Some(path) = line.split_whitespace().nth(1) {
                        if let Some(query) = path.strip_prefix("/?") {
                            let params: std::collections::HashMap<_, _> = query
                                .split('&')
                                .filter_map(|p| {
                                    let mut parts = p.splitn(2, '=');
                                    Some((parts.next()?, parts.next()?))
                                })
                                .collect();

                            if let (Some(&code), Some(&state)) = (params.get("code"), params.get("state")) {
                                if state == expected_state {
                                    let _ = tx.send(code.to_string());
                                    
                                    // Send success response
                                    let response = "HTTP/1.1 200 OK\r\nContent-Type: text/html\r\n\r\n\
                                        <html><body><h1>Authentication Successful!</h1>\
                                        <p>You can close this window and return to opencode.</p>\
                                        <script>window.close();</script></body></html>";
                                    let _ = socket.write_all(response.as_bytes()).await;
                                }
                            }
                        }
                    }
                }
            }
        }
    });

    Ok(rx)
}

/// Get the OAuth redirect URI
pub fn get_oauth_redirect_uri() -> String {
    format!("http://127.0.0.1:{}/", OPENAI_OAUTH_PORT)
}

// ============================================================================
// OAuth Token Storage
// ============================================================================

/// OAuth token info stored in auth.json
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OAuthTokenInfo {
    #[serde(rename = "type")]
    pub auth_type: String,
    pub refresh: String,
    pub access: String,
    pub expires: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub account_id: Option<String>,
}

impl OAuthTokenInfo {
    pub fn new_copilot(access_token: String) -> Self {
        Self {
            auth_type: "oauth".to_string(),
            refresh: access_token.clone(),
            access: access_token,
            expires: 0, // Copilot tokens don't expire in traditional sense
            account_id: None,
        }
    }

    pub fn new_openai(tokens: OpenAITokenResponse) -> Self {
        let expires = tokens
            .expires_in
            .map(|e| chrono::Utc::now().timestamp() + e as i64)
            .unwrap_or(0);

        Self {
            auth_type: "oauth".to_string(),
            refresh: tokens.refresh_token.unwrap_or_default(),
            access: tokens.access_token,
            expires,
            account_id: None,
        }
    }

    pub fn is_expired(&self) -> bool {
        if self.expires == 0 {
            return false; // No expiration
        }
        chrono::Utc::now().timestamp() > self.expires
    }
}
