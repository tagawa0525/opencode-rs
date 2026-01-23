//! OAuth flow handlers for the TUI.
//!
//! This module contains functions for handling OAuth authentication flows
//! for various providers (GitHub Copilot, OpenAI, etc.).

use std::time::Duration;
use tokio::sync::mpsc;
use tokio::time::timeout;

use super::types::AppEvent;
use crate::oauth::{self, PkceCodes};

/// Start GitHub Copilot OAuth device flow
pub async fn start_copilot_oauth_flow(tx: mpsc::Sender<AppEvent>) {
    // Request device code
    match oauth::copilot_request_device_code().await {
        Ok(device_code_response) => {
            // Send device code to UI immediately
            let _ = tx
                .send(AppEvent::DeviceCodeReceived {
                    user_code: device_code_response.user_code.clone(),
                    verification_uri: device_code_response.verification_uri.clone(),
                })
                .await;

            // Start polling in a separate task (non-blocking)
            let device_code = device_code_response.device_code;
            let interval = device_code_response.interval;
            tokio::spawn(async move {
                poll_copilot_token(tx, device_code, interval).await;
            });
        }
        Err(e) => {
            let _ = tx.send(AppEvent::OAuthError(e.to_string())).await;
        }
    }
}

/// Poll for GitHub Copilot token in background
pub async fn poll_copilot_token(tx: mpsc::Sender<AppEvent>, device_code: String, interval: u64) {
    // Timeout after 15 minutes (device codes typically expire after 15 min)
    let poll_result = timeout(
        Duration::from_secs(900),
        oauth::copilot_poll_for_token(&device_code, interval),
    )
    .await;

    match poll_result {
        Ok(Ok(access_token)) => {
            // Save token
            let token_info = oauth::OAuthTokenInfo::new_copilot(access_token.clone());
            if let Err(e) = crate::auth::save_oauth_token("copilot", token_info).await {
                let _ = tx
                    .send(AppEvent::OAuthError(format!("Failed to save token: {}", e)))
                    .await;
                return;
            }

            // Also set environment variable for current session
            std::env::set_var("GITHUB_COPILOT_TOKEN", &access_token);

            let _ = tx
                .send(AppEvent::OAuthSuccess {
                    provider_id: "copilot".to_string(),
                })
                .await;
        }
        Ok(Err(e)) => {
            let _ = tx.send(AppEvent::OAuthError(e.to_string())).await;
        }
        Err(_) => {
            let _ = tx
                .send(AppEvent::OAuthError(
                    "Authentication timed out. Please try again.".to_string(),
                ))
                .await;
        }
    }
}

/// Start OpenAI OAuth PKCE flow
pub async fn start_openai_oauth_flow(tx: mpsc::Sender<AppEvent>) {
    // Generate PKCE codes and state
    let pkce = oauth::generate_pkce();
    let state = oauth::generate_state();
    let redirect_uri = oauth::get_oauth_redirect_uri();

    // Start callback server
    let callback_rx = match oauth::start_oauth_callback_server(state.clone()).await {
        Ok(rx) => rx,
        Err(e) => {
            let _ = tx
                .send(AppEvent::OAuthError(format!(
                    "Failed to start callback server: {}",
                    e
                )))
                .await;
            return;
        }
    };

    // Build and open auth URL
    let auth_url = oauth::build_openai_auth_url(&redirect_uri, &pkce, &state);
    if let Err(e) = open::that(&auth_url) {
        let _ = tx
            .send(AppEvent::OAuthError(format!(
                "Failed to open browser: {}",
                e
            )))
            .await;
        return;
    }

    // Handle callback in a separate task (non-blocking)
    tokio::spawn(async move {
        handle_openai_callback(tx, callback_rx, redirect_uri, pkce).await;
    });
}

/// Handle OpenAI OAuth callback in background
pub async fn handle_openai_callback(
    tx: mpsc::Sender<AppEvent>,
    callback_rx: tokio::sync::oneshot::Receiver<String>,
    redirect_uri: String,
    pkce: PkceCodes,
) {
    // Timeout after 5 minutes for user to complete browser auth
    let callback_result = timeout(Duration::from_secs(300), callback_rx).await;

    match callback_result {
        Ok(Ok(code)) => {
            // Exchange code for tokens
            match oauth::openai_exchange_code(&code, &redirect_uri, &pkce).await {
                Ok(tokens) => {
                    // Save tokens
                    let token_info = oauth::OAuthTokenInfo::new_openai(tokens);
                    if let Err(e) =
                        crate::auth::save_oauth_token("openai", token_info.clone()).await
                    {
                        let _ = tx
                            .send(AppEvent::OAuthError(format!(
                                "Failed to save tokens: {}",
                                e
                            )))
                            .await;
                        return;
                    }

                    // Set environment variable for current session
                    std::env::set_var("OPENAI_API_KEY", &token_info.access);

                    let _ = tx
                        .send(AppEvent::OAuthSuccess {
                            provider_id: "openai".to_string(),
                        })
                        .await;
                }
                Err(e) => {
                    let _ = tx.send(AppEvent::OAuthError(e.to_string())).await;
                }
            }
        }
        Ok(Err(_)) => {
            let _ = tx
                .send(AppEvent::OAuthError("OAuth callback failed".to_string()))
                .await;
        }
        Err(_) => {
            let _ = tx
                .send(AppEvent::OAuthError(
                    "Authentication timed out. Please try again.".to_string(),
                ))
                .await;
        }
    }
}
