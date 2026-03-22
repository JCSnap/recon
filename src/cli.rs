use clap::{Parser, Subcommand};

/// Monitor and manage AI coding agent sessions running in tmux
#[derive(Parser)]
#[command(name = "recon", version)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Option<Command>,
}

#[derive(Subcommand)]
pub enum Command {
    /// Open the visual (tamagotchi) dashboard
    View,
    /// Interactive form to create a new session (pick tag + agent, launches in current directory)
    New,
    /// Create a new agent session in the current directory and attach to it
    Launch {
        /// Short label shown in the # column of the dashboard (e.g. your tab number)
        #[arg(value_name = "TAG")]
        tag: Option<String>,
        /// Agent to launch: claude, claude-2, codex, gemini (default: claude)
        #[arg(long, value_name = "AGENT")]
        agent: Option<String>,
        /// Print only the tmux session name without attaching
        #[arg(long)]
        name_only: bool,
    },
    /// Jump directly to the next agent waiting for input
    Next,
    /// Resume a past Claude session (interactive picker, or by ID)
    Resume {
        /// Session ID to resume directly (skips the picker)
        #[arg(long)]
        id: Option<String>,
        /// Custom tmux session name
        #[arg(long)]
        name: Option<String>,
        /// Don't attach to the session after resuming
        #[arg(long)]
        no_attach: bool,
    },
    /// Print all session state as JSON
    Json,
    /// Save all live sessions to disk for restoring later
    Park,
    /// Restore previously parked sessions
    Unpark,
}
