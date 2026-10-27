//! Request-only budgets retain canonical results and their continuation references.
use super::*;
use crate::agent::{compaction::estimate, model::history::Window};
use adk_core::LlmRequest;
use std::collections::BTreeSet;
use std::io::Write;

/// Stored result revisions by call; references must match the canonical read path.
pub(in crate::agent) type Revisions = BTreeMap<String, BTreeSet<String>>;
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

pub(in crate::agent) fn revisions(events: &[adk_core::Event]) -> Revisions {
    collect(events.iter().filter_map(adk_core::Event::content))
}

fn collect<'a>(contents: impl Iterator<Item = &'a Content>) -> Revisions {
    contents
        .flat_map(|content| &content.parts)
        .filter_map(|part| match part {
            Part::FunctionResponse {
                id: Some(id),
                function_response,
                ..
            } if !id.is_empty() => Some((id.clone(), hash(&function_response.response))),
            _ => None,
        })
        .fold(Revisions::new(), |mut revisions, (id, revision)| {
            revisions.entry(id).or_default().insert(revision);
            revisions
        })
}

fn hash(value: &Value) -> String {
    blake3::hash(value.to_string().as_bytes())
        .to_hex()
        .to_string()
}

#[cfg(test)]
fn source(value: &Value, id: &str, revisions: &Revisions) -> Value {
    encoded_source(value, id, revisions, &value.to_string())
}

fn encoded_source(value: &Value, id: &str, revisions: &Revisions, encoded: &str) -> Value {
    if let Some(source) = value.get("sailry_output").or_else(|| value.get("output"))
        && source["call"].as_str() == Some(id)
        && source["revision"].as_str().is_some_and(|revision| {
            revision.len() == 64 && revision.bytes().all(|byte| byte.is_ascii_hexdigit())
        })
    {
        return source.clone();
    }
    if let Some(known) = revisions.get(id)
        && known.len() == 1
    {
        return json!({"call":id,"revision":known.first().unwrap()});
    }
    json!({"call":id,"revision":blake3::hash(encoded.as_bytes()).to_hex().to_string()})
}

pub(in crate::agent) const PREPARED: &str = "sailry.output.prepared";

#[derive(Clone)]
struct Slot {
    content: usize,
    part: usize,
    id: String,
    name: String,
    arguments: Option<Value>,
    source: Option<Value>,
    encoded: String,
    tokens: u64,
    overhead: u64,
    projected: bool,
}

#[derive(Clone)]
pub(in crate::agent) struct Prepared {
    slots: Vec<Slot>,
    revisions: Revisions,
}

pub(in crate::agent) fn prepare(request: &mut LlmRequest, revisions: &Revisions) -> Prepared {
    if let Some(config) = &mut request.config {
        config.extensions.remove(PREPARED);
    }
    Prepared::new(&mut request.contents, revisions)
}

impl Prepared {
    pub(in crate::agent) fn set_revisions(&mut self, revisions: &Revisions) {
        self.revisions = revisions.clone();
    }

    pub(super) fn new(contents: &mut [Content], revisions: &Revisions) -> Self {
        let mut calls = BTreeMap::new();
        let mut slots = Vec::new();
        for (content, message) in contents.iter_mut().enumerate() {
            for (part, value) in message.parts.iter_mut().enumerate() {
                if let Part::FunctionCall {
                    id: Some(id),
                    name,
                    args,
                    ..
                } = value
                {
                    calls.insert(id.clone(), (name.clone(), args.clone()));
                    continue;
                }
                if !matches!(value, Part::FunctionResponse { id: Some(id), .. } if !id.is_empty()) {
                    continue;
                }
                let Part::FunctionResponse {
                    id: Some(id),
                    function_response,
                    ..
                } = value
                else {
                    unreachable!()
                };
                let overhead = estimate::response(function_response, 0);
                let mut encoded = function_response.response.to_string();
                let mut canonical = None;
                if function_response.name.starts_with("mcp_")
                    && let Some(deduplicated) =
                        super::super::mcp::deduplicate(&function_response.response)
                {
                    canonical = Some(encoded_source(
                        &function_response.response,
                        id,
                        revisions,
                        &encoded,
                    ));
                    function_response.response = deduplicated;
                    function_response.response["sailry_output"] =
                        canonical.as_ref().unwrap().clone();
                    encoded = function_response.response.to_string();
                }
                let arguments = calls
                    .get(id)
                    .filter(|(name, _)| name == &function_response.name)
                    .map(|(_, arguments)| arguments.clone());
                slots.push(Slot {
                    content,
                    part,
                    id: id.clone(),
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
        Self {
            slots,
            revisions: revisions.clone(),
        }
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

    fn value<'a>(slot: &Slot, request: &'a LlmRequest) -> &'a Value {
        let Part::FunctionResponse {
            function_response, ..
        } = &request.contents[slot.content].parts[slot.part]
        else {
            unreachable!()
        };
        &function_response.response
    }

    fn reference(slot: &mut Slot, value: &Value, revisions: &Revisions) -> Value {
        slot.source
            .get_or_insert_with(|| encoded_source(value, &slot.id, revisions, &slot.encoded))
            .clone()
    }

    fn replace(slot: &mut Slot, contents: &mut [Content], value: Value) {
        slot.encoded = value.to_string();
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
            let source = Self::reference(slot, original, &self.revisions);
            if let Some(mut candidate) = super::candidate(
                original,
                &slot.name,
                slot.arguments.as_ref(),
                &source,
                &slot.encoded,
            ) {
                if let Some(context) = original.get("sailry_context") {
                    candidate["sailry_context"] = context.clone();
                }
                if candidate.to_string().len() < slot.encoded.len() {
                    Self::replace(slot, contents, candidate);
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
            let source = Self::reference(slot, value, &self.revisions);
            if !calls.contains(&(
                slot.id.clone(),
                source["revision"].as_str().unwrap().to_owned(),
            )) {
                continue;
            }
            let replacement = super::clip(value, &source, &slot.encoded, 500, true);
            Self::replace(slot, &mut request.contents, replacement);
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
            let source = Self::reference(slot, value, &self.revisions);
            let replacement = super::clip(value, &source, &slot.encoded, 500, true);
            let reduction = slot
                .tokens
                .saturating_sub(estimate::text(&replacement.to_string()));
            if reduction > 0 {
                saved += reduction;
                replacements.push((index, replacement));
            }
        }
        if saved < minimum {
            return Ok(0);
        }
        for (index, replacement) in replacements {
            Self::replace(&mut self.slots[index], &mut request.contents, replacement);
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
        let empty_config = request.config.is_none();
        request.config.get_or_insert_with(Default::default);
        self.project(&mut request.contents);
        let mut fingerprint = None;
        if let Some(window) = window {
            let output = request
                .config
                .as_ref()
                .and_then(|config| config.max_output_tokens)
                .map_or(window.output, |output| output.max(0) as u64)
                .min(window.output);
            let capacity = window.context.saturating_sub(output);
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
                    let source = Self::reference(slot, value, &self.revisions);
                    let replacement = super::clip(value, &source, &slot.encoded, budget, false);
                    let encoded = replacement.to_string();
                    if estimate::text(&encoded) < slot.tokens {
                        replaced.update(slot.id.as_bytes());
                        replaced.update(encoded.as_bytes());
                        Self::replace(slot, &mut request.contents, replacement);
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
        request
            .config
            .get_or_insert_with(Default::default)
            .extensions
            .remove(PREPARED);
        let stamp = signature(request, window, Some(self))?;
        request
            .config
            .as_mut()
            .unwrap()
            .extensions
            .insert(PREPARED.into(), json!(stamp));
        Ok(fingerprint)
    }
}

struct Writer(blake3::Hasher);
impl Write for Writer {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.0.update(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

fn signature(
    request: &LlmRequest,
    window: Option<Window>,
    prepared: Option<&Prepared>,
) -> adk_core::Result<String> {
    let mut writer = Writer(blake3::Hasher::new());
    let error = |_| adk_core::AdkError::model("prepared tool output could not be encoded");
    serde_json::to_writer(
        &mut writer,
        &(
            &request.model,
            &request.config,
            &request.tools,
            &request.previous_response_id,
            window.map(|window| (window.context, window.output)),
        ),
    )
    .map_err(error)?;
    let mut slots = prepared.map(|prepared| prepared.slots.iter().peekable());
    for (content, message) in request.contents.iter().enumerate() {
        serde_json::to_writer(&mut writer, &(&message.role, message.parts.len())).map_err(error)?;
        for (part, value) in message.parts.iter().enumerate() {
            if let Part::FunctionResponse {
                id,
                function_response,
                annotations,
            } = value
            {
                serde_json::to_writer(
                    &mut writer,
                    &(
                        id,
                        &function_response.name,
                        &function_response.inline_data,
                        &function_response.file_data,
                        annotations,
                    ),
                )
                .map_err(error)?;
                if let Some(slots) = &mut slots
                    && let Some(slot) = slots.peek()
                    && (slot.content, slot.part) == (content, part)
                {
                    writer.0.update(slot.encoded.as_bytes());
                    slots.next();
                } else {
                    serde_json::to_writer(&mut writer, &function_response.response)
                        .map_err(error)?;
                }
            } else {
                serde_json::to_writer(&mut writer, value).map_err(error)?;
            }
        }
    }
    Ok(writer.0.finalize().to_hex().to_string())
}

pub(in crate::agent) fn take_prepared(
    request: &mut LlmRequest,
    window: Option<Window>,
) -> adk_core::Result<bool> {
    let marker = request
        .config
        .as_mut()
        .and_then(|config| config.extensions.remove(PREPARED));
    let Some(Value::String(marker)) = marker else {
        return Ok(false);
    };
    Ok(signature(request, window, None)? == marker)
}

pub(in crate::agent) fn mask(
    request: &mut LlmRequest,
    protect: u64,
    minimum: u64,
    revisions: &Revisions,
) -> adk_core::Result<u64> {
    prepare(request, revisions).mask(request, protect, minimum)
}
pub(in crate::agent) fn remask(request: &mut LlmRequest, calls: &Masks, revisions: &Revisions) {
    prepare(request, revisions).remask(request, calls);
}
pub(in crate::agent) fn limit(
    request: &mut LlmRequest,
    window: Option<Window>,
    revisions: &Revisions,
) -> adk_core::Result<Option<String>> {
    prepare(request, revisions).limit(request, window)
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
                id: Some(id),
                function_response,
                ..
            } if function_response.response.get("masked") == Some(&Value::Bool(true)) => {
                function_response.response["output"]["revision"]
                    .as_str()
                    .map(|revision| (id.clone(), revision.into()))
            }
            _ => None,
        })
        .collect()
}

#[cfg(test)]
mod tests;
