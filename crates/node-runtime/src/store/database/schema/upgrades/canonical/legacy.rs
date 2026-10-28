//! Startup-only conversion of the former Sailry event format.
//! Runtime readers and writers use canonical records exclusively.
use crate::{agent::canonical::*, store::agent::blobs, store::database::storage_error};
use sailry_protocol::{Fault, TurnId};
use serde_json::{Value, json};
use turso::Connection;
use wire::{Content, Event, Part};

mod content;
mod metadata;
mod native;
mod response;
mod wire;

pub(super) async fn capture(db: &Connection, turn: TurnId, body: &[u8]) -> Result<Record, Fault> {
    let event: Event = serde_json::from_slice(body).map_err(storage_error)?;
    let mut metadata = metadata::capture(&event)?;
    let kind = if let Some(summary) = &event.actions.compaction {
        let mut content = summary.compacted_content.clone();
        // Historical summaries may start with one owned reasoning update. It is
        // execution configuration, so the canonical summary retains only text.
        if matches!(content.parts.first(), Some(Part::ProviderData { provider_data })
            if provider_data.origin.is_some()
                && matches!(provider_data.parts.as_ref(), [Part::ConfigurationUpdate { .. }]))
        {
            content.parts.remove(0);
        }
        if content.parts.is_empty()
            || content
                .parts
                .iter()
                .any(|part| !matches!(part, Part::Text { .. }))
        {
            return Err(Fault::invalid("context summary must contain only text"));
        }
        let boundary = event
            .metadata
            .get("sailry.context.end_event")
            .cloned()
            .ok_or_else(|| Fault::invalid("context summary has no boundary"))?;
        let mut message = response::capture(&event.response);
        message.role = Role::User;
        message.part_count = content.parts.len();
        message.content = content::capture(db, &content).await?;
        let files = event
            .metadata
            .get("sailry.compaction.files")
            .map(|encoded| {
                serde_json::from_str::<std::collections::BTreeMap<String, bool>>(encoded)
                    .map_err(storage_error)
            })
            .transpose()?
            .unwrap_or_default()
            .into_iter()
            .map(|(path, modified)| CompactedFile {
                path,
                modified: Some(modified),
                summary: None,
            })
            .collect();
        Kind::Compaction(Compaction {
            boundary,
            summary: message,
            start: summary.start_timestamp,
            end: summary.end_timestamp,
            files,
        })
    } else if let Some(notice) = notice(&event)? {
        if event.content().is_some()
            && matches!(
                &notice,
                Notice::PlanReview { .. } | Notice::Elicitation { .. }
            )
        {
            return Err(Fault::invalid(
                "input notice must not contain model content",
            ));
        }
        Kind::Notice(notice)
    } else if event.content().is_none()
        && metadata
            .context
            .as_ref()
            .is_some_and(|context| context.captured)
    {
        Kind::Configuration(Configuration {
            context: metadata.context.take().unwrap(),
        })
    } else {
        let mut message = response::capture(&event.response);
        if let Some(value) = event.content() {
            message.content = content::capture(db, value).await?;
            content::tool_metadata(&mut message.content, event)?;
        }
        Kind::Message(message)
    };
    Ok(Record {
        id: event.id.clone(),
        created: event.timestamp,
        turn,
        provenance: Provenance {
            author: event.author.clone(),
            branch: event.branch.clone(),
        },
        metadata,
        kind,
    })
}

fn notice(event: &Event) -> Result<Option<Notice>, Fault> {
    let fields = &event.metadata;
    if let Some(source) = fields.get("sailry.plan.review") {
        return Ok(Some(Notice::PlanReview {
            source: source.clone(),
        }));
    }
    if let Some(encoded) = fields.get("sailry.plan.review_result") {
        let value: Value = serde_json::from_str(encoded).map_err(storage_error)?;
        return Ok(Some(Notice::PlanResult {
            call: value["call"]
                .as_str()
                .ok_or_else(|| Fault::invalid("plan result has no call"))?
                .into(),
            response: serde_json::from_value(value["response"].clone()).map_err(storage_error)?,
        }));
    }
    if let Some(encoded) = fields.get("sailry.mcp.elicitation") {
        let value: Value = serde_json::from_str(encoded).map_err(storage_error)?;
        let parent: wire::ToolIdentity =
            serde_json::from_value(value["parent"].clone()).map_err(storage_error)?;
        return Ok(Some(Notice::Elicitation {
            parent: ToolIdentity {
                id: parent.function_call_id,
                name: parent.tool_name,
                arguments: parent.args,
            },
            request: value["request"].clone(),
            spec: serde_json::from_value(value["spec"].clone()).map_err(storage_error)?,
            schema: value["schema"].clone(),
        }));
    }
    if let Some(encoded) = fields.get("sailry.mcp.elicitation_result") {
        let value: Value = serde_json::from_str(encoded).map_err(storage_error)?;
        return Ok(Some(Notice::ElicitationResult {
            call: value["id"]
                .as_str()
                .ok_or_else(|| Fault::invalid("elicitation result has no call"))?
                .into(),
            response: serde_json::from_value(value["response"].clone()).map_err(storage_error)?,
        }));
    }
    Ok(None)
}

#[cfg(test)]
mod tests;
