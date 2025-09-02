use serde::Deserialize;
use std::path::{Path, PathBuf};

#[derive(Debug, Default, Deserialize)]
pub struct UserConfig {
    pub logging: Option<LoggingCfg>,
    pub agents: Option<AgentsCfg>,
}

#[derive(Debug, Default, Deserialize)]
pub struct LoggingCfg {
    pub to_file: Option<bool>,
    pub dir: Option<String>,
    pub json: Option<bool>,
    pub compact: Option<bool>,
    pub pretty: Option<bool>,
    pub level: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
pub struct AgentsCfg {
    pub enable_codex: Option<bool>,
    pub enable_anthropic: Option<bool>,
    pub enable_vscode: Option<bool>,

    pub codex_dirs: Option<Vec<String>>,      // absolute paths preferred
    pub anthropic_dirs: Option<Vec<String>>,  // absolute paths preferred
    pub vscode_dirs: Option<Vec<String>>,     // absolute paths preferred

    pub prefix_codex: Option<String>,
    pub prefix_anthropic: Option<String>,
    pub prefix_vscode: Option<String>,

    pub filter: Option<String>,

    pub mcp_discovery: Option<bool>,
    pub vscode_user_mcp: Option<String>,
    pub limit_mcp_to_referenced: Option<bool>,
    pub enumerate: Option<bool>,
    pub enum_timeout_ms: Option<u64>,
    pub enum_max_servers: Option<usize>,
    pub enum_strict: Option<bool>,
    pub enum_fallback_all: Option<bool>,

    pub toolmap_enable: Option<bool>,
    pub toolmap_allow_custom_servers: Option<bool>,

    pub model_map_enable: Option<bool>,
    pub model_map_file: Option<String>,
    pub model_map_strict: Option<bool>,
    pub model_map_override_provider: Option<bool>,
    pub model_map_normalize_provider: Option<bool>,
}

pub fn load_user_config(sb_home: &Path) -> anyhow::Result<Option<UserConfig>> {
    let path = sb_home.join("config.toml");
    if !path.exists() {
        return Ok(None);
    }
    let s = std::fs::read_to_string(&path)?;
    let cfg: UserConfig = toml::from_str(&s)?;
    Ok(Some(cfg))
}

pub fn expand_home(path: &str) -> PathBuf {
    if let Some(stripped) = path.strip_prefix("~/") {
        if let Ok(home) = std::env::var("HOME") {
            return PathBuf::from(home).join(stripped);
        }
    }
    PathBuf::from(path)
}

