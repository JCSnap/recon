use std::collections::HashMap;
use std::process::Command;
use std::sync::{Mutex, OnceLock};
use std::thread;
use std::time::Duration;

#[derive(Clone, Debug)]
pub struct UsageInfo {
    pub five_hour_pct: Option<u32>,
    pub resets_at: Option<String>,
}

static CACHE: OnceLock<Mutex<HashMap<String, UsageInfo>>> = OnceLock::new();

fn cache() -> &'static Mutex<HashMap<String, UsageInfo>> {
    CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

pub fn get(account: &str) -> Option<UsageInfo> {
    cache().lock().ok()?.get(account).cloned()
}

pub fn store(account: &str, info: UsageInfo) {
    if let Ok(mut c) = cache().lock() {
        c.insert(account.to_string(), info);
    }
}

/// Trigger a background usage fetch for the given agent label ("claude" or "claude-2").
/// No-ops if a fetch is already in progress (the session will already exist).
pub fn trigger_fetch(account: &str) {
    let account = account.to_string();
    thread::spawn(move || {
        if let Some(info) = fetch(&account) {
            if let Ok(mut c) = cache().lock() {
                c.insert(account, info);
            }
        }
    });
}

pub fn fetch_sync(account: &str) -> Option<UsageInfo> {
    fetch(account)
}

fn fetch(account: &str) -> Option<UsageInfo> {
    let session_name = format!("_recon_usage_{}", account.replace('-', "_"));

    // Kill any stale checker session from a previous run.
    let _ = Command::new("tmux")
        .args(["kill-session", "-t", &session_name])
        .output();

    let mut args: Vec<String> = vec![
        "new-session".into(),
        "-d".into(),
        "-s".into(),
        session_name.clone(),
        "-c".into(),
        "/tmp".into(),
    ];

    if account == "claude-2" {
        if let Some(home) = dirs::home_dir() {
            args.push("-e".into());
            args.push(format!(
                "CLAUDE_CONFIG_DIR={}",
                home.join(".claude-2").display()
            ));
        }
    }

    // Use the claude binary (resolved via PATH)
    args.push("claude".into());
    args.push("--dangerously-skip-permissions".into());

    let ok = Command::new("tmux")
        .args(&args)
        .status()
        .map(|s| s.success())
        .unwrap_or(false);

    if !ok {
        return None;
    }

    // Wait for either the trust prompt or the ready main UI (whichever comes first).
    // "bypass permissions on" = main UI ready
    // "trust this folder"     = need to accept trust first
    let ready = wait_for_either(
        &session_name,
        "bypass permissions on",
        "trust this folder",
        20,
    );
    match ready {
        Some(1) => {
            // Main UI appeared directly (directory already trusted).
        }
        Some(2) => {
            // Trust prompt — accept it, then wait for main UI.
            let _ = Command::new("tmux")
                .args(["send-keys", "-t", &session_name, "", "Enter"])
                .status();
            if !wait_for_pane(&session_name, "bypass permissions on", 15) {
                let _ = Command::new("tmux")
                    .args(["kill-session", "-t", &session_name])
                    .status();
                return None;
            }
        }
        _ => {
            // Timed out.
            let _ = Command::new("tmux")
                .args(["kill-session", "-t", &session_name])
                .status();
            return None;
        }
    }

    // Send /usage slash command.
    let _ = Command::new("tmux")
        .args(["send-keys", "-t", &session_name, "/usage", "Enter"])
        .status();

    // Wait for the dialog to render.
    thread::sleep(Duration::from_millis(2000));

    // Capture the pane.
    let output = Command::new("tmux")
        .args(["capture-pane", "-t", &session_name, "-p", "-S", "-50"])
        .output()
        .ok()?;

    let content = String::from_utf8_lossy(&output.stdout).to_string();


    // Clean up.
    let _ = Command::new("tmux")
        .args(["kill-session", "-t", &session_name])
        .status();

    parse_output(&content)
}

/// Poll until one of two needles appears. Returns Some(1) for needle_a,
/// Some(2) for needle_b, or None on timeout.
fn wait_for_either(
    session_name: &str,
    needle_a: &str,
    needle_b: &str,
    timeout_secs: u64,
) -> Option<u8> {
    let deadline = std::time::Instant::now() + Duration::from_secs(timeout_secs);
    let a = needle_a.to_lowercase();
    let b = needle_b.to_lowercase();
    while std::time::Instant::now() < deadline {
        if let Ok(out) = Command::new("tmux")
            .args(["capture-pane", "-t", session_name, "-p"])
            .output()
        {
            let content = String::from_utf8_lossy(&out.stdout).to_lowercase();
            if content.contains(&a) {
                return Some(1);
            }
            if content.contains(&b) {
                return Some(2);
            }
        }
        thread::sleep(Duration::from_millis(400));
    }
    None
}

/// Poll the pane until `needle` appears in its content (case-insensitive).
/// Returns true if found within `timeout_secs`, false if timed out.
fn wait_for_pane(session_name: &str, needle: &str, timeout_secs: u64) -> bool {
    let deadline = std::time::Instant::now() + Duration::from_secs(timeout_secs);
    let needle_lower = needle.to_lowercase();
    while std::time::Instant::now() < deadline {
        if let Ok(out) = Command::new("tmux")
            .args(["capture-pane", "-t", session_name, "-p"])
            .output()
        {
            let content = String::from_utf8_lossy(&out.stdout);
            if content.to_lowercase().contains(&needle_lower) {
                return true;
            }
        }
        thread::sleep(Duration::from_millis(400));
    }
    false
}

fn parse_output(content: &str) -> Option<UsageInfo> {
    let mut five_hour_pct = None;
    let mut resets_at = None;

    for line in content.lines() {
        let clean = strip_ansi(line.trim());

        // Look for "XX% used" anywhere on the line — take the FIRST match (current session).
        if five_hour_pct.is_none() && (clean.contains("% used") || clean.contains("%\u{a0}used")) {
            if let Some(pct) = extract_percent(&clean) {
                five_hour_pct = Some(pct);
            }
        }

        // Look for "Resets ..." line — take the FIRST match.
        if resets_at.is_none() {
            // Skip lines that also contain "$" (extra usage billing line like "Resets Apr 1")
            if !clean.contains('$') {
                if let Some(pos) = clean.find("Resets ") {
                    let after = clean[pos + "Resets ".len()..].trim().to_string();
                    if !after.is_empty() {
                        resets_at = Some(after);
                    }
                }
            }
        }
    }

    if five_hour_pct.is_some() || resets_at.is_some() {
        Some(UsageInfo { five_hour_pct, resets_at })
    } else {
        None
    }
}

fn extract_percent(s: &str) -> Option<u32> {
    // Find all digit sequences immediately followed by '%' and take the last one.
    let mut last = None;
    let bytes = s.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i].is_ascii_digit() {
            let start = i;
            while i < bytes.len() && bytes[i].is_ascii_digit() {
                i += 1;
            }
            if i < bytes.len() && bytes[i] == b'%' {
                if let Ok(n) = s[start..i].parse::<u32>() {
                    last = Some(n);
                }
            }
        } else {
            i += 1;
        }
    }
    last
}

/// Strip ANSI escape sequences from a string.
fn strip_ansi(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\x1b' {
            // Skip until we hit a letter (end of escape sequence).
            for nc in chars.by_ref() {
                if nc.is_ascii_alphabetic() {
                    break;
                }
            }
        } else {
            out.push(c);
        }
    }
    out
}
