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
    for suffix in &["gemini"] {
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
        "claude" | "claude-2" => fetch_claude(account),
        _ => None, // opencode, pi, and others don't have usage limits via CLI
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

/// macOS Keychain entry name for a given Claude account.
/// cc2 is auto-discovered from the keychain (the unique
/// "Claude Code-credentials-<hex>" entry); override with
/// RECON_CC2_KEYCHAIN if multiple non-default accounts exist.
fn claude_keychain_entry(account: &str) -> Option<String> {
    match account {
        "claude" => Some("Claude Code-credentials".to_string()),
        "claude-2" => std::env::var("RECON_CC2_KEYCHAIN")
            .ok()
            .or_else(discover_cc2_keychain_entry),
        _ => None,
    }
}

/// Scan `security dump-keychain` for the unique
/// "Claude Code-credentials-<hex>" entry that backs a non-default
/// CLAUDE_CONFIG_DIR. Returns None if zero or multiple are found.
fn discover_cc2_keychain_entry() -> Option<String> {
    let out = Command::new("security").arg("dump-keychain").output().ok()?;
    let text = String::from_utf8_lossy(&out.stdout);
    const PREFIX: &str = "\"Claude Code-credentials-";

    let mut found: Option<String> = None;
    for line in text.lines() {
        let Some(start) = line.find(PREFIX) else { continue };
        let rest = &line[start + 1..]; // skip leading quote
        let Some(end) = rest.find('"') else { continue };
        let entry = &rest[..end];
        let suffix = &entry["Claude Code-credentials-".len()..];
        if suffix.is_empty() || !suffix.bytes().all(|b| b.is_ascii_hexdigit()) {
            continue;
        }
        match &found {
            Some(prev) if prev != entry => return None, // ambiguous — bail
            None => found = Some(entry.to_string()),
            _ => {}
        }
    }
    found
}

fn read_keychain_blob(entry: &str) -> Option<serde_json::Value> {
    let out = Command::new("security")
        .args(["find-generic-password", "-s", entry, "-w"])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let blob = String::from_utf8_lossy(&out.stdout);
    serde_json::from_str(blob.trim()).ok()
}

fn read_oauth_token(entry: &str) -> Option<String> {
    read_keychain_blob(entry)?
        .get("claudeAiOauth")?
        .get("accessToken")?
        .as_str()
        .map(String::from)
}

/// Cached subscription type ("pro", "max", "free", …) for a Claude account.
/// Reads from the same Keychain blob as the OAuth token, once per process.
static SUBSCRIPTION_CACHE: OnceLock<Mutex<HashMap<String, Option<String>>>> = OnceLock::new();

pub fn subscription_type(account: &str) -> Option<String> {
    let cache = SUBSCRIPTION_CACHE.get_or_init(|| Mutex::new(HashMap::new()));
    if let Ok(c) = cache.lock() {
        if let Some(v) = c.get(account) {
            return v.clone();
        }
    }
    let entry = claude_keychain_entry(account)?;
    let result = read_keychain_blob(&entry)
        .and_then(|v| v.get("claudeAiOauth")?.get("subscriptionType")?.as_str().map(String::from));
    if let Ok(mut c) = cache.lock() {
        c.insert(account.to_string(), result.clone());
    }
    result
}

fn fetch_claude(account: &str) -> Option<UsageInfo> {
    let entry = claude_keychain_entry(account)?;
    let token = read_oauth_token(&entry)?;

    let out = Command::new("curl")
        .args([
            "-sS",
            "--max-time",
            "5",
            "-H",
            "Accept: application/json",
            "-H",
            &format!("Authorization: Bearer {token}"),
            "-H",
            "anthropic-beta: oauth-2025-04-20",
            "https://api.anthropic.com/api/oauth/usage",
        ])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }

    let v: serde_json::Value = serde_json::from_slice(&out.stdout).ok()?;

    let pct = |key: &str| -> Option<u32> {
        v.get(key)?
            .get("utilization")?
            .as_f64()
            .map(|f| f.floor() as u32)
    };
    let resets = |key: &str| -> Option<String> {
        v.get(key)?.get("resets_at")?.as_str().map(String::from)
    };

    let info = UsageInfo {
        five_hour_pct: pct("five_hour"),
        resets_at: resets("five_hour"),
        weekly_pct: pct("seven_day"),
        weekly_resets_at: resets("seven_day"),
    };
    if info.five_hour_pct.is_none() && info.weekly_pct.is_none() {
        return None;
    }
    Some(info)
}

// ── Codex ─────────────────────────────────────────────────────────────────────

/// Fetch Codex rate-limit usage via the same OAuth-protected endpoint that the
/// TUI status line uses (`https://chatgpt.com/backend-api/wham/usage`).
/// Reads OAuth credentials from `~/.codex/auth.json`.
fn fetch_codex() -> Option<UsageInfo> {
    let auth_path = dirs::home_dir()?.join(".codex/auth.json");
    let blob = std::fs::read_to_string(&auth_path).ok()?;
    let v: serde_json::Value = serde_json::from_str(&blob).ok()?;
    let access_token = v.get("tokens")?.get("access_token")?.as_str()?;
    let account_id = v
        .get("tokens")
        .and_then(|t| t.get("account_id"))
        .and_then(|a| a.as_str())
        .unwrap_or("");

    let mut args = vec![
        "-sS".to_string(),
        "--max-time".to_string(),
        "5".to_string(),
        "-H".to_string(),
        "Accept: application/json".to_string(),
        "-H".to_string(),
        format!("Authorization: Bearer {access_token}"),
        "-H".to_string(),
        "User-Agent: codex-cli/0.0.0".to_string(),
    ];
    if !account_id.is_empty() {
        args.push("-H".to_string());
        args.push(format!("ChatGPT-Account-Id: {account_id}"));
    }
    args.push("https://chatgpt.com/backend-api/wham/usage".to_string());

    let out = Command::new("curl").args(&args).output().ok()?;
    if !out.status.success() {
        return None;
    }
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).ok()?;
    let rl = v.get("rate_limit")?;

    let pct = |window: &str| -> Option<u32> {
        rl.get(window)?
            .get("used_percent")?
            .as_f64()
            .map(|f| f.floor() as u32)
    };
    let resets = |window: &str| -> Option<String> {
        let secs = rl.get(window)?.get("reset_at")?.as_i64()?;
        let dt = chrono::DateTime::<chrono::Utc>::from_timestamp(secs, 0)?;
        Some(dt.to_rfc3339())
    };

    let info = UsageInfo {
        five_hour_pct: pct("primary_window"),
        resets_at: resets("primary_window"),
        weekly_pct: pct("secondary_window"),
        weekly_resets_at: resets("secondary_window"),
    };
    if info.five_hour_pct.is_none() && info.weekly_pct.is_none() {
        return None;
    }
    Some(info)
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

