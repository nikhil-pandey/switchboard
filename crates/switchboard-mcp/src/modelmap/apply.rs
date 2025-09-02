use super::{ApplyOptions, ModelMap};
use crate::model::AgentConfig;

pub fn apply_to_agent(agent: &mut AgentConfig, map: &ModelMap, opts: ApplyOptions) {
    let Some(mut run) = agent.run.clone() else {
        return;
    };

    // Normalize provider alias if requested
    if opts.normalize_provider
        && let Some(p) = run.model_provider.as_ref()
    {
        let key = p.to_ascii_lowercase();
        if let Some(canon) = map.provider_aliases.get(&key)
            && Some(canon) != run.model_provider.as_ref()
        {
            run.model_provider = Some(canon.clone());
        }
    }

    if let Some(m) = run.model.as_ref() {
        let key = m.to_ascii_lowercase();
        if let Some(canon) = map.by_token.get(&key) {
            // Always normalize the model string
            run.model = Some(canon.model.clone());
            // Provider: set if empty, or if override allowed and available
            if let Some(cprov) = canon.provider.as_ref() {
                let should_set = run.model_provider.is_none() || opts.override_provider;
                if should_set {
                    run.model_provider = Some(cprov.clone());
                }
            }
        } else if opts.strict {
            // Strict + unknown token: leave unchanged but log
            tracing::warn!(
                "model mapping strict: unknown token '{}' for agent '{}'",
                m,
                agent.name
            );
        } else {
            tracing::debug!("model mapping: no entry for token '{}'", m);
        }
    }

    agent.run = Some(run);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{AgentConfig, AgentRun};

    fn stub_agent(model: Option<&str>, provider: Option<&str>) -> AgentConfig {
        AgentConfig {
            name: "n".into(),
            description: "d".into(),
            tags: None,
            toggles: None,
            mcp_tool_refs: None,
            instructions_file: None,
            instructions: None,
            run: Some(AgentRun {
                model: model.map(|s| s.to_string()),
                model_provider: provider.map(|s| s.to_string()),
                ..Default::default()
            }),
            mcp_servers: None,
        }
    }

    #[test]
    fn maps_model_and_sets_provider() {
        let mut map = ModelMap::default();
        map.by_token.insert(
            "gpt-4.1".into(),
            super::super::types::Canonical {
                model: "gpt-4.1".into(),
                provider: Some("openai".into()),
            },
        );
        let mut agent = stub_agent(Some("GPT-4.1"), None);
        apply_to_agent(
            &mut agent,
            &map,
            ApplyOptions {
                normalize_provider: true,
                override_provider: false,
                strict: false,
            },
        );
        let run = agent.run.unwrap();
        assert_eq!(run.model.as_deref(), Some("gpt-4.1"));
        assert_eq!(run.model_provider.as_deref(), Some("openai"));
    }

    #[test]
    fn provider_alias_normalization() {
        let mut map = ModelMap::default();
        map.provider_aliases
            .insert("claude".into(), "anthropic".into());
        let mut agent = stub_agent(None, Some("Claude"));
        apply_to_agent(
            &mut agent,
            &map,
            ApplyOptions {
                normalize_provider: true,
                override_provider: false,
                strict: false,
            },
        );
        let run = agent.run.unwrap();
        assert_eq!(run.model_provider.as_deref(), Some("anthropic"));
    }
}
