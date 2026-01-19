//! Global permission state management.
//!
//! This module provides a shared permission approval system that works across
//! both CLI and TUI modes. It handles:
//! - Storing approved permission rules
//! - Tracking pending permission requests
//! - Auto-approving requests based on approved rules
//! - Batch approval when "always" is selected

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::Mutex;

use crate::tool::{self, PermissionScope};

/// Permission approval rule
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PermissionRule {
    pub permission: String,
    pub pattern: String,
    pub scope: PermissionScope,
}

/// Permission request information
#[derive(Debug, Clone)]
pub struct PermissionRequestInfo {
    pub id: String,
    pub permission: String,
    pub patterns: Vec<String>,
    pub always: Vec<String>,
    pub metadata: HashMap<String, serde_json::Value>,
}

// Global permission state
lazy_static::lazy_static! {
    /// Response channels for pending permission requests
    static ref PERMISSION_RESPONSES: Arc<Mutex<HashMap<String, tokio::sync::oneshot::Sender<tool::PermissionResponse>>>> =
        Arc::new(Mutex::new(HashMap::new()));

    /// Session-scoped approved permission rules (memory only)
    static ref SESSION_RULES: Arc<Mutex<Vec<PermissionRule>>> =
        Arc::new(Mutex::new(Vec::new()));

    /// Workspace-scoped approved permission rules (saved to project .opencode/permissions.json)
    static ref WORKSPACE_RULES: Arc<Mutex<Vec<PermissionRule>>> =
        Arc::new(Mutex::new(Vec::new()));

    /// Global-scoped approved permission rules (saved to ~/.opencode/permissions.json)
    static ref GLOBAL_RULES: Arc<Mutex<Vec<PermissionRule>>> =
        Arc::new(Mutex::new(Vec::new()));

    /// Pending permission requests (for auto-approval)
    static ref PENDING_REQUESTS: Arc<Mutex<HashMap<String, PermissionRequestInfo>>> =
        Arc::new(Mutex::new(HashMap::new()));
}

/// Store a response channel for a permission request
pub async fn store_response_channel(
    id: String,
    tx: tokio::sync::oneshot::Sender<tool::PermissionResponse>,
) {
    let mut map = PERMISSION_RESPONSES.lock().await;
    map.insert(id, tx);
}

/// Store a pending permission request
pub async fn store_pending_request(request: PermissionRequestInfo) {
    let mut map = PENDING_REQUESTS.lock().await;
    map.insert(request.id.clone(), request);
}

/// Check if all patterns match approved rules for a given permission
async fn check_patterns_against_rules(permission: &str, patterns: &[String]) -> bool {
    let global_rules = GLOBAL_RULES.lock().await;
    let workspace_rules = WORKSPACE_RULES.lock().await;
    let session_rules = SESSION_RULES.lock().await;

    let all_rules: Vec<&PermissionRule> = global_rules
        .iter()
        .chain(workspace_rules.iter())
        .chain(session_rules.iter())
        .collect();

    patterns.iter().all(|pattern| {
        all_rules
            .iter()
            .any(|rule| rule.permission == permission && wildcard_match(&rule.pattern, pattern))
    })
}

/// Check if a request should be auto-approved based on approved rules
pub async fn check_auto_approve(request: &tool::PermissionRequest) -> bool {
    check_patterns_against_rules(&request.permission, &request.patterns).await
}

/// Save workspace rules to project .opencode/permissions.json
async fn save_workspace_rules() -> Result<(), Box<dyn std::error::Error>> {
    let rules = WORKSPACE_RULES.lock().await;
    let rules_vec: Vec<PermissionRule> = rules.clone();
    drop(rules);

    // Get current working directory for workspace
    let cwd = std::env::current_dir()?;
    let permissions_dir = cwd.join(".opencode");
    let permissions_file = permissions_dir.join("permissions.json");

    // Create directory if it doesn't exist
    tokio::fs::create_dir_all(&permissions_dir).await?;

    // Write rules to file
    let json = serde_json::to_string_pretty(&rules_vec)?;
    tokio::fs::write(&permissions_file, json).await?;

    Ok(())
}

/// Load workspace rules from project .opencode/permissions.json
async fn load_workspace_rules() -> Result<(), Box<dyn std::error::Error>> {
    let cwd = std::env::current_dir()?;
    let permissions_file = cwd.join(".opencode").join("permissions.json");

    // Check if file exists
    if !permissions_file.exists() {
        return Ok(()); // No rules to load
    }

    // Read and parse rules
    let json = tokio::fs::read_to_string(&permissions_file).await?;
    let rules: Vec<PermissionRule> = serde_json::from_str(&json)?;

    // Store in global state
    let mut workspace_rules = WORKSPACE_RULES.lock().await;
    *workspace_rules = rules;

    Ok(())
}

/// Save global rules to ~/.opencode/permissions.json
async fn save_global_rules() -> Result<(), Box<dyn std::error::Error>> {
    let rules = GLOBAL_RULES.lock().await;
    let rules_vec: Vec<PermissionRule> = rules.clone();
    drop(rules);

    // Use storage module for global permissions
    let storage = crate::storage::global();
    storage.write(&["permissions"], &rules_vec).await?;

    Ok(())
}

/// Load global rules from ~/.opencode/permissions.json
async fn load_global_rules() -> Result<(), Box<dyn std::error::Error>> {
    let storage = crate::storage::global();

    // Read rules from storage
    match storage.read::<Vec<PermissionRule>>(&["permissions"]).await {
        Ok(Some(rules)) => {
            let mut global_rules = GLOBAL_RULES.lock().await;
            *global_rules = rules;
            Ok(())
        }
        Ok(None) => Ok(()), // No rules to load
        Err(e) => Err(e.into()),
    }
}

/// Initialize permission state by loading saved rules
pub async fn initialize() -> Result<(), Box<dyn std::error::Error>> {
    // Load global rules
    if let Err(e) = load_global_rules().await {
        eprintln!("Warning: Failed to load global permission rules: {}", e);
    }

    // Load workspace rules
    if let Err(e) = load_workspace_rules().await {
        eprintln!("Warning: Failed to load workspace permission rules: {}", e);
    }

    Ok(())
}

/// Send a permission response and remove from pending requests
async fn send_response(id: &str, allow: bool, scope: PermissionScope) {
    let mut response_map = PERMISSION_RESPONSES.lock().await;
    if let Some(tx) = response_map.remove(id) {
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
    drop(response_map);

    let mut pending = PENDING_REQUESTS.lock().await;
    pending.remove(id);
}

/// Store rules based on scope
async fn store_rules(rules: Vec<PermissionRule>, scope: PermissionScope) {
    match scope {
        PermissionScope::Session => {
            let mut session_rules = SESSION_RULES.lock().await;
            session_rules.extend(rules);
        }
        PermissionScope::Workspace => {
            let mut workspace_rules = WORKSPACE_RULES.lock().await;
            workspace_rules.extend(rules);
            drop(workspace_rules);
            if let Err(e) = save_workspace_rules().await {
                eprintln!("Warning: Failed to save workspace permission rules: {}", e);
            }
        }
        PermissionScope::Global => {
            let mut global_rules = GLOBAL_RULES.lock().await;
            global_rules.extend(rules);
            drop(global_rules);
            if let Err(e) = save_global_rules().await {
                eprintln!("Warning: Failed to save global permission rules: {}", e);
            }
        }
        PermissionScope::Once => {}
    }
}

/// Send permission response to waiting tool
pub async fn send_permission_response(id: String, allow: bool, scope: PermissionScope) {
    // Simple case: deny or one-time approval
    if scope == PermissionScope::Once || !allow {
        send_response(&id, allow, PermissionScope::Once).await;
        return;
    }

    // Scoped approval: get request details and store rules
    let request = {
        let pending_map = PENDING_REQUESTS.lock().await;
        match pending_map.get(&id) {
            Some(req) => req.clone(),
            None => return,
        }
    };

    // Create and store rules for each pattern in the "always" list
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

    // Respond to the original request
    send_response(&id, true, scope).await;

    // Auto-approve other pending requests that now match
    auto_approve_pending_requests().await;
}

/// Auto-approve pending requests that match stored rules
async fn auto_approve_pending_requests() {
    let pending_ids: Vec<String> = {
        let pending_map = PENDING_REQUESTS.lock().await;
        pending_map.keys().cloned().collect()
    };

    for pending_id in pending_ids {
        if !should_auto_approve(&pending_id).await {
            continue;
        }

        // Atomically check and remove to prevent race conditions
        let mut response_map = PERMISSION_RESPONSES.lock().await;
        let mut pending_map = PENDING_REQUESTS.lock().await;

        if !pending_map.contains_key(&pending_id) {
            continue;
        }

        if let Some(tx) = response_map.remove(&pending_id) {
            let response = tool::PermissionResponse {
                id: pending_id.clone(),
                allow: true,
                scope: tool::PermissionScope::Once,
            };
            if tx.send(response).is_err() {
                eprintln!(
                    "Warning: Permission response receiver dropped for auto-approved request {}",
                    pending_id
                );
            }
        }
        pending_map.remove(&pending_id);
    }
}

/// Check if a pending request should be auto-approved based on approved rules
async fn should_auto_approve(request_id: &str) -> bool {
    let pending_map = PENDING_REQUESTS.lock().await;
    let request = match pending_map.get(request_id) {
        Some(req) => req.clone(),
        None => return false,
    };
    drop(pending_map);

    check_patterns_against_rules(&request.permission, &request.patterns).await
}

/// Create a CLI permission handler that prompts the user in the terminal
pub fn create_cli_permission_handler() -> crate::tool::PermissionHandler {
    use std::io::{self, Write};

    std::sync::Arc::new(move |request| {
        let (response_tx, response_rx) = tokio::sync::oneshot::channel();

        let request_clone = request.clone();
        tokio::spawn(async move {
            // Check if this request matches any approved rules
            if check_auto_approve(&request_clone).await {
                let _ = response_tx.send(tool::PermissionResponse {
                    id: request_clone.id.clone(),
                    allow: true,
                    scope: tool::PermissionScope::Session,
                });
                return;
            }

            // Store response channel for later use
            store_response_channel(request_clone.id.clone(), response_tx).await;

            // Store pending request for potential auto-approval
            store_pending_request(PermissionRequestInfo {
                id: request_clone.id.clone(),
                permission: request_clone.permission.clone(),
                patterns: request_clone.patterns.clone(),
                always: request_clone.always.clone(),
                metadata: request_clone.metadata.clone(),
            })
            .await;

            // Ask user in blocking thread
            let request_for_blocking = request_clone.clone();
            let (user_response_tx, user_response_rx) = tokio::sync::oneshot::channel();

            tokio::task::spawn_blocking(move || {
                eprintln!("\n[Permission Required]");
                eprintln!("Tool: {}", request_for_blocking.permission);
                eprintln!("Patterns: {:?}", request_for_blocking.patterns);
                eprintln!(
                    "Action: Execute with arguments: {}",
                    serde_json::to_string(&request_for_blocking.metadata).unwrap_or_default()
                );
                eprintln!();
                eprintln!("Options:");
                eprintln!("  y/yes      - Allow once (this request only)");
                eprintln!("  s/session  - Allow for this session (until program restarts)");
                eprintln!("  w/workspace- Allow for this workspace (saved to .opencode/)");
                eprintln!("  g/global   - Allow globally for this user");
                eprintln!("  n/no       - Deny this request");
                eprint!("\nChoice [Y/s/w/g/n]: ");
                let _ = io::stderr().flush();

                let mut input = String::new();
                let _ = io::stdin().read_line(&mut input);

                let answer = input.trim().to_lowercase();
                let (allow, scope) = parse_permission_choice(&answer);

                let _ = user_response_tx.send((request_for_blocking.id, allow, scope));
            });

            // Wait for user response and send permission response
            tokio::spawn(async move {
                if let Ok((id, allow, scope)) = user_response_rx.await {
                    send_permission_response(id, allow, scope).await;
                }
            });
        });

        response_rx
    })
}

/// Parse user's permission choice into (allow, scope) tuple
fn parse_permission_choice(answer: &str) -> (bool, PermissionScope) {
    if answer.is_empty() || answer == "y" || answer == "yes" {
        (true, PermissionScope::Once)
    } else if answer == "s" || answer == "session" {
        (true, PermissionScope::Session)
    } else if answer == "w" || answer == "workspace" {
        (true, PermissionScope::Workspace)
    } else if answer == "g" || answer == "global" {
        (true, PermissionScope::Global)
    } else {
        (false, PermissionScope::Once)
    }
}

/// Create a TUI permission handler that sends requests via event channel
pub fn create_tui_permission_handler(
    event_tx: tokio::sync::mpsc::Sender<crate::tui::AppEvent>,
) -> crate::tool::PermissionHandler {
    std::sync::Arc::new(move |request| {
        let event_tx = event_tx.clone();
        let (response_tx, response_rx) = tokio::sync::oneshot::channel();

        let request_clone = request.clone();
        tokio::spawn(async move {
            // Check if this request matches any approved rules
            if check_auto_approve(&request_clone).await {
                let _ = response_tx.send(tool::PermissionResponse {
                    id: request_clone.id.clone(),
                    allow: true,
                    scope: tool::PermissionScope::Session,
                });
                return;
            }

            // Store response channel for later use
            store_response_channel(request_clone.id.clone(), response_tx).await;

            // Store pending request for potential auto-approval
            store_pending_request(PermissionRequestInfo {
                id: request_clone.id.clone(),
                permission: request_clone.permission.clone(),
                patterns: request_clone.patterns.clone(),
                always: request_clone.always.clone(),
                metadata: request_clone.metadata.clone(),
            })
            .await;

            // Send permission request event to TUI
            let _ = event_tx.try_send(crate::tui::AppEvent::PermissionRequested(
                crate::tui::PermissionRequest {
                    id: request_clone.id,
                    permission: request_clone.permission,
                    patterns: request_clone.patterns,
                    always: request_clone.always,
                    metadata: request_clone.metadata,
                },
            ));
        });

        response_rx
    })
}

/// Wildcard matching supporting multiple asterisks
/// Supports patterns like:
/// - "*" matches everything
/// - "https://crates.io/*" matches all URLs on crates.io
/// - "https://*.example.com/*" matches all subdomains
/// - "**/*.rs" matches all .rs files
/// - exact matches
fn wildcard_match(pattern: &str, text: &str) -> bool {
    // Exact match
    if pattern == text {
        return true;
    }

    // Single asterisk matches everything
    if pattern == "*" {
        return true;
    }

    // No wildcards - must be exact match (already checked above)
    if !pattern.contains('*') {
        return false;
    }

    // Security: Reject overly broad patterns that could match system paths
    if pattern == "/*" && !text.starts_with("./") && !text.starts_with("../") {
        // Only allow /* for relative paths, not absolute system paths
        return false;
    }

    // Split pattern by asterisks
    let parts: Vec<&str> = pattern.split('*').collect();

    // Fast path for single asterisk patterns
    if parts.len() == 2 {
        let (prefix, suffix) = (parts[0], parts[1]);
        return text.starts_with(prefix)
            && text.ends_with(suffix)
            && text.len() >= prefix.len() + suffix.len();
    }

    // General case: multiple asterisks
    // Use greedy matching algorithm
    let mut text_pos = 0;

    for (i, part) in parts.iter().enumerate() {
        if part.is_empty() {
            // Empty part means consecutive asterisks or leading/trailing asterisk
            continue;
        }

        if i == 0 {
            // First part must match at the beginning
            if !text[text_pos..].starts_with(part) {
                return false;
            }
            text_pos += part.len();
        } else if i == parts.len() - 1 {
            // Last part must match at the end
            if !text.ends_with(part) {
                return false;
            }
            // Verify we haven't gone past where the suffix should start
            if text_pos > text.len() - part.len() {
                return false;
            }
        } else {
            // Middle parts: find next occurrence
            if let Some(pos) = text[text_pos..].find(part) {
                text_pos += pos + part.len();
            } else {
                return false;
            }
        }
    }

    true
}
