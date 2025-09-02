//! Naming helpers and provider variants.

/// Convert a display name into a filesystem/profile-safe identifier.
pub fn safe_name(name: &str) -> String {
    let mut out = String::with_capacity(name.len());
    for ch in name.chars() {
        if ch.is_ascii_alphanumeric() {
            out.push(ch.to_ascii_lowercase());
        } else {
            out.push('_');
        }
    }
    out.trim_matches('_').to_string()
}

/// Supported agent provider variants.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AgentVariant {
    Codex,
    Anthropic,
    Vscode,
}

/// Tool name prefixes to apply for each variant when deriving `tool_name`.
pub struct EnvPrefixes<'a> {
    pub codex: &'a str,
    pub anthropic: &'a str,
    pub vscode: &'a str,
}

/// Get the configured tool prefix for a provider variant.
pub fn tool_prefix_for<'a>(variant: AgentVariant, env: &'a EnvPrefixes<'a>) -> &'a str {
    match variant {
        AgentVariant::Codex => env.codex,
        AgentVariant::Anthropic => env.anthropic,
        AgentVariant::Vscode => env.vscode,
    }
}

/// Compose a tool name as `<prefix><safe_name(name)>`.
pub fn tool_name_for(prefix: &str, name: &str) -> String {
    format!("{}{}", prefix, safe_name(name))
}
