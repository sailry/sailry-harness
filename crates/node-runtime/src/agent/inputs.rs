//! Input records retain their admitted turn ownership through the shared writer.
use super::*;
use canonical::{Block, BlockKind, Kind, Record};
use sailry_protocol::conversation::{QueuedMessage, reference::Reference};
use std::collections::{BTreeMap, BTreeSet};

pub(super) struct Batch {
    entries: Vec<Entry>,
}
#[derive(Clone)]
struct Entry {
    turn: TurnId,
    references: Vec<Reference>,
    record: Record,
    capability: Option<usize>,
}
impl Batch {
    pub async fn select(
        &mut self,
        invocation: &Invocation,
        resources: &crate::plugins::resources::Resources,
        tools: &BTreeMap<String, Vec<String>>,
    ) -> Result<(), Fault> {
        for entry in &mut self.entries {
            let instruction = references::capabilities::instruction(
                invocation,
                entry.references.iter(),
                resources,
                tools,
            )
            .await?;
            if !instruction.is_empty()
                && let Kind::Message(message) = &mut entry.record.kind
            {
                entry.capability = Some(message.part_count);
                message.content.push(Block {
                    index: message.part_count,
                    kind: BlockKind::Text { text: instruction },
                });
                message.part_count += 1;
            }
        }
        Ok(())
    }
    pub async fn capture(ingress: &Ingress, invocation: &Invocation) -> Result<Self, Fault> {
        let mut entries = vec![Entry {
            turn: invocation.input_turn.unwrap_or(invocation.turn.id),
            references: invocation.message.references.clone(),
            record: attachments::input(ingress, invocation).await?,
            capability: None,
        }];
        for input in &invocation.followups {
            entries.push(Entry {
                turn: input.turn.id,
                references: input.message.references.clone(),
                record: attachments::message(ingress, invocation.turn.id, &input.message, None)
                    .await?,
                capability: None,
            });
        }
        if let Some(entry) = entries.first_mut() {
            if invocation.notice_input.is_some() {
                entry.record.metadata.input = Some(canonical::Input {
                    notice_part: Some(0),
                    notice_ids: invocation
                        .notices
                        .iter()
                        .map(|notice| notice.id.clone())
                        .collect(),
                    ..Default::default()
                });
            }
        }
        Ok(Self { entries })
    }
    pub async fn guidance(
        ingress: &Ingress,
        owner: TurnId,
        inputs: &[QueuedMessage],
    ) -> Result<Self, Fault> {
        let mut entries = Vec::new();
        for input in inputs {
            entries.push(Entry {
                turn: input.turn.id,
                references: input.message.references.clone(),
                record: attachments::message(ingress, owner, &input.message, None).await?,
                capability: None,
            });
        }
        Ok(Self { entries })
    }
    pub fn records(&self, source: &str, steer: bool) -> Vec<Record> {
        self.entries
            .iter()
            .map(|entry| {
                let mut record = entry.record.clone();
                record.turn = entry.turn;
                record.metadata.runtime = Some(canonical::Runtime {
                    steer,
                    ..canonical::Runtime::default()
                });
                let input = record.metadata.input.get_or_insert_with(Default::default);
                input.turn = Some(entry.turn);
                input.source = Some(match source {
                    "parent" => canonical::InputSource::Parent,
                    "notice" => canonical::InputSource::Notice,
                    "plugin" => canonical::InputSource::Plugin,
                    _ => canonical::InputSource::User,
                });
                input.references = entry.references.clone();
                input.capability_part = entry.capability;
                let index = usize::from(!entry.references.is_empty())
                    + usize::from(input.notice_part.is_some());
                input.text_part = match &record.kind {
                    Kind::Message(message) => message
                        .content
                        .iter()
                        .find(|block| {
                            block.index == index && matches!(block.kind, BlockKind::Text { .. })
                        })
                        .map(|_| index),
                    _ => None,
                };
                record
            })
            .collect()
    }
    pub async fn append(
        &self,
        ingress: &Arc<Ingress>,
        target: SessionId,
        owner: TurnId,
        source: &'static str,
        steer: bool,
    ) -> Result<(), Fault> {
        let current = ingress
            .sessions(owner)
            .get(crate::store::agent::SessionQuery {
                session: target,
                recent: None,
                after: None,
            })
            .await?;
        let present: BTreeSet<_> = current
            .records
            .iter()
            .filter_map(|record| record.metadata.input.as_ref()?.turn)
            .collect();
        for record in self.records(source, steer) {
            if present.contains(&record.turn) {
                continue;
            }
            crate::store::agent::Sessions::new(ingress.clone(), owner)
                .with_input_turn(record.turn)
                .append_event(&target.to_string(), record)
                .await?;
        }
        Ok(())
    }
}
