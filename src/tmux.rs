use std::process::Command;

use crate::session;

/// Switch to a tmux session (inside tmux) or attach to it (outside tmux).
pub fn switch_to_session(name: &str) {
    let inside_tmux = std::env::var("TMUX").is_ok();
    if inside_tmux {
        let _ = Command::new("tmux")
            .args(["switch-client", "-t", name])
            .status();
    } else {
        let _ = Command::new("tmux")
            .args(["attach-session", "-t", name])
            .status();
    }
}

/// Which AI agent to launch in the session.
#[derive(Clone, Copy, PartialEq, Default)]
pub enum Agent {
    #[default]
    Claude1,
    Claude2,
    Codex,
    Gemini,
    Opencode,
    Pi,
}

impl Agent {
    pub fn all() -> &'static [Agent] {
        &[Agent::Claude1, Agent::Claude2, Agent::Codex, Agent::Gemini, Agent::Opencode, Agent::Pi]
    }

    pub fn from_str(s: &str) -> Option<Agent> {
        Agent::all().iter().find(|a| a.label() == s).copied()
    }

    pub fn label(&self) -> &'static str {
        match self {
            Agent::Claude1 => "claude",
            Agent::Claude2 => "claude-2",
            Agent::Codex => "codex",
            Agent::Gemini => "gemini",
            Agent::Opencode => "opencode",
            Agent::Pi => "pi",
        }
    }

    pub fn binary(&self) -> &'static str {
        match self {
            Agent::Claude1 | Agent::Claude2 => "claude",
            Agent::Codex => "codex",
            Agent::Gemini => "gemini",
            Agent::Opencode => "opencode",
            Agent::Pi => "pi",
        }
    }

    /// Returns (binary_path, flags, optional_env_var as "KEY=VALUE").
    fn command_info(&self) -> (String, &'static [&'static str], Option<String>) {
        match self {
            Agent::Claude1 => {
                let path = which_tool("claude").unwrap_or_else(|| "claude".to_string());
                (path, &["--dangerously-skip-permissions"], None)
            }
            Agent::Claude2 => {
                let path = which_tool("claude").unwrap_or_else(|| "claude".to_string());
                let dir = dirs::home_dir()
                    .map(|h| h.join(".claude-2").to_string_lossy().to_string())
                    .unwrap_or_else(|| "~/.claude-2".to_string());
                (
                    path,
                    &["--dangerously-skip-permissions"],
                    Some(format!("CLAUDE_CONFIG_DIR={dir}")),
                )
            }
            Agent::Codex => {
                let path = which_tool("codex").unwrap_or_else(|| "codex".to_string());
                (path, &["--sandbox", "danger-full-access"], None)
            }
            Agent::Gemini => {
                let path = which_tool("gemini").unwrap_or_else(|| "gemini".to_string());
                (path, &["-y"], None)
            }
            Agent::Opencode => {
                let path = which_tool("opencode").unwrap_or_else(|| "opencode".to_string());
                (path, &[], None)
            }
            Agent::Pi => {
                let path = which_tool("pi").unwrap_or_else(|| "pi".to_string());
                (path, &[], None)
            }
        }
    }
}

/// Launch an AI agent in a new tmux session with the given name and working directory.
/// Returns the session name on success.
pub fn create_session(
    name: &str,
    cwd: &str,
    agent: Agent,
    tag: Option<&str>,
) -> Result<String, String> {
    let base_name = sanitize_session_name(name);
    let session_name = unique_session_name(&base_name);

    let (cmd_path, flags, maybe_env) = agent.command_info();

    let mut args: Vec<String> = vec![
        "new-session".into(),
        "-d".into(),
        "-s".into(),
        session_name.clone(),
        "-c".into(),
        cwd.to_string(),
    ];
    if let Some(env_str) = maybe_env {
        args.push("-e".into());
        args.push(env_str);
    }
    if let Some(t) = tag {
        args.push("-e".into());
        args.push(format!("RECON_TAG={t}"));
    }
    args.push("-e".into());
    args.push(format!("RECON_AGENT={}", agent.label()));
    args.push(cmd_path);
    args.extend(flags.iter().map(|s| s.to_string()));

    let status = Command::new("tmux")
        .args(&args)
        .status()
        .map_err(|e| format!("Failed to create tmux session: {e}"))?;

    if !status.success() {
        return Err("tmux new-session failed".to_string());
    }

    Ok(session_name)
}

/// Resume a claude session in a new tmux session.
/// No-op if the session is already running — returns the existing tmux name.
pub fn resume_session(session_id: &str, name: Option<&str>) -> Result<String, String> {
    if let Some(existing) = session::find_live_tmux_for_session(session_id) {
        return Ok(existing);
    }

    let tmux_name = name
        .map(|n| n.to_string())
        .unwrap_or_else(|| session_id[..6.min(session_id.len())].to_string());

    // Use the original session's cwd so we start in the right project directory.
    let cwd = session::find_session_cwd(session_id)
        .or_else(|| {
            std::env::current_dir()
                .map(|p| p.to_string_lossy().to_string())
                .ok()
        })
        .unwrap_or_else(|| ".".to_string());

    let base_name = sanitize_session_name(&tmux_name);
    let session_name = unique_session_name(&base_name);

    let claude_path = which_tool("claude").unwrap_or_else(|| "claude".to_string());
    // Store the original session-id in the tmux session environment so recon can
    // find the right JSONL without parsing process command lines.
    let env_var = format!("RECON_RESUMED_FROM={session_id}");
    let status = Command::new("tmux")
        .args([
            "new-session",
            "-d",
            "-s",
            &session_name,
            "-c",
            &cwd,
            "-e",
            &env_var,
            &claude_path,
            "--resume",
            session_id,
        ])
        .status()
        .map_err(|e| format!("Failed to create tmux session: {e}"))?;

    if !status.success() {
        return Err("tmux new-session failed".to_string());
    }

    Ok(session_name)
}

/// Get default session name and cwd for a new session.
pub fn default_new_session_info() -> (String, String) {
    let cwd = std::env::current_dir()
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_else(|_| ".".to_string());

    let name = std::path::Path::new(&cwd)
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| "claude".to_string());

    (name, cwd)
}

fn unique_session_name(base_name: &str) -> String {
    if !session_exists(base_name) {
        return base_name.to_string();
    }
    let mut n = 2;
    loop {
        let candidate = format!("{base_name}-{n}");
        if !session_exists(&candidate) {
            return candidate;
        }
        n += 1;
    }
}

fn session_exists(name: &str) -> bool {
    Command::new("tmux")
        .args(["has-session", "-t", name])
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

pub fn is_installed(name: &str) -> bool {
    which_tool(name).is_some()
}

fn which_tool(name: &str) -> Option<String> {
    let output = Command::new("which").arg(name).output().ok()?;
    let path = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if path.is_empty() {
        None
    } else {
        Some(path)
    }
}

/// Kill a tmux session by name.
pub fn kill_session(name: &str) -> bool {
    Command::new("tmux")
        .args(["kill-session", "-t", name])
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// Sanitize a string for use as a tmux session name (no dots or colons).
fn sanitize_session_name(name: &str) -> String {
    name.replace('.', "-").replace(':', "-")
}
