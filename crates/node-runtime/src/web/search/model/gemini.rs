use super::*;
use chrono::{DateTime, SecondsFormat};
use serde_json::json;

pub(super) fn body(input: &search::Request, now: DateTime<Utc>) -> Value {
    let mut tool = json!({});
    if let Some(freshness) = input.freshness {
        tool["timeRangeFilter"] = json!({
            "startTime":filters::since(freshness, now).to_rfc3339_opts(SecondsFormat::Secs, true),
            "endTime":now.to_rfc3339_opts(SecondsFormat::Secs, true)
        });
    }
    json!({
        "contents":[{"role":"user","parts":[{"text":filters::query(input)}]}],
        "tools":[{"googleSearch":tool}]
    })
}

pub(super) fn parse(value: &Value) -> Result<Parsed, Failure> {
    if let Some(error) = value.get("error").filter(|error| !error.is_null()) {
        return Err(network::upstream(error));
    }
    let candidate = value
        .get("candidates")
        .and_then(Value::as_array)
        .and_then(|candidates| candidates.first())
        .ok_or_else(|| invalid("model search response has no candidate"))?;
    if candidate.get("finishReason").and_then(Value::as_str) != Some("STOP") {
        return Err(invalid("model search response did not complete"));
    }
    let metadata = candidate
        .get("groundingMetadata")
        .filter(|metadata| {
            metadata.get("groundingChunks").is_some_and(Value::is_array)
                || metadata
                    .get("webSearchQueries")
                    .is_some_and(Value::is_array)
        })
        .ok_or_else(|| invalid("model returned no web search metadata"))?;
    let mut results = Vec::new();
    if let Some(chunks) = metadata.get("groundingChunks").and_then(Value::as_array) {
        for chunk in chunks {
            let web = &chunk["web"];
            if web.is_object() {
                let url = web
                    .get("uri")
                    .and_then(Value::as_str)
                    .ok_or_else(|| invalid("model search source URL is missing"))?;
                // Grounding support segments quote the generated answer, not the source.
                push(
                    &mut results,
                    url,
                    web.get("title").and_then(Value::as_str),
                    None,
                )?;
            }
        }
    }
    let parts = candidate
        .pointer("/content/parts")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter(|part| part.get("thought").and_then(Value::as_bool) != Some(true))
        .filter_map(|part| part.get("text").and_then(Value::as_str))
        .collect();
    Ok(Parsed {
        results,
        summary: summary(parts),
        usage: usage(value),
    })
}

pub(super) fn usage(value: &Value) -> Option<Usage> {
    value
        .get("usageMetadata")
        .filter(|value| value.is_object())
        .map(|usage| {
            let reasoning = tokens(usage, "/thoughtsTokenCount");
            Usage {
                input: Some(tokens(usage, "/promptTokenCount")),
                output: Some(tokens(usage, "/candidatesTokenCount").saturating_add(reasoning)),
                cache_read: Some(tokens(usage, "/cachedContentTokenCount")),
                reasoning: Some(reasoning),
                ..Default::default()
            }
        })
}
