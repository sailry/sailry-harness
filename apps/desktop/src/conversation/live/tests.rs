use super::*;
use sailry_protocol::{
    ProviderId,
    conversation::{Model, ModelApi},
};

pub(super) use crate::agent_fixture as support;

pub(crate) mod fixture;

fn provider(endpoint: &str, name: &str) -> Provider {
    Provider {
        request_max_retries: None,
        stream_max_retries: None,
        preset: None,
        binding: None,
        consent: None,
        options: None,
        oauth: None,
        id: ProviderId::new(),
        revision: 0,
        name: name.into(),
        api: ModelApi::ChatCompletions,
        authentication: sailry_protocol::Authentication::ApiKey,
        endpoint: endpoint.into(),
        enabled: true,
        credential: None,
        default_model: name.into(),
        models: vec![Model {
            id: name.into(),
            display_name: None,
            context: 4096,
            output: 128,
            capabilities: None,
            vision: false,
            tools: false,
            reasoning: true,
            web_search: false,
            generates: vec![],
            efforts: vec![sailry_protocol::Effort::Low, sailry_protocol::Effort::High],
            custom_efforts: false,
            compaction_tokens: None,
            default_effort: sailry_protocol::Effort::High,
        }],
    }
}
