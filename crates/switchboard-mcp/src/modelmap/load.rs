use super::default::{default_model_tokens, default_provider_aliases};
use super::{Canonical, ModelMap, RawEntry, RawMappingFile};

pub fn from_toml_str(s: &str) -> anyhow::Result<ModelMap> {
    let raw: RawMappingFile = toml::from_str(s)?;
    Ok(build_model_map(raw))
}

pub fn load_from_file(path: &std::path::Path) -> anyhow::Result<ModelMap> {
    let content = std::fs::read_to_string(path)?;
    from_toml_str(&content)
}

pub fn load_default() -> ModelMap {
    ModelMap {
        by_token: default_model_tokens(),
        provider_aliases: default_provider_aliases(),
    }
}

fn build_model_map(raw: RawMappingFile) -> ModelMap {
    let mut by_token = std::collections::HashMap::new();
    for e in raw.mappings.into_iter() {
        insert_entry(&mut by_token, &e);
        if let Some(aliases) = e.aliases.as_ref() {
            for a in aliases {
                let mut alias = e.clone();
                alias.token = a.clone();
                insert_entry(&mut by_token, &alias);
            }
        }
    }
    let mut provider_aliases = default_provider_aliases();
    if let Some(m) = raw.provider_aliases {
        for (k, v) in m.into_iter() {
            provider_aliases.insert(k.to_ascii_lowercase(), v);
        }
    }
    ModelMap {
        by_token,
        provider_aliases,
    }
}

fn insert_entry(map: &mut std::collections::HashMap<String, Canonical>, e: &RawEntry) {
    let key = e.token.to_ascii_lowercase();
    map.insert(
        key,
        Canonical {
            model: e.to_model.clone(),
            provider: e.to_provider.clone(),
        },
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_and_build_map() {
        let toml = r#"
[[mappings]]
token = "GPT-4.1"
to_model = "gpt-4.1"
to_provider = "openai"
[provider_aliases]
OpenAI = "openai"
"#;
        let m = from_toml_str(toml).expect("parse ok");
        assert!(m.by_token.contains_key("gpt-4.1"));
        assert_eq!(
            m.by_token.get("gpt-4.1").unwrap().provider.as_deref(),
            Some("openai")
        );
        assert!(m.provider_aliases.contains_key("openai"));
    }
}
