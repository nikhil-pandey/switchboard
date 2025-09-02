use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Default)]
pub struct ModelMap {
    pub by_token: HashMap<String, Canonical>,
    pub provider_aliases: HashMap<String, String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Canonical {
    pub model: String,
    pub provider: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct RawMappingFile {
    #[serde(default)]
    pub mappings: Vec<RawEntry>,
    #[serde(default)]
    pub provider_aliases: Option<HashMap<String, String>>, // alias -> canonical
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct RawEntry {
    pub token: String,
    pub to_model: String,
    #[serde(default)]
    pub to_provider: Option<String>,
    #[serde(default)]
    pub aliases: Option<Vec<String>>, // additional tokens mapping to the same entry
}

#[derive(Debug, Clone, Copy, Default)]
pub struct ApplyOptions {
    pub normalize_provider: bool,
    pub override_provider: bool,
    pub strict: bool,
}
