//! Preserve visible unfinished output through the canonical writer before ending a run.
use super::*;

pub(in crate::store::agent) async fn retain(
    database: &mut Database,
    run: &Run,
    events: &broadcast::Sender<EventEnvelope>,
) -> Result<(), Fault> {
    for draft in database.feeds.seal(run.session, run.turn) {
        let content: Vec<_> = draft
            .parts
            .into_iter()
            .filter_map(|part| {
                let kind = match part {
                    Part::Text(text) => BlockKind::Text { text },
                    Part::Thinking(text) => BlockKind::Thinking {
                        text,
                        signature: None,
                    },
                    _ => return None,
                };
                Some(kind)
            })
            .enumerate()
            .map(|(index, kind)| records::Block { index, kind })
            .collect();
        let part_count = content.len();
        if content.is_empty() {
            continue;
        }
        let mut event = Record::new(
            run.turn,
            Kind::Message(records::Message {
                role: records::Role::Assistant,
                part_count,
                content,
                finish: Some(records::Finish::Interrupted),
                ..Default::default()
            }),
        );
        event.id = draft.id;
        event.provenance.author = draft.author;
        event.provenance.branch = draft.branch;
        append(database, run.session, run.turn, event, events)
            .await
            .map_err(|error| {
                Fault::new(
                    ErrorCode::Unavailable,
                    format!("interrupted output could not be saved: {error}"),
                )
            })?;
    }
    Ok(())
}

/// A user-stopped turn keeps its admitted input in history at its own
/// position, even when inference never consumed it. Repeated settlement finds
/// the committed user entry and writes nothing.
pub(in crate::store::agent) async fn input(
    database: &mut Database,
    run: &Run,
    events: &broadcast::Sender<EventEnvelope>,
) -> Result<(), Fault> {
    if run.kind != RunKind::Task || run.continuation.is_some() || run.started_ms.is_none() {
        return Ok(());
    }
    let body: Option<Vec<u8>> = {
        let mut statement = database
            .connection
            .prepare("SELECT input FROM agent_runs WHERE turn=?1 AND input IS NOT NULL AND NOT EXISTS(SELECT 1 FROM agent_events WHERE turn=?1 AND json_extract(body,'$.provenance.author')='user')")
            .await
            .map_err(storage_error)?;
        match statement.query_row([run.turn.to_string()]).await {
            Ok(row) => {
                let mapped: turso::Result<_> = row.get(0);
                Some(mapped.map_err(storage_error)?)
            }
            Err(turso::Error::QueryReturnedNoRows) => None,
            Err(error) => return Err(storage_error(error)),
        }
    };
    let Some(body) = body else {
        return Ok(());
    };
    let message: sailry_protocol::conversation::Input =
        serde_json::from_slice(&body).map_err(storage_error)?;
    let attachments =
        crate::store::attachments::turns::list(&database.connection, run.turn).await?;
    let record = crate::agent::inputs::stopped(run.turn, &message, &attachments)?;
    append(database, run.session, run.turn, record, events)
        .await
        .map(|_| ())
}
