/// Map raw model IDs to human-friendly display names.
/// The "[1m]" / "[200k]" bracket suffix encodes the context-window flavor;
/// bare IDs are treated as the 200k default.
pub fn display_name(model_id: &str) -> &str {
    match model_id {
        "claude-opus-4-7" | "claude-opus-4-7[1m]" => "Opus 4.7",
        "claude-opus-4-6" | "claude-opus-4-6[1m]" => "Opus 4.6",
        "claude-sonnet-4-6" => "Sonnet 4.6",
        "claude-sonnet-4-5-20250514" => "Sonnet 4.5",
        "claude-haiku-4-5-20251001" => "Haiku 4.5",
        "claude-opus-4-20250514" => "Opus 4",
        "claude-sonnet-4-20250514" => "Sonnet 4",
        _ => model_id,
    }
}

/// Context window size for a given model ID.
/// A bare ID means the 200k default flavor; "[1m]" means the 1M flavor.
pub fn context_window(model_id: &str) -> u64 {
    match model_id {
        "claude-opus-4-7[1m]" | "claude-opus-4-6[1m]" => 1_000_000,
        _ => 200_000,
    }
}

/// Reverse lookup: display name (from /model output, including the
/// "(1M context)" / "(200k context)" suffix) → model ID.
pub fn id_from_display_name(display: &str) -> Option<&'static str> {
    match display.trim() {
        "Opus 4.7"                  => Some("claude-opus-4-7"),
        "Opus 4.7 (1M context)"     => Some("claude-opus-4-7[1m]"),
        "Opus 4.7 (200k context)"   => Some("claude-opus-4-7"),
        "Opus 4.6"                  => Some("claude-opus-4-6"),
        "Opus 4.6 (1M context)"     => Some("claude-opus-4-6[1m]"),
        "Opus 4.6 (200k context)"   => Some("claude-opus-4-6"),
        "Sonnet 4.6"                => Some("claude-sonnet-4-6"),
        "Sonnet 4.5"                => Some("claude-sonnet-4-5-20250514"),
        "Haiku 4.5"                 => Some("claude-haiku-4-5-20251001"),
        "Opus 4"                    => Some("claude-opus-4-20250514"),
        "Sonnet 4"                  => Some("claude-sonnet-4-20250514"),
        _ => None,
    }
}

/// Format model name with optional effort level.
pub fn format_with_effort(model_id: &str, effort: &str) -> String {
    let name = display_name(model_id);
    if effort.is_empty() || effort == "default" {
        name.to_string()
    } else {
        format!("{name} ({effort})")
    }
}
