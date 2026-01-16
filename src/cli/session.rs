//! Session management CLI commands.

use crate::session::Session;
use anyhow::Result;
use chrono::{TimeZone, Utc};

/// List all sessions
pub async fn list() -> Result<()> {
    let sessions = Session::list("default").await?;

    if sessions.is_empty() {
        println!("No sessions found.");
        return Ok(());
    }

    println!("{:<30} {:<40} {:<20}", "ID", "Title", "Created");
    println!("{}", "-".repeat(90));

    for session in sessions {
        let created = Utc
            .timestamp_millis_opt(session.time.created)
            .single()
            .map(|t| t.format("%Y-%m-%d %H:%M").to_string())
            .unwrap_or_else(|| "Unknown".to_string());

        // Truncate title if too long
        let title = if session.title.len() > 38 {
            format!("{}...", &session.title[..35])
        } else {
            session.title.clone()
        };

        println!("{:<30} {:<40} {:<20}", session.id, title, created);
    }

    Ok(())
}

/// Show session details
pub async fn show(id: &str) -> Result<()> {
    let session = Session::get("default", id)
        .await?
        .ok_or_else(|| anyhow::anyhow!("Session not found: {}", id))?;

    println!("Session: {}", session.id);
    println!("Title: {}", session.title);
    println!("Slug: {}", session.slug);
    println!("Directory: {}", session.directory);

    let created = Utc
        .timestamp_millis_opt(session.time.created)
        .single()
        .map(|t| t.format("%Y-%m-%d %H:%M:%S").to_string())
        .unwrap_or_else(|| "Unknown".to_string());
    println!("Created: {}", created);

    let updated = Utc
        .timestamp_millis_opt(session.time.updated)
        .single()
        .map(|t| t.format("%Y-%m-%d %H:%M:%S").to_string())
        .unwrap_or_else(|| "Unknown".to_string());
    println!("Updated: {}", updated);

    if let Some(parent_id) = &session.parent_id {
        println!("Parent: {}", parent_id);
    }

    if let Some(share) = &session.share {
        println!("Share URL: {}", share.url);
    }

    // Show message count
    let messages = session.messages().await?;
    println!("\nMessages: {}", messages.len());

    // Show summary if available
    if let Some(summary) = &session.summary {
        println!("\nSummary:");
        println!("  Files changed: {}", summary.files);
        println!("  Additions: +{}", summary.additions);
        println!("  Deletions: -{}", summary.deletions);
    }

    Ok(())
}

/// Delete a session
pub async fn delete(id: &str) -> Result<()> {
    // Check if session exists
    let session = Session::get("default", id)
        .await?
        .ok_or_else(|| anyhow::anyhow!("Session not found: {}", id))?;

    println!("Deleting session: {} ({})", session.title, session.id);

    Session::delete("default", id).await?;

    println!("Session deleted.");

    Ok(())
}
