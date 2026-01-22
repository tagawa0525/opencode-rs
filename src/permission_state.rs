//! Global permission state management.
//!
//! This module provides a shared permission approval system that works across
//! both CLI and TUI modes. It handles:
//! - Storing approved permission rules
//! - Tracking pending permission requests
//! - Auto-approving requests based on approved rules
//! - Batch approval when "always" is selected

use std::collections::HashMap;
use std::sync::{Arc, LazyLock};

use serde::{Deserialize, Serialize};
use tokio::sync::oneshot;
use tokio::sync::Mutex;

use crate::tool::{self, PermissionScope};

// =============================================================================
// Types
// =============================================================================

/// Permission approval rule (stored in memory or persisted to disk)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PermissionRule {
    pub permission: String,
    pub pattern: String,
    pub scope: PermissionScope,
}

/// Permission request information for pending requests
#[derive(Debug, Clone)]
pub struct PermissionRequestInfo {
    pub id: String,
    pub permission: String,
    pub patterns: Vec<String>,
    pub always: Vec<String>,
}

// =============================================================================
// Global State
// =============================================================================

type ResponseChannelMap = HashMap<String, oneshot::Sender<tool::PermissionResponse>>;
type PendingRequestMap = HashMap<String, PermissionRequestInfo>;

static PERMISSION_RESPONSES: LazyLock<Arc<Mutex<ResponseChannelMap>>> =
    LazyLock::new(|| Arc::new(Mutex::new(HashMap::new())));

static SESSION_RULES: LazyLock<Arc<Mutex<Vec<PermissionRule>>>> =
    LazyLock::new(|| Arc::new(Mutex::new(Vec::new())));

static WORKSPACE_RULES: LazyLock<Arc<Mutex<Vec<PermissionRule>>>> =
    LazyLock::new(|| Arc::new(Mutex::new(Vec::new())));

static GLOBAL_RULES: LazyLock<Arc<Mutex<Vec<PermissionRule>>>> =
    LazyLock::new(|| Arc::new(Mutex::new(Vec::new())));

static PENDING_REQUESTS: LazyLock<Arc<Mutex<PendingRequestMap>>> =
    LazyLock::new(|| Arc::new(Mutex::new(HashMap::new())));

// =============================================================================
// Initialization
// =============================================================================

/// Initialize permission state by loading saved rules
pub async fn initialize() -> Result<(), Box<dyn std::error::Error>> {
    if let Err(e) = load_global_rules().await {
        eprintln!("Warning: Failed to load global permission rules: {}", e);
    }

    if let Err(e) = load_workspace_rules().await {
        eprintln!("Warning: Failed to load workspace permission rules: {}", e);
    }

    Ok(())
}

// =============================================================================
// Public API
// =============================================================================

/// Store a response channel for a permission request
pub async fn store_response_channel(id: String, tx: oneshot::Sender<tool::PermissionResponse>) {
    PERMISSION_RESPONSES.lock().await.insert(id, tx);
}

/// Store a pending permission request
pub async fn store_pending_request(request: PermissionRequestInfo) {
    PENDING_REQUESTS
        .lock()
        .await
        .insert(request.id.clone(), request);
}

/// Check if a request should be auto-approved based on approved rules
pub async fn check_auto_approve(request: &tool::PermissionRequest) -> bool {
    check_patterns_against_rules(&request.permission, &request.patterns).await
}

/// Send permission response to waiting tool
pub async fn send_permission_response(id: String, allow: bool, scope: PermissionScope) {
    if scope == PermissionScope::Once || !allow {
        send_response(&id, allow, PermissionScope::Once).await;
        return;
    }

    let request = {
        let pending = PENDING_REQUESTS.lock().await;
        match pending.get(&id) {
            Some(req) => req.clone(),
            None => return,
        }
    };

    let rules: Vec<PermissionRule> = request
        .always
        .iter()
        .map(|pattern| PermissionRule {
            permission: request.permission.clone(),
            pattern: pattern.clone(),
            scope,
        })
        .collect();

    store_rules(rules, scope).await;
    send_response(&id, true, scope).await;
    auto_approve_pending_requests().await;
}

/// Create a CLI permission handler that prompts the user in the terminal
pub fn create_cli_permission_handler() -> tool::PermissionHandler {
    use std::io::{self, Write};

    Arc::new(move |request| {
        let (response_tx, response_rx) = oneshot::channel();
        let request_clone = request.clone();

        tokio::spawn(async move {
            if check_auto_approve(&request_clone).await {
                let _ = response_tx.send(tool::PermissionResponse {
                    id: request_clone.id.clone(),
                    allow: true,
                    scope: PermissionScope::Session,
                });
                return;
            }

            store_response_channel(request_clone.id.clone(), response_tx).await;
            store_pending_request(PermissionRequestInfo {
                id: request_clone.id.clone(),
                permission: request_clone.permission.clone(),
                patterns: request_clone.patterns.clone(),
                always: request_clone.always.clone(),
            })
            .await;

            let request_for_blocking = request_clone.clone();
            let (user_tx, user_rx) = oneshot::channel();

            tokio::task::spawn_blocking(move || {
                print_permission_prompt(&request_for_blocking);
                let _ = io::stderr().flush();

                let mut input = String::new();
                let _ = io::stdin().read_line(&mut input);

                let (allow, scope) = parse_permission_choice(input.trim());
                let _ = user_tx.send((request_for_blocking.id, allow, scope));
            });

            tokio::spawn(async move {
                if let Ok((id, allow, scope)) = user_rx.await {
                    send_permission_response(id, allow, scope).await;
                }
            });
        });

        response_rx
    })
}

/// Create a TUI permission handler that sends requests via event channel
pub fn create_tui_permission_handler(
    event_tx: tokio::sync::mpsc::Sender<crate::tui::AppEvent>,
) -> tool::PermissionHandler {
    Arc::new(move |request| {
        let event_tx = event_tx.clone();
        let (response_tx, response_rx) = oneshot::channel();
        let request_clone = request.clone();

        tokio::spawn(async move {
            if check_auto_approve(&request_clone).await {
                let _ = response_tx.send(tool::PermissionResponse {
                    id: request_clone.id.clone(),
                    allow: true,
                    scope: PermissionScope::Session,
                });
                return;
            }

            store_response_channel(request_clone.id.clone(), response_tx).await;
            store_pending_request(PermissionRequestInfo {
                id: request_clone.id.clone(),
                permission: request_clone.permission.clone(),
                patterns: request_clone.patterns.clone(),
                always: request_clone.always.clone(),
            })
            .await;

            let _ = event_tx.try_send(crate::tui::AppEvent::PermissionRequested(
                crate::tui::PermissionRequest {
                    id: request_clone.id,
                    permission: request_clone.permission,
                    patterns: request_clone.patterns,
                    metadata: request_clone.metadata,
                },
            ));
        });

        response_rx
    })
}

// =============================================================================
// Internal Functions
// =============================================================================

async fn check_patterns_against_rules(permission: &str, patterns: &[String]) -> bool {
    let global = GLOBAL_RULES.lock().await;
    let workspace = WORKSPACE_RULES.lock().await;
    let session = SESSION_RULES.lock().await;

    let all_rules: Vec<&PermissionRule> = global
        .iter()
        .chain(workspace.iter())
        .chain(session.iter())
        .collect();

    patterns.iter().all(|pattern| {
        all_rules
            .iter()
            .any(|rule| rule.permission == permission && wildcard_match(&rule.pattern, pattern))
    })
}

async fn send_response(id: &str, allow: bool, scope: PermissionScope) {
    let mut responses = PERMISSION_RESPONSES.lock().await;
    if let Some(tx) = responses.remove(id) {
        let response = tool::PermissionResponse {
            id: id.to_string(),
            allow,
            scope,
        };
        if tx.send(response).is_err() {
            eprintln!(
                "Warning: Permission response receiver dropped for request {}",
                id
            );
        }
    }
    drop(responses);

    PENDING_REQUESTS.lock().await.remove(id);
}

async fn store_rules(rules: Vec<PermissionRule>, scope: PermissionScope) {
    match scope {
        PermissionScope::Session => {
            SESSION_RULES.lock().await.extend(rules);
        }
        PermissionScope::Workspace => {
            WORKSPACE_RULES.lock().await.extend(rules);
            if let Err(e) = save_workspace_rules().await {
                eprintln!("Warning: Failed to save workspace permission rules: {}", e);
            }
        }
        PermissionScope::Global => {
            GLOBAL_RULES.lock().await.extend(rules);
            if let Err(e) = save_global_rules().await {
                eprintln!("Warning: Failed to save global permission rules: {}", e);
            }
        }
        PermissionScope::Once => {}
    }
}

async fn auto_approve_pending_requests() {
    let pending_ids: Vec<String> = PENDING_REQUESTS.lock().await.keys().cloned().collect();

    for id in pending_ids {
        let should_approve = {
            let pending = PENDING_REQUESTS.lock().await;
            match pending.get(&id) {
                Some(req) => {
                    let permission = req.permission.clone();
                    let patterns = req.patterns.clone();
                    drop(pending);
                    check_patterns_against_rules(&permission, &patterns).await
                }
                None => continue,
            }
        };

        if !should_approve {
            continue;
        }

        let mut responses = PERMISSION_RESPONSES.lock().await;
        let mut pending = PENDING_REQUESTS.lock().await;

        if !pending.contains_key(&id) {
            continue;
        }

        if let Some(tx) = responses.remove(&id) {
            let response = tool::PermissionResponse {
                id: id.clone(),
                allow: true,
                scope: PermissionScope::Once,
            };
            if tx.send(response).is_err() {
                eprintln!(
                    "Warning: Permission response receiver dropped for auto-approved request {}",
                    id
                );
            }
        }
        pending.remove(&id);
    }
}

// =============================================================================
// Persistence
// =============================================================================

async fn save_workspace_rules() -> Result<(), Box<dyn std::error::Error>> {
    let rules = WORKSPACE_RULES.lock().await.clone();
    let cwd = std::env::current_dir()?;
    let dir = cwd.join(".opencode");

    tokio::fs::create_dir_all(&dir).await?;
    let json = serde_json::to_string_pretty(&rules)?;
    tokio::fs::write(dir.join("permissions.json"), json).await?;

    Ok(())
}

async fn load_workspace_rules() -> Result<(), Box<dyn std::error::Error>> {
    let path = std::env::current_dir()?.join(".opencode/permissions.json");

    if !path.exists() {
        return Ok(());
    }

    let json = tokio::fs::read_to_string(&path).await?;
    let rules: Vec<PermissionRule> = serde_json::from_str(&json)?;
    *WORKSPACE_RULES.lock().await = rules;

    Ok(())
}

async fn save_global_rules() -> Result<(), Box<dyn std::error::Error>> {
    let rules = GLOBAL_RULES.lock().await.clone();
    let storage = crate::storage::global();
    storage.write(&["permissions"], &rules).await?;
    Ok(())
}

async fn load_global_rules() -> Result<(), Box<dyn std::error::Error>> {
    let storage = crate::storage::global();
    if let Some(rules) = storage
        .read::<Vec<PermissionRule>>(&["permissions"])
        .await?
    {
        *GLOBAL_RULES.lock().await = rules;
    }
    Ok(())
}

// =============================================================================
// CLI Helpers
// =============================================================================

fn print_permission_prompt(request: &tool::PermissionRequest) {
    eprintln!("\n[Permission Required]");
    eprintln!("Tool: {}", request.permission);
    eprintln!("Patterns: {:?}", request.patterns);
    eprintln!(
        "Action: Execute with arguments: {}",
        serde_json::to_string(&request.metadata).unwrap_or_default()
    );
    eprintln!();
    eprintln!("Options:");
    eprintln!("  y/yes      - Allow once (this request only)");
    eprintln!("  s/session  - Allow for this session (until program restarts)");
    eprintln!("  w/workspace- Allow for this workspace (saved to .opencode/)");
    eprintln!("  g/global   - Allow globally for this user");
    eprintln!("  n/no       - Deny this request");
    eprint!("\nChoice [Y/s/w/g/n]: ");
}

fn parse_permission_choice(answer: &str) -> (bool, PermissionScope) {
    let answer = answer.to_lowercase();
    match answer.as_str() {
        "" | "y" | "yes" => (true, PermissionScope::Once),
        "s" | "session" => (true, PermissionScope::Session),
        "w" | "workspace" => (true, PermissionScope::Workspace),
        "g" | "global" => (true, PermissionScope::Global),
        _ => (false, PermissionScope::Once),
    }
}

// =============================================================================
// Wildcard Matching
// =============================================================================

/// Wildcard matching supporting multiple asterisks
fn wildcard_match(pattern: &str, text: &str) -> bool {
    if pattern == text || pattern == "*" {
        return true;
    }

    if !pattern.contains('*') {
        return false;
    }

    // Security: Reject overly broad patterns for absolute paths
    if pattern == "/*" && !text.starts_with("./") && !text.starts_with("../") {
        return false;
    }

    let parts: Vec<&str> = pattern.split('*').collect();

    // Fast path for single asterisk patterns (e.g., "prefix*suffix")
    if parts.len() == 2 {
        let (prefix, suffix) = (parts[0], parts[1]);
        return text.starts_with(prefix)
            && text.ends_with(suffix)
            && text.len() >= prefix.len() + suffix.len();
    }

    // General case: multiple asterisks using greedy matching
    let mut pos = 0;

    for (i, part) in parts.iter().enumerate() {
        if part.is_empty() {
            continue;
        }

        if i == 0 {
            if !text[pos..].starts_with(part) {
                return false;
            }
            pos += part.len();
        } else if i == parts.len() - 1 {
            if !text.ends_with(part) || pos > text.len() - part.len() {
                return false;
            }
        } else if let Some(found) = text[pos..].find(part) {
            pos += found + part.len();
        } else {
            return false;
        }
    }

    true
}
