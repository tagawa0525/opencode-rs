//! Global permission state management.
//!
//! This module provides a shared permission approval system that works across
//! both CLI and TUI modes. It handles:
//! - Storing approved permission rules
//! - Tracking pending permission requests
//! - Auto-approving requests based on approved rules
//! - Batch approval when "always" is selected

use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::Mutex;

use crate::tool::{self, PermissionScope};

/// Permission approval rule
#[derive(Debug, Clone)]
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

/// Check if a request should be auto-approved based on approved rules
pub async fn check_auto_approve(request: &tool::PermissionRequest) -> bool {
    // Check all rule scopes (global, workspace, session)
    let global_rules = GLOBAL_RULES.lock().await;
    let workspace_rules = WORKSPACE_RULES.lock().await;
    let session_rules = SESSION_RULES.lock().await;

    let all_rules: Vec<&PermissionRule> = global_rules
        .iter()
        .chain(workspace_rules.iter())
        .chain(session_rules.iter())
        .collect();

    // Check if all patterns in the request match approved rules
    for pattern in &request.patterns {
        let matches = all_rules.iter().any(|rule| {
            rule.permission == request.permission && wildcard_match(&rule.pattern, pattern)
        });
        if !matches {
            return false;
        }
    }

    true
}

/// Send permission response to waiting tool
pub async fn send_permission_response(id: String, allow: bool, scope: PermissionScope) {
    if scope == PermissionScope::Once || !allow {
        // Simple case: just respond to this one permission request
        let mut map = PERMISSION_RESPONSES.lock().await;
        if let Some(tx) = map.remove(&id) {
            let _ = tx.send(tool::PermissionResponse {
                id: id.clone(),
                allow,
                scope: PermissionScope::Once,
            });
        }
        // Remove from pending
        let mut pending = PENDING_REQUESTS.lock().await;
        pending.remove(&id);
        return;
    }

    // Session/Workspace/Global case: approve this request and store the rule

    // 1. Get the permission request details
    let pending_map = PENDING_REQUESTS.lock().await;
    let request = match pending_map.get(&id) {
        Some(req) => req.clone(),
        None => return,
    };
    drop(pending_map);

    // 2. Add approval rules for each pattern in the "always" list
    let rules_to_add: Vec<PermissionRule> = request
        .always
        .iter()
        .map(|pattern| PermissionRule {
            permission: request.permission.clone(),
            pattern: pattern.clone(),
            scope,
        })
        .collect();

    match scope {
        PermissionScope::Session => {
            let mut session_rules = SESSION_RULES.lock().await;
            session_rules.extend(rules_to_add.clone());
        }
        PermissionScope::Workspace => {
            let mut workspace_rules = WORKSPACE_RULES.lock().await;
            workspace_rules.extend(rules_to_add.clone());
            // TODO: Save to .opencode/permissions.json in project root
        }
        PermissionScope::Global => {
            let mut global_rules = GLOBAL_RULES.lock().await;
            global_rules.extend(rules_to_add.clone());
            // TODO: Save to ~/.opencode/permissions.json
        }
        PermissionScope::Once => {
            // Already handled above
        }
    }

    // 3. Respond to the original request
    let mut response_map = PERMISSION_RESPONSES.lock().await;
    if let Some(tx) = response_map.remove(&id) {
        let _ = tx.send(tool::PermissionResponse {
            id: id.clone(),
            allow: true,
            scope,
        });
    }
    drop(response_map);

    // 4. Remove this request from pending
    let mut pending_map = PENDING_REQUESTS.lock().await;
    pending_map.remove(&id);

    // 5. Auto-approve all other pending requests that now match approved rules
    let pending_ids: Vec<String> = pending_map.keys().cloned().collect();
    drop(pending_map);

    for pending_id in pending_ids {
        if should_auto_approve(&pending_id).await {
            // Auto-approve this pending request
            let mut response_map = PERMISSION_RESPONSES.lock().await;
            if let Some(tx) = response_map.remove(&pending_id) {
                let _ = tx.send(tool::PermissionResponse {
                    id: pending_id.clone(),
                    allow: true,
                    scope,
                });
            }
            drop(response_map);

            let mut pending_map = PENDING_REQUESTS.lock().await;
            pending_map.remove(&pending_id);
            drop(pending_map);
        }
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

    // Check all rule scopes
    let global_rules = GLOBAL_RULES.lock().await;
    let workspace_rules = WORKSPACE_RULES.lock().await;
    let session_rules = SESSION_RULES.lock().await;

    let all_rules: Vec<&PermissionRule> = global_rules
        .iter()
        .chain(workspace_rules.iter())
        .chain(session_rules.iter())
        .collect();

    // Check if all patterns in the request match approved rules
    for pattern in &request.patterns {
        let matches = all_rules.iter().any(|rule| {
            rule.permission == request.permission && wildcard_match(&rule.pattern, pattern)
        });
        if !matches {
            return false;
        }
    }

    true
}

/// Simple wildcard matching (* matches anything)
/// Supports patterns like:
/// - "*" matches everything
/// - "https://crates.io/*" matches all URLs on crates.io
/// - exact matches
fn wildcard_match(pattern: &str, text: &str) -> bool {
    if pattern == "*" {
        return true;
    }
    if pattern == text {
        return true;
    }

    // Handle patterns with * at the end (e.g., "https://crates.io/*")
    if pattern.ends_with("/*") {
        let prefix = &pattern[..pattern.len() - 1]; // Remove the * but keep the /
        if text.starts_with(prefix) {
            return true;
        }
    }

    // Handle patterns with * in the middle or beginning
    if pattern.contains('*') {
        let parts: Vec<&str> = pattern.split('*').collect();
        if parts.len() == 2 {
            let (prefix, suffix) = (parts[0], parts[1]);
            if text.starts_with(prefix) && text.ends_with(suffix) {
                return true;
            }
        }
    }

    false
}
