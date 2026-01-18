use anyhow::Result;
use clap::{Parser, Subcommand};
use tracing_subscriber::EnvFilter;

mod auth;
mod bus;
mod cli;
mod config;
mod id;
mod oauth;
mod permission;
mod permission_state;
mod provider;
mod session;
mod slash_command;
mod storage;
mod tool;
mod tui;

#[derive(Parser)]
#[command(name = "opencode")]
#[command(about = "AI-powered development tool", long_about = None)]
#[command(version)]
struct Cli {
    /// Enable verbose logging
    #[arg(short, long, global = true)]
    verbose: bool,

    /// Working directory
    #[arg(short = 'C', long, global = true)]
    directory: Option<String>,

    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand)]
enum Commands {
    /// Start an interactive TUI session
    #[command(alias = "tui")]
    Run {
        /// Initial prompt to send
        #[arg(short, long)]
        prompt: Option<String>,

        /// Model to use (provider/model format)
        #[arg(short, long)]
        model: Option<String>,
    },

    /// Run a single prompt without TUI
    #[command(alias = "ask")]
    Prompt {
        /// The prompt to send
        prompt: String,

        /// Model to use (provider/model format)
        #[arg(short, long)]
        model: Option<String>,

        /// Output format (text, json, markdown)
        #[arg(short, long, default_value = "text")]
        format: String,
    },

    /// Start the HTTP server
    Serve {
        /// Port to listen on
        #[arg(short, long, default_value = "19876")]
        port: u16,

        /// Host to bind to
        #[arg(long, default_value = "127.0.0.1")]
        host: String,
    },

    /// Manage sessions
    Session {
        #[command(subcommand)]
        command: SessionCommands,
    },

    /// Manage configuration
    Config {
        #[command(subcommand)]
        command: ConfigCommands,
    },

    /// Show version information
    Version,
}

#[derive(Subcommand)]
enum SessionCommands {
    /// List all sessions
    List,
    /// Show session details
    Show {
        /// Session ID
        id: String,
    },
    /// Delete a session
    Delete {
        /// Session ID
        id: String,
    },
}

#[derive(Subcommand)]
enum ConfigCommands {
    /// Show current configuration
    Show,
    /// Show configuration file path
    Path,
    /// Initialize configuration file with defaults
    Init,
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    // Initialize logging
    let filter = if cli.verbose {
        EnvFilter::new("debug")
    } else {
        EnvFilter::new("info")
    };

    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_target(false)
        .init();

    // Change directory if specified
    if let Some(dir) = &cli.directory {
        std::env::set_current_dir(dir)?;
    }

    // Initialize permission state (load saved rules)
    if let Err(e) = permission_state::initialize().await {
        tracing::warn!("Failed to initialize permission state: {}", e);
    }

    match cli.command {
        Some(Commands::Run { prompt, model }) => {
            cli::run::execute(prompt, model).await?;
        }
        Some(Commands::Prompt {
            prompt,
            model,
            format,
        }) => {
            cli::prompt::execute(&prompt, model.as_deref(), &format).await?;
        }
        Some(Commands::Serve { port, host }) => {
            cli::serve::execute(&host, port).await?;
        }
        Some(Commands::Session { command }) => match command {
            SessionCommands::List => {
                cli::session::list().await?;
            }
            SessionCommands::Show { id } => {
                cli::session::show(&id).await?;
            }
            SessionCommands::Delete { id } => {
                cli::session::delete(&id).await?;
            }
        },
        Some(Commands::Config { command }) => match command {
            ConfigCommands::Show => {
                cli::config::show().await?;
            }
            ConfigCommands::Path => {
                cli::config::path().await?;
            }
            ConfigCommands::Init => {
                cli::config::init().await?;
            }
        },
        Some(Commands::Version) => {
            println!("opencode {}", env!("CARGO_PKG_VERSION"));
        }
        None => {
            // Default: start TUI
            cli::run::execute(None, None).await?;
        }
    }

    Ok(())
}
