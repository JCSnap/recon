use std::collections::HashMap;
use std::process::Command;
use std::sync::{Mutex, OnceLock};
use std::thread;
use std::time::Duration;

#[derive(Clone, Debug)]
pub struct UsageInfo {
    pub five_hour_pct: Option<u32>,
    pub resets_at: Option<String>,
    pub weekly_pct: Option<u32>,
    pub weekly_resets_at: Option<String>,
}

impl UsageInfo {
    /// Return the effective usage percentage — the worse of 5h and weekly limits.
    pub fn effective_pct(&self) -> Option<u32> {
        match (self.five_hour_pct, self.weekly_pct) {
            (Some(a), Some(b)) => Some(a.max(b)),
            (a, b) => a.or(b),
        }
    }

    /// Return the reset time for whichever limit is the bottleneck.
    pub fn effective_resets_at(&self) -> Option<&str> {
        match (self.five_hour_pct, self.weekly_pct) {
            (Some(a), Some(b)) if b > a => self.weekly_resets_at.as_deref(),
            _ => self
                .resets_at
                .as_deref()
                .or(self.weekly_resets_at.as_deref()),
        }
    }
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

/// Kill any leftover `_recon_usage_*` tmux sessions.
pub fn cleanup() {
    for suffix in &["claude", "claude_2", "codex", "gemini"] {
        let session_name = format!("_recon_usage_{}", suffix);
        tmux_kill(&session_name);
    }
}

/// Trigger a background usage fetch for the given agent label.
/// Retries up to 2 times (3 attempts total) with a 5-second delay between attempts.
pub fn trigger_fetch(account: &str) {
    let account = account.to_string();
    thread::spawn(move || {
        for attempt in 0..3 {
            if let Some(info) = fetch(&account) {
                if let Ok(mut c) = cache().lock() {
                    c.insert(account, info);
                }
                return;
            }
            if attempt < 2 {
                thread::sleep(Duration::from_secs(5));
            }
        }
    });
}

pub fn fetch_sync(account: &str) -> Option<UsageInfo> {
    fetch(account)
}

fn fetch(account: &str) -> Option<UsageInfo> {
    match account {
        "codex" => fetch_codex(),
        "gemini" => fetch_gemini(),
        _ => fetch_claude(account), // "claude" or "claude-2"
    }
}

// ── tmux helpers ──────────────────────────────────────────────────────────────

fn tmux_kill(session: &str) {
    let _ = Command::new("tmux")
        .args(["kill-session", "-t", session])
        .output();
}

fn tmux_send(session: &str, keys: &[&str]) {
    let mut args = vec!["send-keys", "-t", session];
    args.extend_from_slice(keys);
    let _ = Command::new("tmux").args(&args).status();
}

fn tmux_capture(session: &str) -> Option<String> {
    let output = Command::new("tmux")
        .args(["capture-pane", "-t", session, "-p", "-S", "-100"])
        .output()
        .ok()?;
    Some(String::from_utf8_lossy(&output.stdout).to_string())
}

// ── Claude / Claude-2 ─────────────────────────────────────────────────────────

fn fetch_claude(account: &str) -> Option<UsageInfo> {
    let session_name = format!("_recon_usage_{}", account.replace('-', "_"));
    tmux_kill(&session_name);

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

    // Wait for main UI ready or trust prompt.
    let ready = wait_for_either(
        &session_name,
        "bypass permissions on",
        "trust this folder",
        20,
    );
    match ready {
        Some(1) => {}
        Some(2) => {
            tmux_send(&session_name, &["", "Enter"]);
            if !wait_for_pane(&session_name, "bypass permissions on", 15) {
                tmux_kill(&session_name);
                return None;
            }
        }
        _ => {
            tmux_kill(&session_name);
            return None;
        }
    }

    tmux_send(&session_name, &["/usage", "Enter"]);

    // Poll for the usage output to appear instead of a fixed sleep.
    if !wait_for_pane(&session_name, "% used", 10) {
        tmux_kill(&session_name);
        return None;
    }
    // Small settle delay so the full output (including "Resets") renders.
    thread::sleep(Duration::from_millis(500));

    let content = tmux_capture(&session_name)?;
    tmux_kill(&session_name);
    parse_claude_output(&content)
}

// ── Codex ─────────────────────────────────────────────────────────────────────

fn fetch_codex() -> Option<UsageInfo> {
    let session_name = "_recon_usage_codex";
    tmux_kill(session_name);

    let ok = Command::new("tmux")
        .args([
            "new-session",
            "-d",
            "-s",
            session_name,
            "-x",
            "120",
            "-y",
            "40",
            "-c",
            "/tmp",
            "codex",
            "--full-auto",
        ])
        .status()
        .map(|s| s.success())
        .unwrap_or(false);
    if !ok {
        return None;
    }

    // Wait for status bar ("% left") or trust prompt ("do you trust").
    let ready = wait_for_either(session_name, "% left", "do you trust", 20);
    match ready {
        Some(1) => {}
        Some(2) => {
            // Accept trust prompt (option 1 is default — just press Enter).
            tmux_send(session_name, &["", "Enter"]);
            if !wait_for_pane(session_name, "% left", 15) {
                tmux_kill(session_name);
                return None;
            }
        }
        _ => {
            tmux_kill(session_name);
            return None;
        }
    }

    // Send /status to get account-level usage (the status bar only shows session usage).
    // Type the command, dismiss autocomplete with Escape, then press Enter.
    tmux_send(session_name, &["/status"]);
    thread::sleep(Duration::from_millis(500));
    tmux_send(session_name, &["Escape"]);
    thread::sleep(Duration::from_millis(300));
    tmux_send(session_name, &["Enter"]);

    // Poll for the 5h or weekly limit line to appear in the /status output.
    if wait_for_either(session_name, "5h limit", "weekly limit", 10).is_none() {
        // Fall back to status bar if /status didn't produce output.
        let content = tmux_capture(session_name)?;
        tmux_kill(session_name);
        return parse_codex_output(&content);
    }
    thread::sleep(Duration::from_millis(500));

    let content = tmux_capture(session_name)?;
    tmux_kill(session_name);
    parse_codex_output(&content)
}

// ── Gemini ────────────────────────────────────────────────────────────────────

fn fetch_gemini() -> Option<UsageInfo> {
    let session_name = "_recon_usage_gemini";
    tmux_kill(session_name);

    let ok = Command::new("tmux")
        .args([
            "new-session",
            "-d",
            "-s",
            session_name,
            "-c",
            "/tmp",
            "gemini",
            "-y",
        ])
        .status()
        .map(|s| s.success())
        .unwrap_or(false);
    if !ok {
        return None;
    }

    // Wait for input prompt ("type your message") or trust prompt ("do you trust").
    let ready = wait_for_either(session_name, "type your message", "do you trust", 20);
    match ready {
        Some(1) => {}
        Some(2) => {
            tmux_send(session_name, &["", "Enter"]);
            if !wait_for_pane(session_name, "type your message", 15) {
                tmux_kill(session_name);
                return None;
            }
        }
        _ => {
            tmux_kill(session_name);
            return None;
        }
    }

    // Send /stats — gemini needs text and Enter as separate sends.
    tmux_send(session_name, &["/stats"]);
    thread::sleep(Duration::from_millis(200));
    tmux_send(session_name, &["Enter"]);

    // Wait for the stats table to appear.
    if !wait_for_pane(session_name, "model usage", 10) {
        tmux_kill(session_name);
        return None;
    }
    thread::sleep(Duration::from_millis(500));

    let content = tmux_capture(session_name)?;
    tmux_kill(session_name);
    parse_gemini_output(&content)
}

// ── Polling helpers ───────────────────────────────────────────────────────────

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

/// Poll the pane until `needle` appears (case-insensitive).
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

// ── Parsers ───────────────────────────────────────────────────────────────────

fn parse_claude_output(content: &str) -> Option<UsageInfo> {
    let mut five_hour_pct = None;
    let mut resets_at = None;

    for line in content.lines() {
        let clean = strip_ansi(line.trim());

        // Take the FIRST "XX% used" line (current session, not extra usage).
        if five_hour_pct.is_none() && (clean.contains("% used") || clean.contains("%\u{a0}used")) {
            if let Some(pct) = extract_percent(&clean) {
                five_hour_pct = Some(pct);
            }
        }

        // Take the FIRST "Resets ..." line that isn't a billing line.
        if resets_at.is_none() && !clean.contains('$') {
            if let Some(pos) = clean.find("Resets ") {
                let after = clean[pos + "Resets ".len()..].trim().to_string();
                if !after.is_empty() {
                    resets_at = Some(after);
                }
            }
        }
    }

    if five_hour_pct.is_some() || resets_at.is_some() {
        Some(UsageInfo {
            five_hour_pct,
            resets_at,
            weekly_pct: None,
            weekly_resets_at: None,
        })
    } else {
        None
    }
}

fn parse_codex_output(content: &str) -> Option<UsageInfo> {
    // Parse the "5h limit" and "Weekly limit" lines from /status output.
    // e.g. "5h limit:  [...] 86% left (resets 11:31)"
    //      "Weekly limit:  [...] 82% left (resets 16:27 on 29 Mar)"
    // Fall back to the status bar "X% left" if /status output isn't present.
    let mut five_hour_pct = None;
    let mut resets_at = None;
    let mut weekly_pct = None;
    let mut weekly_resets_at = None;
    let mut fallback_pct = None;

    for line in content.lines() {
        let clean = strip_ansi(line.trim());
        let lower = clean.to_lowercase();

        if lower.contains("5h limit") && clean.contains("% left") {
            if let Some(pct_left) = extract_percent(&clean) {
                five_hour_pct = Some(100u32.saturating_sub(pct_left));
            }
            if let Some(reset) = extract_resets(&clean) {
                resets_at = Some(reset);
            }
        } else if lower.contains("weekly limit") && clean.contains("% left") {
            if let Some(pct_left) = extract_percent(&clean) {
                weekly_pct = Some(100u32.saturating_sub(pct_left));
            }
            if let Some(reset) = extract_resets(&clean) {
                weekly_resets_at = Some(reset);
            }
        } else if fallback_pct.is_none() && clean.contains("% left") {
            if let Some(pct_left) = extract_percent(&clean) {
                fallback_pct = Some(100u32.saturating_sub(pct_left));
            }
        }
    }

    let pct = five_hour_pct.or(fallback_pct);
    if pct.is_some() || resets_at.is_some() || weekly_pct.is_some() {
        Some(UsageInfo {
            five_hour_pct: pct,
            resets_at,
            weekly_pct,
            weekly_resets_at,
        })
    } else {
        None
    }
}

/// Extract reset time from a line containing "resets HH:MM" or similar.
fn extract_resets(clean: &str) -> Option<String> {
    if let Some(pos) = clean.find("resets ") {
        let after = &clean[pos + "resets ".len()..];
        // Trim trailing paren/bracket/box chars.
        let trimmed = after.trim_end_matches(|c: char| c == ')' || c == '│' || c == ' ');
        if !trimmed.is_empty() {
            return Some(trimmed.to_string());
        }
    }
    None
}

fn parse_gemini_output(content: &str) -> Option<UsageInfo> {
    // Parse the /stats model table. Example line (after ANSI strip):
    // "│  gemini-2.5-flash   -   ▬▬▬▬▬▬▬▬   0%  8:19 PM (24h)   │"
    // Take the model with the highest usage %, carrying its reset time.
    let mut max_pct: Option<u32> = None;
    let mut resets_at: Option<String> = None;

    for line in content.lines() {
        let clean = strip_ansi(line.trim());
        if !clean.contains("gemini-") {
            continue;
        }
        if let Some(pct) = extract_percent(&clean) {
            let update = match max_pct {
                None => true,
                Some(m) => pct > m,
            };
            if update {
                max_pct = Some(pct);
                // Extract reset time: text after "X%  " on this line.
                resets_at = extract_text_after_percent(&clean);
            }
        }
    }

    if max_pct.is_some() || resets_at.is_some() {
        Some(UsageInfo {
            five_hour_pct: max_pct,
            resets_at,
            weekly_pct: None,
            weekly_resets_at: None,
        })
    } else {
        None
    }
}

/// Return the trimmed text that follows the last "X%" in `s`, stopping at box borders.
fn extract_text_after_percent(s: &str) -> Option<String> {
    let bytes = s.as_bytes();
    let mut i = 0;
    let mut after_pos: Option<usize> = None;
    while i < bytes.len() {
        if bytes[i].is_ascii_digit() {
            let start = i;
            while i < bytes.len() && bytes[i].is_ascii_digit() {
                i += 1;
            }
            if i < bytes.len() && bytes[i] == b'%' {
                after_pos = Some(i + 1);
            }
        } else {
            i += 1;
        }
    }
    after_pos.map(|pos| {
        s[pos..]
            .split('│') // stop at box border
            .next()
            .unwrap_or("")
            .trim()
            .to_string()
    })
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_codex_status_output() {
        let content = r#"
│  5h limit:             [████████░░░░░░░░░░░░] 42% left (resets 11:31)           │
│  Weekly limit:         [████████████████░░░░] 82% left (resets 16:27 on 29 Mar) │

  gpt-5.4 medium · 100% left · /private/tmp
"#;
        let info = parse_codex_output(content).expect("should parse");
        // 42% left → 58% used
        assert_eq!(info.five_hour_pct, Some(58));
        assert!(info.resets_at.is_some(), "should have resets_at");
        assert!(
            info.resets_at.as_ref().unwrap().contains("11:31"),
            "resets_at should contain 11:31, got: {:?}",
            info.resets_at
        );
        // 82% left → 18% used
        assert_eq!(info.weekly_pct, Some(18));
        assert!(
            info.weekly_resets_at.as_ref().unwrap().contains("16:27"),
            "weekly_resets_at should contain 16:27, got: {:?}",
            info.weekly_resets_at
        );
        // effective should be max(58, 18) = 58
        assert_eq!(info.effective_pct(), Some(58));
    }

    #[test]
    fn test_parse_codex_status_bar_fallback() {
        // When /status output isn't present, fall back to status bar.
        let content = "  gpt-5.4 medium · 100% left · /private/tmp\n";
        let info = parse_codex_output(content).expect("should parse");
        // 100% left → 0% used
        assert_eq!(info.five_hour_pct, Some(0));
    }

    #[test]
    fn test_parse_codex_prefers_5h_over_status_bar() {
        let content = r#"
│  5h limit:             [████████░░░░░░░░░░░░] 86% left (resets 11:31)           │
  gpt-5.4 medium · 100% left · /private/tmp
"#;
        let info = parse_codex_output(content).expect("should parse");
        // Should use 5h limit (86% left → 14% used), not status bar (100% left → 0%)
        assert_eq!(info.five_hour_pct, Some(14));
    }

    #[test]
    fn test_parse_codex_weekly_exhausted() {
        // When weekly limit is exhausted but 5h limit has capacity,
        // effective_pct should reflect the weekly exhaustion.
        let content = r#"
│  5h limit:             [████████████████████] 100% left (resets 16:51)           │
│  Weekly limit:         [░░░░░░░░░░░░░░░░░░░░] 0% left (resets 16:27 on 29 Mar)  │

  gpt-5.4 medium · 100% left · /private/tmp
"#;
        let info = parse_codex_output(content).expect("should parse");
        // 5h: 100% left → 0% used
        assert_eq!(info.five_hour_pct, Some(0));
        // Weekly: 0% left → 100% used
        assert_eq!(info.weekly_pct, Some(100));
        // Effective: max(0, 100) = 100
        assert_eq!(info.effective_pct(), Some(100));
        // Bottleneck is weekly, so effective reset should be weekly's
        assert!(info.effective_resets_at().unwrap().contains("16:27"));
    }
}
