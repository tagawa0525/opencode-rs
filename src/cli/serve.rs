//! Serve command - starts the HTTP server.

use anyhow::Result;

/// Execute the serve command
pub async fn execute(host: &str, port: u16) -> Result<()> {
    println!("Starting server on {}:{}", host, port);
    println!("Note: HTTP server not yet implemented in Rust version");

    // TODO: Implement HTTP server using axum or actix-web
    // The server should expose:
    // - Session management APIs
    // - Streaming chat endpoints
    // - Tool execution endpoints
    // - SSE for real-time updates

    // For now, just keep the process running
    tokio::signal::ctrl_c().await?;
    println!("\nShutting down...");

    Ok(())
}
