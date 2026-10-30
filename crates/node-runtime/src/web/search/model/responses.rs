use super::*;
use serde_json::json;

pub(super) fn body(model: &str, input: &search::Request) -> Value {
    let mut tool = json!({"type":"web_search"});
    if !input.include_domains.is_empty() {
        tool["filters"]["allowed_domains"] = json!(input.include_domains);
    }
    if !input.exclude_domains.is_empty() {
        tool["filters"]["blocked_domains"] = json!(input.exclude_domains);
    }
    json!({
        "model":model,
        "input":[{"role":"user","content":[{"type":"input_text","text":filters::query(input)}]}],
        "tools":[tool],
        "tool_choice":"auto",
        "include":["web_search_call.action.sources"],
        "stream":true,
        "store":false
    })
}

pub(super) fn parse(value: &Value) -> Result<Parsed, Failure> {
    if let Some(error) = value.get("error").filter(|error| !error.is_null()) {
        return Err(network::upstream(error));
    }
    if value.get("status").and_then(Value::as_str) != Some("completed") {
        return Err(invalid("model search response did not complete"));
    }
    let output = value
        .get("output")
        .and_then(Value::as_array)
        .ok_or_else(|| invalid("model search response has no output"))?;
    let mut results = Vec::new();
    let mut parts = Vec::new();
    let mut searched = false;
    for item in output {
        match item.get("type").and_then(Value::as_str) {
            Some("web_search_call") => {
                if item.get("status").and_then(Value::as_str) != Some("completed") {
                    return Err(invalid("native web search did not complete"));
                }
                searched = true;
                if let Some(sources) = item.pointer("/action/sources").and_then(Value::as_array) {
                    for source in sources {
                        if let Some(url) = source.get("url").and_then(Value::as_str) {
                            push(
                                &mut results,
                                url,
                                source.get("title").and_then(Value::as_str),
                                None,
                            )?;
                        }
                    }
                }
            }
            Some("message") => {
                let Some(content) = item.get("content").and_then(Value::as_array) else {
                    continue;
                };
                for block in content {
                    if block.get("type").and_then(Value::as_str) != Some("output_text") {
                        continue;
                    }
                    if let Some(text) = block.get("text").and_then(Value::as_str) {
                        parts.push(text);
                    }
                    if let Some(annotations) = block.get("annotations").and_then(Value::as_array) {
                        for citation in annotations {
                            if citation.get("type").and_then(Value::as_str) != Some("url_citation")
                            {
                                continue;
                            }
                            let url = citation
                                .get("url")
                                .and_then(Value::as_str)
                                .ok_or_else(|| invalid("model search citation URL is missing"))?;
                            // Answer spans are model output, not source excerpts.
                            push(
                                &mut results,
                                url,
                                citation.get("title").and_then(Value::as_str),
                                None,
                            )?;
                        }
                    }
                }
            }
            _ => {}
        }
    }
    if !searched {
        return Err(invalid("model returned no web search metadata"));
    }
    Ok(Parsed {
        results,
        summary: summary(parts),
        usage: usage(value),
    })
}

pub(super) fn usage(value: &Value) -> Option<Usage> {
    value
        .get("usage")
        .filter(|value| value.is_object())
        .map(|usage| Usage {
            input: Some(tokens(usage, "/input_tokens")),
            output: Some(tokens(usage, "/output_tokens")),
            cache_read: Some(tokens(usage, "/input_tokens_details/cached_tokens")),
            reasoning: Some(tokens(usage, "/output_tokens_details/reasoning_tokens")),
            ..Default::default()
        })
}
