//! Global question state management.
//!
//! This module provides a shared question/answer system that works across
//! both CLI and TUI modes. It handles:
//! - Storing pending question requests
//! - Managing response channels for waiting tools
//! - Creating handlers for different UIs

use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::Mutex;

use crate::tool::{QuestionInfo, QuestionResponse};

/// Question request information
#[derive(Debug, Clone)]
pub struct QuestionRequestInfo {
    pub id: String,
    pub questions: Vec<QuestionInfo>,
}

// Global question state
lazy_static::lazy_static! {
    /// Response channels for pending question requests
    static ref QUESTION_RESPONSES: Arc<Mutex<HashMap<String, tokio::sync::oneshot::Sender<QuestionResponse>>>> =
        Arc::new(Mutex::new(HashMap::new()));

    /// Pending question requests
    static ref PENDING_QUESTIONS: Arc<Mutex<HashMap<String, QuestionRequestInfo>>> =
        Arc::new(Mutex::new(HashMap::new()));
}

/// Store a response channel for a question request
pub async fn store_response_channel(
    id: String,
    tx: tokio::sync::oneshot::Sender<QuestionResponse>,
) {
    let mut map = QUESTION_RESPONSES.lock().await;
    map.insert(id, tx);
}

/// Store a pending question request
pub async fn store_pending_request(request: QuestionRequestInfo) {
    let mut map = PENDING_QUESTIONS.lock().await;
    map.insert(request.id.clone(), request);
}

/// Send question response to waiting tool
pub async fn send_question_response(id: String, answers: QuestionResponse) {
    let mut response_map = QUESTION_RESPONSES.lock().await;
    if let Some(tx) = response_map.remove(&id) {
        if tx.send(answers).is_err() {
            eprintln!(
                "Warning: Question response receiver dropped for request {}",
                id
            );
        }
    }
    drop(response_map);

    let mut pending = PENDING_QUESTIONS.lock().await;
    pending.remove(&id);
}

/// Create a TUI question handler that sends requests via event channel
pub fn create_tui_question_handler(
    event_tx: tokio::sync::mpsc::Sender<crate::tui::AppEvent>,
) -> crate::tool::QuestionHandler {
    std::sync::Arc::new(move |request| {
        let event_tx = event_tx.clone();
        let (response_tx, response_rx) = tokio::sync::oneshot::channel();

        let request_clone = request.clone();
        tokio::spawn(async move {
            // Store response channel for later use
            store_response_channel(request_clone.id.clone(), response_tx).await;

            // Store pending request
            store_pending_request(QuestionRequestInfo {
                id: request_clone.id.clone(),
                questions: request_clone.questions.clone(),
            })
            .await;

            // Send question request event to TUI
            // No conversion needed - types are unified (tui re-exports tool types)
            let _ = event_tx.try_send(crate::tui::AppEvent::QuestionRequested(request_clone));
        });

        response_rx
    })
}
