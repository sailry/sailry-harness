//! Temporary typed boundary while ADK still executes against Sailry-owned records.
//! Stored records are always canonical; legacy JSON decoding belongs only to startup.
use super::canonical::*;
use crate::{store::agent::blobs, store::database::storage_error};
use adk_core::{Content, Event};
use sailry_protocol::{Fault, TurnId};
use serde_json::{Value, json};
use turso::Connection;

mod content;
mod metadata;
mod native;
mod response;
pub(crate) use metadata::state;

pub(crate) async fn from_adk(
    db: &Connection,
    turn: TurnId,
    event: &Event,
) -> Result<Record, Fault> {
    let mut metadata = metadata::capture(event)?;
    let kind = if let Some(summary) = &event.actions.compaction {
        let mut content = summary.compacted_content.clone();
        // Historical summaries may start with one owned reasoning update. It is
        // execution configuration, so the canonical summary retains only text.
        if matches!(content.parts.first(), Some(adk_core::Part::ProviderData { provider_data })
            if provider_data.origin.is_some()
                && matches!(provider_data.parts.as_ref(), [adk_core::Part::ConfigurationUpdate { .. }]))
        {
            content.parts.remove(0);
        }
        if content.parts.is_empty()
            || content
                .parts
                .iter()
                .any(|part| !matches!(part, adk_core::Part::Text { .. }))
        {
            return Err(Fault::invalid("context summary must contain only text"));
        }
        let boundary = event
            .provider_metadata
            .get("sailry.context.end_event")
            .cloned()
            .ok_or_else(|| Fault::invalid("context summary has no boundary"))?;
        let mut message = response::capture(&event.llm_response);
        message.role = Role::User;
        message.part_count = content.parts.len();
        message.content = content::capture(db, &content).await?;
        let files = event
            .provider_metadata
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
    } else if let Some(notice) = notice(event)? {
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
        let mut message = response::capture(&event.llm_response);
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

pub(crate) async fn to_adk(db: &Connection, record: &Record) -> Result<Event, Fault> {
    let mut event = Event::new(record.turn.to_string());
    event.id = record.id.clone();
    event.timestamp = record.created;
    event.author = record.provenance.author.clone();
    event.branch = record.provenance.branch.clone();
    metadata::restore(&mut event, &record.metadata, record.context())?;
    match &record.kind {
        Kind::Message(message) => {
            response::restore(&mut event.llm_response, message, &record.metadata)?;
            if !message.content.is_empty() {
                event.set_content(content::restore(db, message, &record.provenance).await?);
            }
        }
        Kind::Compaction(summary) => {
            response::restore(&mut event.llm_response, &summary.summary, &record.metadata)?;
            event.actions.compaction = Some(adk_core::EventCompaction {
                start_timestamp: summary.start,
                end_timestamp: summary.end,
                compacted_content: content::restore(db, &summary.summary, &record.provenance)
                    .await?,
            });
            event
                .provider_metadata
                .insert("sailry.context.end_event".into(), summary.boundary.clone());
            if !summary.files.is_empty() {
                let files: std::collections::BTreeMap<_, _> = summary
                    .files
                    .iter()
                    .filter_map(|file| file.modified.map(|modified| (&file.path, modified)))
                    .collect();
                event.provider_metadata.insert(
                    "sailry.compaction.files".into(),
                    serde_json::to_string(&files).map_err(storage_error)?,
                );
            }
        }
        Kind::Configuration(_) => {}
        Kind::Notice(notice) => restore_notice(&mut event, notice)?,
        Kind::Operation(Operation::Visibility { .. }) => {}
    }
    Ok(event)
}

fn notice(event: &Event) -> Result<Option<Notice>, Fault> {
    let fields = &event.provider_metadata;
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
        let parent: adk_core::ToolConfirmationRequest =
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

fn restore_notice(event: &mut Event, notice: &Notice) -> Result<(), Fault> {
    let (key, value) = match notice {
        Notice::PlanReview { source } => {
            event
                .provider_metadata
                .insert("sailry.plan.review".into(), source.clone());
            return Ok(());
        }
        Notice::PlanResult { call, response } => (
            "sailry.plan.review_result",
            json!({"call":call,"response":response}),
        ),
        Notice::Elicitation {
            parent,
            request,
            spec,
            schema,
        } => {
            let mut identity =
                adk_core::ToolConfirmationRequest::new(&parent.name, parent.arguments.clone());
            identity.function_call_id = parent.id.clone();
            (
                "sailry.mcp.elicitation",
                json!({"parent":identity,"request":request,"spec":spec,"schema":schema}),
            )
        }
        Notice::ElicitationResult { call, response } => (
            "sailry.mcp.elicitation_result",
            json!({"id":call,"response":response}),
        ),
    };
    event.provider_metadata.insert(
        key.into(),
        serde_json::to_string(&value).map_err(storage_error)?,
    );
    Ok(())
}

#[cfg(test)]
mod tests;
