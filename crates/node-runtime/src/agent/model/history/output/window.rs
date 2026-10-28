//! Request-only budgets retain canonical results and their continuation references.
use super::*;
use crate::agent::{compaction::estimate, model::history::Window};
use adk_core::LlmRequest;
use std::collections::BTreeSet;
use std::sync::OnceLock;

/// Stored result revisions by call; references must match the canonical read path.
#[derive(Default)]
pub(in crate::agent) struct Revisions<'a> {
    values: BTreeMap<&'a str, Vec<(&'a Value, OnceLock<String>)>>,
}

impl Revisions<'_> {
    fn unique(&self, id: &str) -> Option<&str> {
        let values = self.values.get(id)?;
        let (value, cached) = values.first()?;
        let first = cached.get_or_init(|| hash(value));
        values
            .iter()
            .skip(1)
            .all(|(value, cached)| cached.get_or_init(|| hash(value)) == first)
            .then_some(first.as_str())
    }
}
pub(in crate::agent) type Masks = BTreeSet<(String, String)>;

pub(in crate::agent) fn protected(name: &str, value: &Value) -> bool {
    matches!(name, "load_skill" | "read_skill_resource" | "plan_review")
        || value.get("progress").is_some_and(|progress| {
            serde_json::from_value::<sailry_protocol::conversation::progress::Progress>(
                progress.clone(),
            )
            .is_ok()
        })
}

pub(in crate::agent) fn revisions(events: &[adk_core::Event]) -> Revisions<'_> {
    let mut revisions = Revisions::default();
    for part in events
        .iter()
        .filter_map(adk_core::Event::content)
        .flat_map(|content| &content.parts)
        .map(Part::provider_part)
    {
        match part {
            Part::FunctionResponse {
                id: Some(id),
                function_response,
                ..
            } if !id.is_empty() => {
                revisions
                    .values
                    .entry(id)
                    .or_default()
                    .push((&function_response.response, OnceLock::new()));
            }
            _ => {}
        }
    }
    revisions
}

fn hash(value: &Value) -> String {
    blake3::hash(value.to_string().as_bytes())
        .to_hex()
        .to_string()
}

fn source(value: &Value) -> Option<&Value> {
    value
        .get("sailry_output")
        .or_else(|| value.get("output"))
        .filter(|source| {
            source["call"].as_str().is_some_and(|id| !id.is_empty())
                && source["revision"].as_str().is_some_and(|revision| {
                    revision.len() == 64 && revision.bytes().all(|byte| byte.is_ascii_hexdigit())
                })
        })
}

fn encode(value: &Value) -> String {
    match value.as_object() {
        Some(fields) if fields.contains_key("sailry_output") => serde_json::to_string(
            &fields
                .iter()
                .filter(|(key, _)| key.as_str() != "sailry_output")
                .collect::<BTreeMap<_, _>>(),
        )
        .unwrap(),
        _ => value.to_string(),
    }
}

fn encoded_source(
    value: &Value,
    id: &str,
    revisions: Option<&Revisions<'_>>,
    encoded: &str,
) -> Value {
    if let Some(source) = source(value).filter(|source| source["call"].as_str() == Some(id)) {
        return source.clone();
    }
    if let Some(known) = revisions.and_then(|revisions| revisions.unique(id)) {
        return json!({"call":id,"revision":known});
    }
    json!({"call":id,"revision":blake3::hash(encoded.as_bytes()).to_hex().to_string()})
}

pub(in crate::agent) const PREPARED: &str = "sailry.output.prepared";

struct Slot {
    content: usize,
    part: usize,
    id: String,
    name: String,
    arguments: Option<(usize, usize)>,
    source: Option<Value>,
    encoded: String,
    tokens: u64,
    overhead: u64,
    projected: bool,
}

pub(in crate::agent) struct Prepared<'a> {
    slots: Vec<Slot>,
    revisions: Option<&'a Revisions<'a>>,
}

pub(in crate::agent) fn prepare<'a>(
    request: &mut LlmRequest,
    revisions: Option<&'a Revisions<'a>>,
) -> Prepared<'a> {
    #[cfg(test)]
    super::super::PREPARATIONS.with(|count| count.set(count.get() + 1));
    if let Some(config) = &mut request.config {
        config.extensions.remove(PREPARED);
    }
    Prepared::new(&mut request.contents, revisions)
}

impl<'a> Prepared<'a> {
    fn new(contents: &mut [Content], revisions: Option<&'a Revisions<'a>>) -> Self {
        let mut calls = BTreeMap::new();
        let mut slots = Vec::new();
        for (content, message) in contents.iter_mut().enumerate() {
            for (part, value) in message.parts.iter_mut().enumerate() {
                if let Part::FunctionCall {
                    id: Some(id), name, ..
                } = value.provider_part()
                {
                    calls.insert(id.clone(), (name.clone(), (content, part)));
                    continue;
                }
                let Part::FunctionResponse {
                    id: part_id,
                    function_response,
                    ..
                } = value
                else {
                    continue;
                };
                let known = source(&function_response.response).filter(|source| {
                    part_id
                        .as_deref()
                        .is_none_or(|id| source["call"].as_str() == Some(id))
                });
                let Some(id) = part_id
                    .as_deref()
                    .filter(|id| !id.is_empty())
                    .or_else(|| known.and_then(|source| source["call"].as_str()))
                    .map(str::to_owned)
                else {
                    continue;
                };
                let overhead = estimate::response(function_response, 0);
                let mut encoded = if known.is_some() {
                    encode(&function_response.response)
                } else {
                    function_response.response.to_string()
                };
                let mut canonical = known.cloned();
                let arguments = calls
                    .get(&id)
                    .filter(|(name, _)| name == &function_response.name)
                    .map(|(_, location)| *location);
                if function_response.name.starts_with("mcp_")
                    && let Some(deduplicated) =
                        super::super::mcp::deduplicate(&function_response.response)
                {
                    let source = canonical.get_or_insert_with(|| {
                        encoded_source(&function_response.response, &id, revisions, &encoded)
                    });
                    encoded = encode(&deduplicated);
                    function_response.response = deduplicated;
                    function_response.response["sailry_output"] = source.clone();
                }
                slots.push(Slot {
                    content,
                    part,
                    id,
                    name: function_response.name.clone(),
                    arguments,
                    source: canonical,
                    tokens: estimate::text(&encoded),
                    encoded,
                    overhead,
                    projected: false,
                });
            }
        }
        Self { slots, revisions }
    }

    pub(in crate::agent) fn tokens(&self, request: &LlmRequest) -> adk_core::Result<u64> {
        let mut slots = self.slots.iter().peekable();
        let mut tokens = estimate::envelope(request)?;
        for (content, message) in request.contents.iter().enumerate() {
            tokens += 4;
            for (part, value) in message.parts.iter().enumerate() {
                if let Some(slot) = slots.peek()
                    && (slot.content, slot.part) == (content, part)
                {
                    tokens += slot.tokens + slot.overhead;
                    slots.next();
                } else {
                    tokens += estimate::part(value);
                }
            }
        }
        Ok(tokens)
    }

    pub(in crate::agent) fn pending_tokens(&self, events: &[adk_core::Event]) -> u64 {
        let mut slots = BTreeMap::<(&str, &str), Vec<&Slot>>::new();
        for slot in &self.slots {
            slots.entry((&slot.id, &slot.name)).or_default().push(slot);
        }
        let mut tokens = 0;
        for content in events.iter().rev().filter_map(adk_core::Event::content) {
            tokens += 4;
            for part in content.parts.iter().rev() {
                let Part::FunctionResponse {
                    id: Some(id),
                    function_response,
                    ..
                } = part
                else {
                    tokens += estimate::part(part);
                    continue;
                };
                let candidates = slots.get_mut(&(id.as_str(), function_response.name.as_str()));
                let matched = candidates.and_then(|candidates| {
                    let index = if candidates.len() == 1 {
                        Some(0)
                    } else {
                        let revision = hash(&function_response.response);
                        candidates.iter().rposition(|slot| {
                            slot.source
                                .as_ref()
                                .and_then(|source| source["revision"].as_str())
                                .map_or_else(
                                    || {
                                        blake3::hash(slot.encoded.as_bytes()).to_hex().as_str()
                                            == revision
                                    },
                                    |known| known == revision,
                                )
                        })
                    }?;
                    Some(candidates.remove(index))
                });
                if let Some(slot) = matched {
                    tokens += slot.tokens + slot.overhead;
                } else {
                    let projected = function_response
                        .name
                        .starts_with("mcp_")
                        .then(|| super::super::mcp::deduplicate(&function_response.response))
                        .flatten();
                    tokens += estimate::response(
                        function_response,
                        estimate::text(
                            &projected
                                .as_ref()
                                .unwrap_or(&function_response.response)
                                .to_string(),
                        ),
                    );
                }
            }
        }
        tokens
    }

    fn value<'r>(slot: &Slot, request: &'r LlmRequest) -> &'r Value {
        let Part::FunctionResponse {
            function_response, ..
        } = &request.contents[slot.content].parts[slot.part]
        else {
            unreachable!()
        };
        &function_response.response
    }

    fn reference(slot: &mut Slot, value: &Value, revisions: Option<&Revisions<'_>>) -> Value {
        slot.source
            .get_or_insert_with(|| encoded_source(value, &slot.id, revisions, &slot.encoded))
            .clone()
    }

    fn replace(slot: &mut Slot, contents: &mut [Content], value: Value, encoded: String) {
        slot.encoded = encoded;
        slot.tokens = estimate::text(&slot.encoded);
        let Part::FunctionResponse {
            function_response, ..
        } = &mut contents[slot.content].parts[slot.part]
        else {
            unreachable!()
        };
        function_response.response = value;
    }

    pub(super) fn project(&mut self, contents: &mut [Content]) {
        for slot in &mut self.slots {
            if slot.projected {
                continue;
            }
            slot.projected = true;
            let Part::FunctionResponse {
                function_response, ..
            } = &contents[slot.content].parts[slot.part]
            else {
                unreachable!()
            };
            let original = &function_response.response;
            if !super::eligible(original, &slot.name, &slot.id) {
                continue;
            }
            let source = Self::reference(slot, original, self.revisions);
            let arguments = slot.arguments.and_then(|(content, part)| {
                match contents[content].parts[part].provider_part() {
                    Part::FunctionCall { args, .. } => Some(args),
                    _ => None,
                }
            });
            if let Some(mut candidate) =
                super::candidate(original, &slot.name, arguments, &source, &slot.encoded)
            {
                if let Some(context) = original.get("sailry_context") {
                    candidate["sailry_context"] = context.clone();
                }
                let encoded = candidate.to_string();
                if encoded.len() < slot.encoded.len() {
                    Self::replace(slot, contents, candidate, encoded);
                }
            }
        }
    }

    pub(in crate::agent) fn remask(&mut self, request: &mut LlmRequest, calls: &Masks) {
        if calls.is_empty() {
            return;
        }
        let ids = calls
            .iter()
            .map(|(id, _)| id.as_str())
            .collect::<BTreeSet<_>>();
        for slot in &mut self.slots {
            if !ids.contains(slot.id.as_str()) {
                continue;
            }
            let value = Self::value(slot, request);
            if !maskable(&slot.id, &slot.name, value) {
                continue;
            }
            let source = Self::reference(slot, value, self.revisions);
            if !calls.contains(&(
                slot.id.clone(),
                source["revision"].as_str().unwrap().to_owned(),
            )) {
                continue;
            }
            let replacement = super::clip(value, &source, &slot.encoded, 500, true);
            let encoded = replacement.to_string();
            Self::replace(slot, &mut request.contents, replacement, encoded);
            request.previous_response_id = None;
        }
    }

    pub(in crate::agent) fn mask(
        &mut self,
        request: &mut LlmRequest,
        protect: u64,
        minimum: u64,
    ) -> adk_core::Result<u64> {
        let mut recent = 0u64;
        let mut saved = 0u64;
        let mut replacements = Vec::new();
        for (index, slot) in self.slots.iter_mut().enumerate().rev() {
            let keep = recent < protect;
            recent = recent.saturating_add(slot.tokens);
            let value = Self::value(slot, request);
            if keep || !maskable(&slot.id, &slot.name, value) {
                continue;
            }
            let source = Self::reference(slot, value, self.revisions);
            let replacement = super::clip(value, &source, &slot.encoded, 500, true);
            let encoded = replacement.to_string();
            let reduction = slot.tokens.saturating_sub(estimate::text(&encoded));
            if reduction > 0 {
                saved += reduction;
                replacements.push((index, replacement, encoded));
            }
        }
        if saved < minimum {
            return Ok(0);
        }
        for (index, replacement, encoded) in replacements {
            Self::replace(
                &mut self.slots[index],
                &mut request.contents,
                replacement,
                encoded,
            );
        }
        if saved > 0 {
            request.previous_response_id = None;
        }
        Ok(saved)
    }

    pub(in crate::agent) fn limit(
        &mut self,
        request: &mut LlmRequest,
        window: Option<Window>,
    ) -> adk_core::Result<Option<String>> {
        let fingerprint = self.limit_unmarked(request, window)?;
        let stamp = boundary(request, window);
        request
            .config
            .get_or_insert_with(Default::default)
            .extensions
            .insert(PREPARED.into(), stamp);
        Ok(fingerprint)
    }

    pub(in crate::agent) fn limit_unmarked(
        &mut self,
        request: &mut LlmRequest,
        window: Option<Window>,
    ) -> adk_core::Result<Option<String>> {
        let empty_config = request.config.is_none();
        clear_prepared(request);
        request.config.get_or_insert_with(Default::default);
        self.project(&mut request.contents);
        let mut fingerprint = None;
        if let Some(window) = window {
            let capacity = window.capacity(request);
            let total = self.tokens(request)?;
            if total > capacity {
                let mut sizes = self
                    .slots
                    .iter()
                    .filter(|slot| !protected(&slot.name, Self::value(slot, request)))
                    .map(|slot| slot.tokens)
                    .collect::<Vec<_>>();
                let mut available =
                    capacity.saturating_sub(total.saturating_sub(sizes.iter().sum::<u64>()));
                sizes.sort_unstable();
                let mut share = 0;
                for (index, size) in sizes.iter().enumerate() {
                    share = available / (sizes.len() - index) as u64;
                    if *size > share {
                        break;
                    }
                    available -= size;
                }
                let budget = share.saturating_sub(128).saturating_mul(2) as usize;
                let mut replaced = blake3::Hasher::new();
                let mut changed = false;
                for slot in &mut self.slots {
                    let value = Self::value(slot, request);
                    if slot.tokens <= share || protected(&slot.name, value) {
                        continue;
                    }
                    let source = Self::reference(slot, value, self.revisions);
                    let replacement = super::clip(value, &source, &slot.encoded, budget, false);
                    let encoded = replacement.to_string();
                    if estimate::text(&encoded) < slot.tokens {
                        replaced.update(slot.id.as_bytes());
                        replaced.update(encoded.as_bytes());
                        Self::replace(slot, &mut request.contents, replacement, encoded);
                        request.previous_response_id = None;
                        changed = true;
                    }
                }
                fingerprint = changed.then(|| replaced.finalize().to_hex().to_string());
            }
            if self.tokens(request)? > capacity {
                if empty_config {
                    request.config = None;
                }
                return Err(adk_core::AdkError::new(
                    adk_core::ErrorComponent::Model,
                    adk_core::ErrorCategory::ResourceExhausted,
                    "model.sailry.context_overflow",
                    "retained context does not fit the captured model window",
                ));
            }
        }
        Ok(fingerprint)
    }
}

fn boundary(request: &LlmRequest, window: Option<Window>) -> Value {
    json!([request.model, window.map(|window| window.capacity(request))])
}

/// Only controlled request builders issue this marker, and rebuilding clears it.
pub(in crate::agent) fn take_prepared(request: &mut LlmRequest, window: Option<Window>) -> bool {
    let marker = request
        .config
        .as_mut()
        .and_then(|config| config.extensions.remove(PREPARED));
    marker.is_some_and(|marker| marker == boundary(request, window))
}

pub(in crate::agent) fn clear_prepared(request: &mut LlmRequest) {
    if let Some(config) = &mut request.config {
        config.extensions.remove(PREPARED);
    }
}

fn maskable(id: &str, name: &str, value: &Value) -> bool {
    !id.is_empty() && !protected(name, value) && value.get("masked") != Some(&Value::Bool(true))
}

pub(in crate::agent) fn masked(request: &LlmRequest) -> Masks {
    request
        .contents
        .iter()
        .flat_map(|content| &content.parts)
        .filter_map(|part| match part {
            Part::FunctionResponse {
                id,
                function_response,
                ..
            } if function_response.response.get("masked") == Some(&Value::Bool(true)) => {
                let source = source(&function_response.response)?;
                let id = id.as_deref().or_else(|| source["call"].as_str())?;
                Some((id.into(), source["revision"].as_str()?.into()))
            }
            _ => None,
        })
        .collect()
}

#[cfg(test)]
mod tests;
