//! Shared content selection; controllers retain layout and Markdown decoration.
use super::tools::{Call, Reference};
use sailry_protocol::{
    TurnId,
    conversation::{Entry, Image, Part, Snapshot, recovery::Retry},
};
use serde::{Serialize, Serializer, ser::SerializeStruct};
use std::{borrow::Cow, collections::BTreeMap};

pub struct Row<'a> {
    pub source: Reference,
    pub draft: bool,
    pub end: usize,
    pub part: &'a Part,
    pub image: Option<&'a Image>,
    pub text: Option<String>,
    pub text_offset: usize,
    pub text_last: bool,
    pub continuation: bool,
    pub commentary: bool,
    pub entry: Option<&'a Entry>,
}

impl Serialize for Row<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let part = match (self.image, &self.text, self.part) {
            (Some(image), _, _) => Cow::Owned(Part::Image(image.clone())),
            (_, Some(text), Part::Text(_)) => Cow::Owned(Part::Text(text.clone())),
            (_, Some(text), Part::Thinking(_)) => Cow::Owned(Part::Thinking(text.clone())),
            _ => Cow::Borrowed(self.part),
        };
        let mut row = serializer.serialize_struct("Row", 9)?;
        row.serialize_field("source", &self.source)?;
        row.serialize_field("draft", &self.draft)?;
        row.serialize_field("end", &self.end)?;
        row.serialize_field("part", &part)?;
        row.serialize_field("text_offset", &self.text_offset)?;
        row.serialize_field("text_last", &self.text_last)?;
        row.serialize_field("continuation", &self.continuation)?;
        row.serialize_field("commentary", &self.commentary)?;
        row.serialize_field(
            "work",
            &super::reply::is_work(part.as_ref(), self.commentary),
        )?;
        row.end()
    }
}

pub fn collect<'a>(snapshot: &'a Snapshot, calls: &[Call], turn: TurnId) -> Vec<Row<'a>> {
    let page = &snapshot.page;
    let calls: BTreeMap<_, _> = calls
        .iter()
        .filter(|call| call.turn == turn)
        .map(|call| ((call.source.entry.as_str(), call.source.index), call))
        .collect();
    let entries = page
        .entries
        .iter()
        .filter(|entry| entry.turn == turn && entry.author != "user")
        .map(|entry| {
            (
                entry.id.as_str(),
                entry.parts.as_slice(),
                &entry.commentary,
                Some(entry),
            )
        })
        .chain(
            snapshot
                .drafts
                .iter()
                .filter(|draft| draft.turn == turn && draft.author != "user")
                .map(|draft| {
                    (
                        draft.id.as_str(),
                        draft.parts.as_slice(),
                        &draft.commentary,
                        None,
                    )
                }),
        );
    let mut rows: Vec<Row<'a>> = Vec::new();
    for (id, parts, commentary, entry) in entries {
        let mut index = 0;
        let mut offset = 0;
        let last = parts.iter().enumerate().rposition(|(index, part)| {
            matches!(part, Part::Text(_)) && !commentary.contains_key(&index)
        });
        while index < parts.len() {
            let start = index;
            let part = &parts[index];
            let call = calls.get(&(id, index));
            index += 1;
            let mut row = Row {
                source: Reference {
                    entry: id.into(),
                    index: start,
                },
                draft: entry.is_none(),
                commentary: commentary.contains_key(&start),
                end: index,
                part,
                image: None,
                text: None,
                text_offset: offset,
                text_last: false,
                continuation: start.checked_sub(1).is_some_and(|previous| {
                    matches!((&parts[previous], part), (Part::Image(_), Part::Image(_)))
                }),
                entry,
            };
            if let Part::Text(text) | Part::Thinking(text) = part {
                let mut joined = text.clone();
                while index < parts.len() {
                    if commentary.get(&start) != commentary.get(&index) {
                        break;
                    }
                    match (part, &parts[index]) {
                        (Part::Text(_), Part::Text(next))
                        | (Part::Thinking(_), Part::Thinking(next)) => joined.push_str(next),
                        _ => break,
                    }
                    index += 1;
                }
                row.end = index;
                row.text_last = !commentary.contains_key(&start)
                    && last.is_some_and(|last| last >= start && last < index);
                if matches!(part, Part::Text(_)) {
                    if !commentary.contains_key(&start) {
                        offset += joined.chars().count();
                    }
                    if joined.trim().is_empty() {
                        continue;
                    }
                }
                row.text = Some(joined);
            }
            if let Part::ToolResult { images, .. } = part {
                for image in images {
                    rows.push(Row {
                        source: row.source.clone(),
                        draft: row.draft,
                        commentary: row.commentary,
                        end: row.end,
                        part,
                        image: Some(image),
                        text: None,
                        text_offset: offset,
                        text_last: false,
                        continuation: false,
                        entry,
                    });
                }
                if call.is_none() {
                    continue;
                }
            }
            if matches!(part, Part::ToolCall { .. } | Part::ToolResult { .. }) {
                let Some(call) = call else {
                    continue;
                };
                if call.hidden(page) {
                    continue;
                }
                if call.name == "compact_context"
                    && let Some(summary) = call
                        .summary(page)
                        .and_then(|summary| summary.summary_id.as_deref())
                    && page.entries.iter().any(|entry| {
                        entry.id == summary
                            && entry
                                .parts
                                .iter()
                                .any(|part| matches!(part, Part::Compaction(_)))
                    })
                {
                    continue;
                }
            }
            if let Part::Resource(value) = part {
                if let Some(retry) = Retry::from_resource(value) {
                    if let Some(previous) = rows.last_mut()
                        && let Part::Resource(value) = previous.part
                        && let Some(before) = Retry::from_resource(value)
                        && !before.fallback
                        && !retry.fallback
                        && before.limit == retry.limit
                        && before.transport == retry.transport
                        && retry.attempt > before.attempt
                    {
                        previous.part = part;
                        continue;
                    }
                } else if sailry_protocol::conversation::compaction::Failure::from_resource(value)
                    .is_none()
                {
                    continue;
                }
            }
            rows.push(row);
        }
    }
    rows
}

#[cfg(test)]
mod tests;
