//! Resolve automatic capabilities without replacing configured limits or choices.
use super::super::database::record;
use sailry_protocol::{
    Effort, Event, EventEnvelope, Fault, NodeId,
    conversation::{Model, Provider, catalog, discovery},
};
use turso::Connection;

pub(in crate::store) async fn fill(db: &Connection, provider: &mut Provider) -> Result<(), Fault> {
    crate::providers::search::enable(provider);
    let cached = super::catalog::status(db).await?.revision != 0;
    for models in provider.models.chunks_mut(100) {
        let ids = models
            .iter()
            .map(|model| model.id.clone())
            .collect::<Vec<_>>();
        let mut query = catalog::Query {
            provider: match provider.endpoint.trim_end_matches('/') {
                "https://api.openai.com/v1" => Some("openai"),
                "https://api.anthropic.com" => Some("anthropic"),
                "https://generativelanguage.googleapis.com/v1beta" => Some("google"),
                "https://api.deepseek.com" | "https://api.deepseek.com/v1" => Some("deepseek"),
                _ => None,
            }
            .map(str::to_owned),
            ids,
            revision: None,
            after: None,
            limit: 100,
        };
        let mut references = Vec::new();
        if cached {
            loop {
                let page = super::catalog::page(db, &query).await?;
                references.extend(page.models);
                query.revision = Some(page.revision);
                query.after = page.next;
                if query.after.is_none() {
                    break;
                }
            }
        }
        for model in models {
            let reference = references.iter().find(|reference| reference.id == model.id);
            resolve(model, reference, provider.api, provider.authentication);
        }
    }
    Ok(())
}

fn resolve(
    model: &mut Model,
    reference: Option<&catalog::Model>,
    api: sailry_protocol::conversation::ModelApi,
    authentication: sailry_protocol::Authentication,
) {
    let native = model.capabilities.as_ref();
    let reasoning = native
        .and_then(|value| value.reasoning)
        .or(reference.and_then(|value| value.reasoning))
        .unwrap_or_else(|| {
            native
                .and_then(|value| value.efforts.as_ref())
                .is_some_and(|efforts| efforts.iter().any(|effort| *effort != Effort::Default))
                || model
                    .efforts
                    .iter()
                    .any(|effort| *effort != Effort::Default)
        });
    let refresh_efforts = !model.custom_efforts
        && model
            .efforts
            .iter()
            .all(|effort| *effort == Effort::Default);
    let mut completed = discovery::Model {
        id: model.id.clone(),
        display_name: model.display_name.clone(),
        context: Some(model.context),
        output: Some(model.output),
        capabilities: model.capabilities.clone(),
    }
    .configuration(reference, api, authentication);
    completed.reasoning = reasoning;
    completed.web_search = native
        .and_then(|value| value.web_search)
        .unwrap_or(model.web_search);
    if !refresh_efforts || completed.efforts.is_empty() {
        completed.efforts = model.efforts.clone();
        completed.default_effort = model.default_effort;
    }
    completed.custom_efforts = model.custom_efforts;
    completed.compaction_tokens = model.compaction_tokens;
    *model = completed;
}

#[cfg(test)]
mod tests;

pub(super) async fn refresh(db: &Connection, node: NodeId) -> Result<Vec<EventEnvelope>, Fault> {
    let mut changes = Vec::new();
    for original in super::super::agent::providers::list(db).await? {
        let mut provider = original.clone();
        fill(db, &mut provider).await?;
        if provider == original {
            continue;
        }
        let provider =
            super::super::agent::providers::put(db, node, &provider, original.revision).await?;
        changes.push(Event::ProviderChanged(provider));
    }
    record(db, node, changes).await
}
