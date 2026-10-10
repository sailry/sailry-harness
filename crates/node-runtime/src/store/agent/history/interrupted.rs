//! Preserve visible unfinished output through the canonical event writer before ending a run.
use super::*;

pub(in crate::store::agent) fn retain(
    database: &mut Database,
    run: &Run,
    events: &broadcast::Sender<EventEnvelope>,
) -> Result<(), Fault> {
    for draft in database.feeds.seal(run.session, run.turn) {
        let parts: Vec<_> = draft
            .parts
            .into_iter()
            .filter_map(|part| match part {
                Part::Text(text) => Some(adk_core::Part::Text { text }),
                Part::Thinking(thinking) => Some(adk_core::Part::Thinking {
                    thinking,
                    signature: None,
                }),
                _ => None,
            })
            .collect();
        if parts.is_empty() {
            continue;
        }
        let mut event = AdkEvent::new(run.turn.to_string());
        event.id = draft.id;
        event.author = draft.author;
        event.branch = draft.branch;
        event.llm_response.interrupted = true;
        event.set_content(adk_core::Content {
            role: "model".into(),
            parts,
        });
        append(database, run.session, run.turn, event, events).map_err(|error| {
            Fault::new(
                ErrorCode::Unavailable,
                format!("interrupted output could not be saved: {error}"),
            )
        })?;
    }
    Ok(())
}
