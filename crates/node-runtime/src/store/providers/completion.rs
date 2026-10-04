//! Fill missing automatic model choices from the Node's cached reference catalog.
use super::super::database::storage_error;
use rusqlite::{Connection, params};
use sailry_protocol::{
    Effort, Event, EventEnvelope, Fault, NodeId,
    conversation::{Provider, catalog},
};

pub(in crate::store) fn fill(db: &Connection, provider: &mut Provider) -> Result<(), Fault> {
    crate::providers::search::enable(provider);
    if super::catalog::status(db)?.revision == 0 {
        return Ok(());
    }
    for models in provider.models.chunks_mut(100) {
        let ids = models
            .iter()
            .filter(|model| {
                model.reasoning
                    && !model.custom_efforts
                    && model
                        .efforts
                        .iter()
                        .all(|effort| *effort == Effort::Default)
            })
            .map(|model| model.id.clone())
            .collect::<Vec<_>>();
        if ids.is_empty() {
            continue;
        }
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
        loop {
            let page = super::catalog::page(db, &query)?;
            references.extend(page.models);
            query.revision = Some(page.revision);
            query.after = page.next;
            if query.after.is_none() {
                break;
            }
        }
        for model in models {
            if !model.reasoning
                || model.custom_efforts
                || model
                    .efforts
                    .iter()
                    .any(|effort| *effort != Effort::Default)
            {
                continue;
            }
            let Some(reference) = references.iter().find(|reference| reference.id == model.id)
            else {
                continue;
            };
            let efforts = reference.efforts(provider.api, model.output);
            if !efforts.iter().any(|effort| *effort != Effort::Default) {
                continue;
            }
            model.default_effort = Effort::initial(&efforts, model.default_effort);
            model.efforts = efforts;
        }
    }
    Ok(())
}

pub(super) fn refresh(db: &Connection, node: NodeId) -> Result<Vec<EventEnvelope>, Fault> {
    let mut events = Vec::new();
    for original in super::super::agent::providers::list(db)? {
        let mut provider = original.clone();
        fill(db, &mut provider)?;
        if provider == original {
            continue;
        }
        let provider = super::super::agent::providers::put(db, node, &provider, original.revision)?;
        let event = Event::ProviderChanged(provider);
        db.execute(
            "INSERT INTO events(body) VALUES(?1)",
            params![super::super::database::encode(&event)?],
        )
        .map_err(storage_error)?;
        events.push(EventEnvelope {
            node,
            cursor: db.last_insert_rowid() as u64,
            event,
        });
    }
    Ok(events)
}
