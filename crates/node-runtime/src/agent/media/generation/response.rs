//! Responses image results arrive as JSON or SSE; only a terminal event is authoritative.
use super::*;
use bytes::Bytes;
use eventsource_stream::{EventStreamError, Eventsource};
use futures::{Stream, StreamExt};
use reqwest::header::CONTENT_TYPE;
use std::collections::BTreeMap;

/// Return the terminal response object, or fail without guessing the provider outcome.
pub(super) async fn read(
    mut response: Response,
    limit: usize,
    secrets: &[Secret],
) -> Result<Value, Failure> {
    let status = response.status();
    if !status.is_success() {
        return Err(rejected(&mut response, secrets).await);
    }
    if response
        .content_length()
        .is_some_and(|size| size > limit as u64)
    {
        return Err(unavailable("media response exceeds the size limit").into());
    }
    let kind = response
        .headers()
        .get(CONTENT_TYPE)
        .map(|value| value.to_str().unwrap_or_default().to_owned());
    let mut prefix = Vec::new();
    let streaming = match &kind {
        Some(kind) => kind
            .split(';')
            .next()
            .is_some_and(|kind| kind.trim().eq_ignore_ascii_case("text/event-stream")),
        None => {
            // Account endpoints may omit Content-Type. Inspect only the first nonempty chunk.
            while let Some(chunk) = response.chunk().await.map_err(|_| uncertain())? {
                if chunk.len() > limit - prefix.len() {
                    return Err(unavailable("media response exceeds the size limit").into());
                }
                prefix.extend_from_slice(&chunk);
                if prefix.iter().any(|byte| !byte.is_ascii_whitespace()) {
                    break;
                }
            }
            prefix
                .iter()
                .find(|byte| !byte.is_ascii_whitespace())
                .is_some_and(|byte| matches!(byte, b'd' | b'e' | b':'))
        }
    };
    if streaming {
        let stream = futures::stream::once(async move { Ok(Bytes::from(prefix)) }).chain(
            response
                .bytes_stream()
                .map(|chunk| chunk.map_err(|_| uncertain())),
        );
        return events(stream, limit, secrets).await;
    }
    let tail = read_body(&mut response, limit - prefix.len()).await?;
    prefix.extend_from_slice(&tail);
    serde_json::from_slice(&prefix)
        .map_err(|_| unavailable("invalid media generation response").into())
}

async fn rejected(response: &mut Response, secrets: &[Secret]) -> Failure {
    let status = response.status().as_u16();
    let request = request_id(response);
    match read_body(response, 64 * 1024).await {
        Ok(bytes) => Failure::http(status, &bytes, true, request.as_deref(), secrets),
        Err(fault) => fault.into(),
    }
}

pub(super) fn request_id(response: &Response) -> Option<String> {
    ["x-request-id", "request-id", "openai-request-id"]
        .into_iter()
        .find_map(|name| response.headers().get(name)?.to_str().ok())
        .map(str::to_owned)
}

async fn events(
    bytes: impl Stream<Item = Result<Bytes, Fault>>,
    limit: usize,
    secrets: &[Secret],
) -> Result<Value, Failure> {
    let mut received = 0;
    let stream = bytes
        .map(move |chunk| {
            let chunk = chunk?;
            if chunk.len() > limit - received {
                return Err(unavailable("media response exceeds the size limit"));
            }
            received += chunk.len();
            Ok(chunk)
        })
        .eventsource();
    futures::pin_mut!(stream);
    let mut output = BTreeMap::new();
    while let Some(event) = stream.next().await {
        let event = event.map_err(|error| match error {
            EventStreamError::Transport(fault) => fault,
            _ => unavailable("invalid media generation event stream"),
        })?;
        if event.data == "[DONE]" {
            break;
        }
        let value: Value = serde_json::from_str(&event.data)
            .map_err(|_| unavailable("media generation stream contains invalid JSON"))?;
        match value
            .get("type")
            .and_then(Value::as_str)
            .unwrap_or(&event.event)
        {
            "response.output_item.done" => {
                let item = value
                    .get("item")
                    .filter(|item| item.is_object())
                    .cloned()
                    .ok_or_else(|| unavailable("media generation output item is missing"))?;
                let index = value
                    .get("output_index")
                    .and_then(Value::as_u64)
                    .unwrap_or(output.len() as u64);
                output.insert(index, item);
            }
            // Incomplete and failed responses are terminal; status validation reports them.
            "response.completed" | "response.incomplete" | "response.failed" => {
                let mut response = value
                    .get("response")
                    .filter(|response| response.is_object())
                    .cloned()
                    .ok_or_else(|| unavailable("media generation stream has no response"))?;
                let terminal = value["type"].as_str().unwrap_or(&event.event);
                if response["status"].as_str() != terminal.strip_prefix("response.") {
                    return Err(
                        unavailable("media generation terminal status is inconsistent").into(),
                    );
                }
                // Account endpoints may leave the terminal output empty after item events.
                if !output.is_empty()
                    && response
                        .get("output")
                        .is_none_or(|items| items.as_array().is_none_or(Vec::is_empty))
                {
                    response["output"] = Value::Array(output.into_values().collect());
                }
                return Ok(response);
            }
            "error" => {
                return Err(unavailable(&error::describe(
                    "image generation failed",
                    Some(value.get("error").unwrap_or(&value)),
                    None,
                    secrets,
                ))
                .into());
            }
            _ => {}
        }
    }
    // The request was accepted, so a missing terminal event leaves billing and output unknown.
    Err(uncertain().into())
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn parse(bytes: Vec<u8>, secrets: &[Secret]) -> Result<Value, Failure> {
        events(
            futures::stream::once(async move { Ok(Bytes::from(bytes)) }),
            IMAGE_BYTES * 2,
            secrets,
        )
        .await
    }

    fn stream(events: &[Value], done: bool) -> Vec<u8> {
        let mut body = String::new();
        for event in events {
            body.push_str(&format!("data: {event}\n\n"));
        }
        if done {
            body.push_str("data: [DONE]\n\n");
        }
        body.into_bytes()
    }

    fn item(status: &str) -> Value {
        json!({"type":"response.output_item.done","output_index":0,
            "item":{"type":"image_generation_call","status":status,"result":"AQID"}})
    }

    #[tokio::test]
    async fn merges_item_events_into_terminal_output() {
        let body = stream(
            &[
                json!({"type":"response.created","response":{"status":"in_progress","output":[]}}),
                json!({"type":"response.image_generation_call.partial_image","partial_image_b64":"AA=="}),
                item("completed"),
                json!({"type":"response.completed","response":{"status":"completed","output":[],
                    "usage":{"input_tokens":1,"output_tokens":2}}}),
            ],
            true,
        );
        let value = parse(body, &[]).await.unwrap();
        assert_eq!(value["usage"]["output_tokens"], 2);
        assert_eq!(image::completed(&value, &[]).unwrap(), vec![1, 2, 3]);
    }

    #[tokio::test]
    async fn keeps_authoritative_terminal_output() {
        let body = stream(
            &[
                item("in_progress"),
                json!({"type":"response.completed","response":{"status":"completed",
                    "output":[{"type":"image_generation_call","status":"completed","result":"AQID"}]}}),
            ],
            false,
        );
        let value = parse(body, &[]).await.unwrap();
        assert_eq!(image::completed(&value, &[]).unwrap(), vec![1, 2, 3]);
    }

    #[tokio::test]
    async fn reports_terminal_failures() {
        for terminal in ["response.failed", "response.incomplete"] {
            let status = terminal.trim_start_matches("response.");
            let body = stream(
                &[json!({"type":terminal,"response":{"status":status,
                    "error":{"code":"server_error","message":"Generation failed"}}})],
                false,
            );
            let value = parse(body, &[]).await.unwrap();
            let error = image::completed(&value, &[]).unwrap_err();
            assert_eq!(error.code, ErrorCode::Unavailable);
        }
        let secrets = [Secret::new("isolated-stream-access".into())];
        let body = stream(
            &[
                json!({"type":"error","error":{"code":"invalid_request_error",
                "message":"Bad token isolated-stream-access","param":"tools"}}),
            ],
            false,
        );
        let error = parse(body, &secrets).await.unwrap_err().fault();
        assert_eq!(error.code, ErrorCode::Unavailable);
        assert!(error.message.contains("param tools"), "{}", error.message);
        assert!(!error.message.contains("isolated-stream-access"));
    }

    #[tokio::test]
    async fn treats_missing_terminal_as_uncertain() {
        for done in [false, true] {
            let body = stream(&[item("completed")], done);
            let error = parse(body, &[]).await.unwrap_err();
            assert!(matches!(&error, Failure::Failed(_)));
            assert_eq!(error.fault().code, ErrorCode::OutcomeUnknown);
        }
    }

    #[tokio::test]
    async fn rejects_invalid_events() {
        let error = parse(b"data: {not json\n\n".to_vec(), &[])
            .await
            .unwrap_err();
        assert_eq!(error.fault().code, ErrorCode::Unavailable);
    }

    #[tokio::test]
    async fn finishes_without_waiting_for_eof() {
        let bytes = stream(
            &[
                json!({"type":"response.completed","response":{"status":"completed",
                "output":[{"type":"image_generation_call","status":"completed","result":"AQID"}]}}),
            ],
            false,
        );
        let source = futures::stream::once(async move { Ok(Bytes::from(bytes)) })
            .chain(futures::stream::pending());
        let value = tokio::time::timeout(Duration::from_secs(1), events(source, IMAGE_BYTES, &[]))
            .await
            .expect("completed stream waited for EOF")
            .unwrap();
        assert_eq!(image::completed(&value, &[]).unwrap(), vec![1, 2, 3]);
    }

    #[tokio::test]
    async fn bounds_chunks_before_parsing() {
        let source = futures::stream::iter([
            Ok(Bytes::from_static(b"data: ")),
            Ok(Bytes::from_static(b"oversized event without a delimiter")),
        ]);
        let error = events(source, 8, &[]).await.unwrap_err().fault();
        assert_eq!(error.code, ErrorCode::Unavailable);
        assert!(error.message.contains("size limit"));
    }

    #[tokio::test]
    async fn rejects_inconsistent_terminal_status() {
        let body = stream(
            &[
                json!({"type":"response.failed","response":{"status":"completed",
                "output":[{"type":"image_generation_call","status":"completed","result":"AQID"}]}}),
            ],
            false,
        );
        let error = parse(body, &[]).await.unwrap_err().fault();
        assert_eq!(error.code, ErrorCode::Unavailable);
        assert!(error.message.contains("inconsistent"));
    }
}
