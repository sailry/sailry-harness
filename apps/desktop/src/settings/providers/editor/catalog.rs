use super::Preset;
use sailry_client::Client;
use sailry_protocol::{
    Command, ErrorCode, Fault, Output,
    conversation::{catalog, discovery},
};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Default)]
pub(super) struct Metadata {
    pub models: BTreeMap<String, catalog::Model>,
    pub failed: bool,
}

pub(super) async fn read(client: &Client, preset: Preset, models: &[discovery::Model]) -> Metadata {
    let provider = match preset {
        Preset::OpenAi | Preset::ChatGpt => Some("openai"),
        Preset::Anthropic => Some("anthropic"),
        Preset::OpenCodeGo => Some("opencode-go"),
        Preset::OpenCodeZen => Some("opencode"),
        Preset::Gemini => Some("google"),
        Preset::Hosted(super::super::vendors::Vendor::DeepSeek) => Some("deepseek"),
        _ => None,
    };
    if models.is_empty() {
        return Metadata::default();
    }
    match tokio::time::timeout(
        std::time::Duration::from_secs(10),
        pages(client, provider, models),
    )
    .await
    {
        Ok(Ok(models)) => Metadata {
            models,
            failed: false,
        },
        Ok(Err(error)) if error.code == ErrorCode::NotConfigured => Metadata::default(),
        _ => Metadata {
            failed: true,
            ..Default::default()
        },
    }
}

async fn pages(
    client: &Client,
    provider: Option<&str>,
    models: &[discovery::Model],
) -> Result<BTreeMap<String, catalog::Model>, Fault> {
    let mut remaining: BTreeSet<_> = models.iter().map(|model| model.id.as_str()).collect();
    let mut result = BTreeMap::new();
    let mut revision = None;
    for _ in 0..32 {
        let query = catalog::Query {
            provider: provider.map(str::to_owned),
            ids: remaining
                .iter()
                .take(100)
                .map(|id| (*id).to_owned())
                .collect(),
            revision,
            after: None,
            limit: 100,
        };
        let Output::ModelCatalog(page) = client
            .execute(client.prepare(Command::ReadModelCatalog(query.clone())))
            .await?
        else {
            return Err(Fault::new(ErrorCode::Internal, "catalog page expected"));
        };
        for model in page.models {
            if remaining.remove(model.id.as_str()) {
                result.insert(model.id.clone(), model);
            }
        }
        for id in &query.ids {
            if page.next.as_ref().is_none_or(|next| id <= next) {
                remaining.remove(id.as_str());
            }
        }
        if remaining.is_empty() {
            return Ok(result);
        }
        revision = Some(page.revision);
    }
    Err(Fault::new(
        ErrorCode::Unavailable,
        "catalog lookup exceeded its page limit",
    ))
}
