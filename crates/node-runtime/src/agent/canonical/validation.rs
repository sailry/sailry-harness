//! Validate Sailry's storage references, without adding rules to the model loop.
use super::*;
use sailry_protocol::Fault;
use std::collections::BTreeSet;

pub(crate) fn validate(record: &Record) -> Result<(), Fault> {
    if record.id.is_empty() {
        return Err(Fault::invalid("conversation record identifier is invalid"));
    }
    let message = match &record.kind {
        Kind::Message(message) => Some(message),
        Kind::Compaction(compaction) => {
            if compaction.boundary.is_empty() {
                return Err(Fault::invalid("context summary has no boundary"));
            }
            Some(&compaction.summary)
        }
        _ => None,
    };
    if let Some(message) = message {
        let mut slots = BTreeSet::new();
        for block in &message.content {
            if block.index >= message.part_count {
                return Err(Fault::invalid(
                    "conversation content slot is outside its message",
                ));
            }
            if !slots.insert(block.index) {
                return Err(Fault::invalid("conversation content slots are duplicated"));
            }
            match &block.kind {
                BlockKind::ToolResponse(response) => {
                    if response.revision.len() != 64 {
                        return Err(Fault::invalid("tool response is invalid"));
                    }
                }
                _ => {}
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record(index: usize, part_count: usize) -> Record {
        Record {
            id: "record".into(),
            created: Utc::now(),
            turn: TurnId::new(),
            provenance: Provenance::default(),
            metadata: Metadata::default(),
            kind: Kind::Message(Message {
                part_count,
                content: vec![Block {
                    index,
                    kind: BlockKind::Text {
                        text: "Answer".into(),
                    },
                }],
                ..Message::default()
            }),
        }
    }

    #[test]
    fn permits_omitted_slots() {
        assert!(validate(&record(2, 5)).is_ok());
    }

    #[test]
    fn rejects_out_of_bounds_slots() {
        assert!(validate(&record(0, 0)).is_err());
        assert!(validate(&record(2, 2)).is_err());
    }
}
