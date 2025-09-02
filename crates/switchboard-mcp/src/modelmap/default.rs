use super::types::Canonical;
use std::collections::HashMap;

pub fn default_provider_aliases() -> HashMap<String, String> {
    let mut m = HashMap::new();
    for (alias, canon) in [
        ("openai", "openai"),
        ("OpenAI", "openai"),
        ("anthropic", "anthropic"),
        ("Anthropic", "anthropic"),
        ("Claude", "anthropic"),
    ] {
        m.insert(alias.to_ascii_lowercase(), canon.to_string());
    }
    m
}

pub fn default_model_tokens() -> HashMap<String, Canonical> {
    let mut m: HashMap<String, Canonical> = HashMap::new();

    let mk = |model: &str, provider: &str| Canonical {
        model: model.to_string(),
        provider: Some(provider.to_string()),
    };

    // Anthropic common names → OpenAI equivalents
    m.insert("sonnet".into(), mk("gpt-5", "openai"));
    m.insert("opus".into(), mk("gpt-5", "openai"));
    m.insert("haiku".into(), mk("gpt-5-mini", "openai"));
    m.insert("claude opus 4".into(), mk("gpt-5", "openai"));
    m.insert("claude opus 4.1".into(), mk("gpt-5", "openai"));
    m.insert("claude sonnet 4".into(), mk("gpt-5", "openai"));
    m.insert("claude sonnet 3.7".into(), mk("gpt-5", "openai"));
    m.insert("claude 3.7".into(), mk("gpt-5", "openai"));
    m.insert("claude 3.7 thinking".into(), mk("gpt-5", "openai"));
    m.insert("claude haiku 3".into(), mk("gpt-5-mini", "openai"));
    m.insert("claude haiku 3.5".into(), mk("gpt-5-mini", "openai"));

    // VS Code common names → OpenAI equivalents
    m.insert("claude sonnet 3.5".into(), mk("gpt-5", "openai"));
    m.insert("gemini 2.5 pro".into(), mk("gpt-5", "openai"));
    m.insert("gpt-4.1".into(), mk("gpt-4.1", "openai"));
    m.insert("gpt-4o".into(), mk("gpt-4o", "openai"));
    m.insert("gpt-5 mini (preview)".into(), mk("gpt-5-mini", "openai"));
    m.insert("gpt-5 mini".into(), mk("gpt-5-mini", "openai"));
    m.insert("gpt-5-mini".into(), mk("gpt-5-mini", "openai"));
    m.insert("o3-mini".into(), mk("o3-mini", "openai"));
    m.insert("auto".into(), mk("gpt-5", "openai"));

    m
}
