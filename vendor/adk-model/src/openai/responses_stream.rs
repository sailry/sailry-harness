//! Responses stream text recovery, separate from transport and request policy.

use std::collections::{BTreeMap, BTreeSet, HashSet};

use adk_core::{Content, LlmResponse, Part};
use async_openai::types::responses::{
    MessagePhase, OutputContent, OutputItem, OutputMessageContent, Response, ResponseStreamEvent,
    SummaryPart,
};

use super::responses_convert;

#[derive(Clone, Copy, Eq, Ord, PartialEq, PartialOrd)]
enum TextKind {
    Answer,
    Refusal,
    Summary,
}

#[derive(Clone, Eq, Ord, PartialEq, PartialOrd)]
struct TextKey {
    output_index: u32,
    part_index: u32,
    item_id: String,
    kind: TextKind,
}

impl TextKey {
    fn new(item_id: &str, output_index: u32, part_index: u32, kind: TextKind) -> Self {
        Self { item_id: item_id.into(), output_index, part_index, kind }
    }
}

#[derive(Default)]
pub(super) struct StreamState {
    emitted: BTreeMap<TextKey, String>,
    finalized: BTreeMap<TextKey, String>,
    finalized_items: BTreeMap<u32, OutputItem>,
    commentary: HashSet<(u32, String)>,
    native_tools: bool,
    corrected: bool,
}

impl StreamState {
    pub(super) fn new(native_tools: bool) -> Self {
        Self { native_tools, ..Default::default() }
    }

    pub(super) fn partial(&mut self, event: &ResponseStreamEvent) -> Option<LlmResponse> {
        use ResponseStreamEvent::*;

        let mut parts = Vec::new();
        match event {
            ResponseOutputItemAdded(event) => {
                self.record_phase(event.output_index, &event.item);
                return None;
            }
            ResponseOutputTextDelta(event) => {
                let key = TextKey::new(
                    &event.item_id,
                    event.output_index,
                    event.content_index,
                    TextKind::Answer,
                );
                self.delta(key, &event.delta, &mut parts);
            }
            ResponseOutputTextDone(event) => {
                let key = TextKey::new(
                    &event.item_id,
                    event.output_index,
                    event.content_index,
                    TextKind::Answer,
                );
                self.done(key, &event.text, &mut parts);
            }
            ResponseRefusalDelta(event) => {
                let key = TextKey::new(
                    &event.item_id,
                    event.output_index,
                    event.content_index,
                    TextKind::Refusal,
                );
                self.delta(key, &event.delta, &mut parts);
            }
            ResponseRefusalDone(event) => {
                let key = TextKey::new(
                    &event.item_id,
                    event.output_index,
                    event.content_index,
                    TextKind::Refusal,
                );
                self.done(key, &event.refusal, &mut parts);
            }
            ResponseContentPartDone(event) => match &event.part {
                OutputContent::OutputText(text) => {
                    let key = TextKey::new(
                        &event.item_id,
                        event.output_index,
                        event.content_index,
                        TextKind::Answer,
                    );
                    self.done(key, &text.text, &mut parts);
                }
                OutputContent::Refusal(refusal) => {
                    let key = TextKey::new(
                        &event.item_id,
                        event.output_index,
                        event.content_index,
                        TextKind::Refusal,
                    );
                    self.done(key, &refusal.refusal, &mut parts);
                }
                OutputContent::ReasoningText(_) => return None,
            },
            ResponseReasoningSummaryTextDelta(event) => {
                let key = TextKey::new(
                    &event.item_id,
                    event.output_index,
                    event.summary_index,
                    TextKind::Summary,
                );
                self.delta(key, &event.delta, &mut parts);
            }
            ResponseReasoningSummaryTextDone(event) => {
                let key = TextKey::new(
                    &event.item_id,
                    event.output_index,
                    event.summary_index,
                    TextKind::Summary,
                );
                self.done(key, &event.text, &mut parts);
            }
            ResponseReasoningSummaryPartDone(event) => {
                let SummaryPart::SummaryText(text) = &event.part;
                let key = TextKey::new(
                    &event.item_id,
                    event.output_index,
                    event.summary_index,
                    TextKind::Summary,
                );
                self.done(key, &text.text, &mut parts);
            }
            ResponseOutputItemDone(event) => {
                self.finalized_items.insert(event.output_index, event.item.clone());
                self.record_phase(event.output_index, &event.item);
                let mut item = event.item.clone();
                self.trim_item(event.output_index, &mut item, &mut parts);
            }
            _ => return None,
        }
        Some(LlmResponse {
            content: (!parts.is_empty()).then_some(Content { role: "model".into(), parts }),
            partial: true,
            ..Default::default()
        })
    }

    pub(super) fn complete(&mut self, response: &Response) -> LlmResponse {
        let mut response = response.clone();
        let recovered = response.output.is_empty() && !self.finalized_items.is_empty();
        if recovered {
            // Codex can omit terminal output after emitting authoritative done items.
            // Recover the whole items, including calls and reasoning, not only text.
            response.output = self.finalized_items.values().cloned().collect();
        }
        let mut full = responses_convert::from_response(&response);
        let mut unseen = response.clone();
        let mut final_keys = BTreeSet::new();
        for (output_index, item) in unseen.output.iter_mut().enumerate() {
            final_keys.extend(Self::item_keys(output_index as u32, item));
            self.record_phase(output_index as u32, item);
            self.trim_item(output_index as u32, item, &mut Vec::new());
        }
        let omitted = response.output.is_empty();
        let removed = !omitted && self.emitted.keys().any(|key| !final_keys.contains(key));
        let unfinished = self.emitted.keys().any(|key| !self.finalized.contains_key(key));
        if self.corrected
            || recovered
            || removed
            || unfinished
            || self.native_tools
            || !self.commentary.is_empty()
        {
            // ADK replaces accumulated fragments with this authoritative snapshot.
            if omitted {
                full.content = Some(Content { role: "model".into(), parts: self.snapshot_parts() });
            } else if (self.corrected || removed || unfinished) && full.content.is_none() {
                full.content = Some(Content { role: "model".into(), parts: Vec::new() });
            }
            full.provider_metadata.get_or_insert_with(|| serde_json::json!({}))["content_complete"] =
                serde_json::json!(true);
        } else {
            // Keep metadata/citation offsets from the full response, not its suffixes.
            full.content = responses_convert::from_response(&unseen).content;
        }
        full
    }

    fn record_phase(&mut self, output_index: u32, item: &OutputItem) {
        if let OutputItem::Message(message) = item
            && message.phase == Some(MessagePhase::Commentary)
        {
            self.commentary.insert((output_index, message.id.clone()));
        }
    }

    fn delta(&mut self, key: TextKey, text: &str, parts: &mut Vec<Part>) {
        if self.emitted.iter().any(|(previous, text)| previous > &key && !text.is_empty()) {
            self.corrected = true;
        }
        self.emitted.entry(key.clone()).or_default().push_str(text);
        if let Some(part) = self.part(&key, text, "in_progress") {
            parts.push(part);
        }
    }

    fn done(&mut self, key: TextKey, text: &str, parts: &mut Vec<Part>) -> String {
        self.finalized.insert(key.clone(), text.into());
        let later_text =
            self.emitted.iter().any(|(previous, text)| previous > &key && !text.is_empty());
        let emitted = self.emitted.entry(key.clone()).or_default();
        let Some(suffix) = text.strip_prefix(emitted.as_str()) else {
            // A final payload can correct earlier text, but it is not another delta.
            self.corrected = true;
            return String::new();
        };
        let suffix = suffix.to_owned();
        if later_text && !suffix.is_empty() {
            // Inserting an earlier part requires replacement, not an appended suffix.
            self.corrected = true;
            return String::new();
        }
        emitted.push_str(&suffix);
        if let Some(part) = self.part(&key, &suffix, "in_progress") {
            parts.push(part);
        }
        suffix
    }

    fn snapshot_parts(&self) -> Vec<Part> {
        // An omitted output retains done payloads, not unfinished streamed deltas.
        let mut parts: Vec<Part> = Vec::new();
        let mut messages: BTreeMap<(u32, String), usize> = BTreeMap::new();
        for (key, text) in &self.finalized {
            let Some(part) = self.part(key, text, "completed") else {
                continue;
            };
            if let Part::ServerToolCall { server_tool_call } = &part {
                let identity = (key.output_index, key.item_id.clone());
                if let Some(index) = messages.get(&identity) {
                    let Part::ServerToolCall { server_tool_call: message } = &mut parts[*index]
                    else {
                        unreachable!("commentary messages use native parts");
                    };
                    message["content"]
                        .as_array_mut()
                        .expect("native message content is an array")
                        .extend(
                            server_tool_call["content"]
                                .as_array()
                                .expect("native message content is an array")
                                .iter()
                                .cloned(),
                        );
                    continue;
                }
                messages.insert(identity, parts.len());
            }
            parts.push(part);
        }
        parts
    }

    fn item_keys(output_index: u32, item: &OutputItem) -> Vec<TextKey> {
        match item {
            OutputItem::Message(message) => message
                .content
                .iter()
                .enumerate()
                .map(|(index, part)| {
                    let kind = match part {
                        OutputMessageContent::OutputText(_) => TextKind::Answer,
                        OutputMessageContent::Refusal(_) => TextKind::Refusal,
                    };
                    TextKey::new(&message.id, output_index, index as u32, kind)
                })
                .collect(),
            OutputItem::Reasoning(reasoning) => reasoning
                .summary
                .iter()
                .enumerate()
                .map(|(index, _)| {
                    TextKey::new(
                        reasoning.id.as_deref().unwrap_or_default(),
                        output_index,
                        index as u32,
                        TextKind::Summary,
                    )
                })
                .collect(),
            _ => Vec::new(),
        }
    }

    fn trim_item(&mut self, output_index: u32, item: &mut OutputItem, parts: &mut Vec<Part>) {
        match item {
            OutputItem::Message(message) => {
                for (part_index, content) in message.content.iter_mut().enumerate() {
                    let (kind, text) = match content {
                        OutputMessageContent::OutputText(text) => {
                            (TextKind::Answer, &mut text.text)
                        }
                        OutputMessageContent::Refusal(refusal) => {
                            (TextKind::Refusal, &mut refusal.refusal)
                        }
                    };
                    let key = TextKey::new(&message.id, output_index, part_index as u32, kind);
                    *text = self.done(key, text, parts);
                }
            }
            OutputItem::Reasoning(reasoning) => {
                let item_id = reasoning.id.as_deref().unwrap_or_default();
                for (part_index, summary) in reasoning.summary.iter_mut().enumerate() {
                    let SummaryPart::SummaryText(text) = summary;
                    let key =
                        TextKey::new(item_id, output_index, part_index as u32, TextKind::Summary);
                    text.text = self.done(key, &text.text, parts);
                }
            }
            _ => {}
        }
    }

    fn part(&self, key: &TextKey, text: &str, message_status: &str) -> Option<Part> {
        if text.is_empty() {
            return None;
        }
        if self.commentary.contains(&(key.output_index, key.item_id.clone())) {
            let content = if key.kind == TextKind::Refusal {
                serde_json::json!({"type": "refusal", "refusal": text})
            } else {
                serde_json::json!({"type": "output_text", "text": text, "annotations": []})
            };
            Some(Part::ServerToolCall {
                server_tool_call: serde_json::json!({
                    "type": "message", "role": "assistant", "id": key.item_id,
                    "phase": "commentary", "status": message_status, "content": [content]
                }),
            })
        } else if key.kind == TextKind::Summary {
            Some(Part::Thinking { thinking: text.into(), signature: None })
        } else {
            Some(Part::Text { text: text.into() })
        }
    }
}
