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
use serde::{Serialize, Deserialize};

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

/// Send permission response to waiting tool
pub async fn send_permission_response(id: String, allow: bool, scope: PermissionScope) {
    if scope == PermissionScope::Once || !allow {
        // Simple case: just respond to this one permission request
        let mut map = PERMISSION_RESPONSES.lock().await;
        if let Some(tx) = map.remove(&id) {
            if let Err(_) = tx.send(tool::PermissionResponse {
                id: id.clone(),
                allow,
                scope: PermissionScope::Once,
            }) {
                eprintln!("Warning: Permission response receiver dropped for request {}", id);
            }
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
            drop(workspace_rules);
            
            // Save to .opencode/permissions.json in project root
            if let Err(e) = save_workspace_rules().await {
                eprintln!("Warning: Failed to save workspace permission rules: {}", e);
            }
        }
        PermissionScope::Global => {
            let mut global_rules = GLOBAL_RULES.lock().await;
            global_rules.extend(rules_to_add.clone());
            drop(global_rules);
            
            // Save to ~/.opencode/permissions.json
            if let Err(e) = save_global_rules().await {
                eprintln!("Warning: Failed to save global permission rules: {}", e);
            }
        }
        PermissionScope::Once => {
            // Already handled above
        }
    }

    // 3. Respond to the original request
    let mut response_map = PERMISSION_RESPONSES.lock().await;
    if let Some(tx) = response_map.remove(&id) {
        if let Err(_) = tx.send(tool::PermissionResponse {
            id: id.clone(),
            allow: true,
            scope,
        }) {
            eprintln!(
                "Warning: Permission response receiver dropped for request {}",
                id
            );
        }
    }
    drop(response_map);

    // 4. Remove this request from pending
    let mut pending_map = PENDING_REQUESTS.lock().await;
    pending_map.remove(&id);

    // 5. Auto-approve all other pending requests that now match approved rules
    // Lock both maps together to prevent race conditions
    let pending_ids: Vec<String> = pending_map.keys().cloned().collect();
    drop(pending_map);

    for pending_id in pending_ids {
        // Check if should auto-approve while holding the lock
        let should_approve = should_auto_approve(&pending_id).await;

        if should_approve {
            // Atomically remove from both maps to prevent duplicate sends
            let mut response_map = PERMISSION_RESPONSES.lock().await;
            let mut pending_map = PENDING_REQUESTS.lock().await;

            // Double-check the request still exists (might have been processed by another thread)
            if pending_map.contains_key(&pending_id) {
                if let Some(tx) = response_map.remove(&pending_id) {
                    // Use Once scope for auto-approved requests since they weren't explicitly approved at this scope
                    if let Err(_) = tx.send(tool::PermissionResponse {
                        id: pending_id.clone(),
                        allow: true,
                        scope: tool::PermissionScope::Once,
                    }) {
                        eprintln!("Warning: Permission response receiver dropped for auto-approved request {}", pending_id);
                    }
                }
                pending_map.remove(&pending_id);
            }

            drop(response_map);
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
        return text.starts_with(prefix) && text.ends_with(suffix) 
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
