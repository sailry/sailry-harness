//! Preserve visible unfinished output through the canonical writer before ending a run.
use super::*;

pub(in crate::store::agent) async fn retain(
    database: &mut Database,
    run: &Run,
    events: &broadcast::Sender<EventEnvelope>,
) -> Result<(), Fault> {
    for draft in database.feeds.seal(run.session, run.turn) {
        let mut content: Vec<_> = draft
            .parts
            .into_iter()
            .enumerate()
            .filter_map(|(index, part)| {
                let kind = match part {
                    Part::Text(text) if draft.commentary.contains_key(&index) => {
                        BlockKind::ServerCall {
                            value: serde_json::json!({
                                "type": "message", "role": "assistant",
                                "id": draft.commentary[&index], "phase": "commentary",
                                "status": "incomplete",
                                "content": [{"type": "output_text", "text": text, "annotations": []}],
                            }).into(),
                        }
                    }
                    Part::Text(text) => BlockKind::Text { text },
                    Part::Thinking(text) => BlockKind::Thinking { text, signature: None },
                    _ => return None,
                };
                Some(records::Block { index, kind })
            })
            .collect();
        for (index, block) in content.iter_mut().enumerate() {
            block.index = index;
        }
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
