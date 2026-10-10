//! Opt-in metadata-only acceptance against the selected Codex authorization.
use super::*;
use adk_core::{Content, LlmRequest};
use futures::StreamExt;
use sailry_protocol::{Permission, ProviderId, Secret, SessionId, WorkMode};
use std::{
    sync::{
        Mutex,
        atomic::{AtomicUsize, Ordering},
    },
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

struct Source {
    provider: Provider,
    grant: Grant,
    model: String,
}

fn source() -> Source {
    let profile = std::env::var_os("SAILRY_ACCEPTANCE_PROFILE")
        .expect("SAILRY_ACCEPTANCE_PROFILE is required");
    let id: ProviderId = std::env::var("SAILRY_ACCEPTANCE_PROVIDER_ID")
        .expect("SAILRY_ACCEPTANCE_PROVIDER_ID is required")
        .parse()
        .unwrap_or_else(|_| panic!("a valid provider ID is required"));
    let model =
        std::env::var("SAILRY_ACCEPTANCE_MODEL").expect("SAILRY_ACCEPTANCE_MODEL is required");
    let path = std::fs::canonicalize(std::path::Path::new(&profile).join("storage/node.sqlite3"))
        .unwrap_or_else(|_| panic!("the selected profile database is unavailable"));
    let mut uri = url::Url::from_file_path(path)
        .unwrap_or_else(|_| panic!("the selected profile path is invalid"));
    // Do not use immutable: an active profile's latest grant may still be in WAL.
    uri.query_pairs_mut().append_pair("mode", "ro");
    let db = rusqlite::Connection::open_with_flags(
        uri.as_str(),
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY | rusqlite::OpenFlags::SQLITE_OPEN_URI,
    )
    .unwrap_or_else(|_| panic!("the selected profile cannot be read without writes"));
    db.busy_timeout(Duration::from_secs(2))
        .unwrap_or_else(|_| panic!("the source read deadline is unavailable"));
    let body: Vec<u8> = db
        .query_row(
            "SELECT body FROM providers WHERE id=?1",
            [id.to_string()],
            |row| row.get(0),
        )
        .unwrap_or_else(|_| panic!("the selected provider is unavailable"));
    drop(db);
    let stored: Value = serde_json::from_slice(&body)
        .unwrap_or_else(|_| panic!("the selected provider record is invalid"));
    assert!(stored["listed"] == true, "the provider must be listed");
    let provider: Provider = serde_json::from_value(stored["provider"].clone())
        .unwrap_or_else(|_| panic!("the selected provider configuration is invalid"));
    assert!(
        provider.id == id
            && provider.enabled
            && provider.authentication == Authentication::ChatGptOAuth
            && provider.models.iter().any(|entry| entry.id == model),
        "an enabled Codex provider with the selected model is required"
    );
    crate::providers::login::validate(&provider)
        .unwrap_or_else(|_| panic!("the selected provider must use the official Codex endpoint"));
    provider
        .models
        .iter()
        .find(|entry| entry.id == model)
        .expect("the selected model is unavailable")
        .validate_effort(provider.api, Effort::Low)
        .unwrap_or_else(|_| panic!("the selected model must support low reasoning effort"));
    let reference = provider
        .credential
        .as_ref()
        .expect("the selected provider requires an authorization");
    let encoded = stored["authorizations"][reference.id.to_string()]["key"]
        .as_str()
        .expect("the selected inline authorization is unavailable");
    let grant = Grant::decode(provider.authentication, &Secret::new(encoded.into()))
        .unwrap_or_else(|_| panic!("the selected authorization is invalid"));
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_else(|_| panic!("the system clock is unavailable"))
        .as_millis();
    assert!(
        !grant.renewal_pending() && u128::from(grant.expires_at_ms()) > now + 60_000,
        "an unexpired authorization is required; this test never refreshes it"
    );
    Source {
        provider,
        grant,
        model,
    }
}

#[derive(Default)]
struct Metrics {
    callbacks: usize,
    intermediates: usize,
    partials: usize,
    bytes: usize,
    final_bytes: usize,
    first: Option<Duration>,
    last: Option<Duration>,
    completed: Option<Duration>,
}

#[tokio::test]
#[ignore = "requires caller-selected Sailry OAuth authorization and incurs one real model request"]
async fn streams_before_completion() {
    let source = source();
    let _ = rustls::crypto::ring::default_provider().install_default();
    let options = crate::providers::login::effective(&source.provider)
        .unwrap_or_else(|_| panic!("the captured OAuth options are invalid"));
    let base = source
        .grant
        .headers(&options)
        .unwrap_or_else(|_| panic!("the captured OAuth headers are invalid"));
    let session = SessionId::new();
    let attempts = Arc::new(AtomicUsize::new(0));
    let count = attempts.clone();
    let adapter: RequestAdapter = Arc::new(move |body, headers| {
        if count.fetch_add(1, Ordering::SeqCst) != 0 {
            return Err(AdkError::model("live acceptance does not retry requests"));
        }
        headers.extend(base.clone());
        codex::adapt(body, headers, session)
    });
    let content_types = Arc::new(Mutex::new(Vec::new()));
    let captured = content_types.clone();
    let observer: ResponseObserver = Arc::new(move |headers| {
        let category = match headers
            .get("content-type")
            .and_then(|value| value.to_str().ok())
        {
            None => "absent",
            Some(value) if value.starts_with("text/event-stream") => "text/event-stream",
            Some(value) if value.starts_with("application/json") => "application/json",
            Some(_) => "other",
        };
        captured.lock().unwrap().push(category);
    });
    let config = SessionConfig {
        assistant: None,
        resource: None,
        provider: source.provider.id,
        model: source.model.clone(),
        effort: Effort::Low,
        mode: WorkMode::Code,
        permission: Permission::Ask,
        credential: source.provider.credential.clone(),
    };
    let model = super::super::super::native(
        &source.provider,
        &config,
        source.grant.access().expose(),
        Effort::Low,
        Some(adapter),
        Some(observer),
    )
    .unwrap_or_else(|_| panic!("the live model configuration is invalid"));
    let request = LlmRequest::new(
        source.model,
        vec![Content::new("user").with_text(
            "Write exactly six short sentences about public transport. Use plain text and no tools.",
        )],
    );
    assert!(
        request.tools.is_empty(),
        "the live request must not contain tools"
    );
    let started = Instant::now();
    let result = tokio::time::timeout(Duration::from_secs(60), async {
        let mut stream = model.generate_content(request, true).await?;
        let mut metrics = Metrics::default();
        while let Some(response) = stream.next().await {
            let response = response?;
            metrics.callbacks += 1;
            let bytes: usize = response
                .content
                .iter()
                .flat_map(|content| &content.parts)
                .filter_map(|part| match part {
                    Part::Text { text } => Some(text.len()),
                    _ => None,
                })
                .sum();
            assert!(
                !response.interrupted
                    && response.error_code.is_none()
                    && response.error_message.is_none(),
                "the live response did not complete successfully"
            );
            assert!(
                response
                    .content
                    .iter()
                    .flat_map(|content| &content.parts)
                    .all(|part| {
                        !matches!(
                            part,
                            Part::FunctionCall { .. }
                                | Part::FunctionResponse { .. }
                                | Part::ServerToolCall { .. }
                                | Part::ServerToolResponse { .. }
                        )
                    }),
                "the live response must not request tools"
            );
            if response.turn_complete {
                assert!(
                    !response.partial
                        && metrics.completed.is_none()
                        && response.provider_metadata.as_ref().is_some_and(|metadata| {
                            metadata["openai"]["status"] == "completed"
                        }),
                    "one successful terminal response is required"
                );
                // ADK accumulates emitted text; a successful terminal may be metadata only.
                metrics.final_bytes = bytes;
                metrics.completed = Some(started.elapsed());
            } else if response.partial && bytes > 0 {
                let elapsed = started.elapsed();
                metrics.partials += 1;
                metrics.bytes += bytes;
                metrics.first.get_or_insert(elapsed);
                metrics.last = Some(elapsed);
            } else {
                metrics.intermediates += 1;
            }
        }
        Ok::<_, AdkError>(metrics)
    })
    .await;
    let metrics = match result {
        Ok(Ok(metrics)) => metrics,
        Ok(Err(_)) => panic!("the real OAuth request failed; provider details are redacted"),
        Err(_) => panic!("the real OAuth request exceeded its 60 second deadline"),
    };
    let first = metrics
        .first
        .expect("a nonempty textual partial is required");
    let last = metrics
        .last
        .expect("a nonempty textual partial is required");
    let completed = metrics
        .completed
        .expect("a successful completion is required");
    assert!(metrics.bytes > 0, "a textual streamed outcome is required");
    assert!(first < completed, "text must arrive before completion");
    assert!(
        attempts.load(Ordering::SeqCst) == 1,
        "one HTTP attempt is required"
    );
    println!(
        "OAuth stream: callbacks={}, intermediates={}, partials={}, bytes={}, final_bytes={}, first_ms={}, last_ms={}, completed_ms={}, fragment_span_ms={}, content_types={:?}",
        metrics.callbacks,
        metrics.intermediates,
        metrics.partials,
        metrics.bytes,
        metrics.final_bytes,
        first.as_millis(),
        last.as_millis(),
        completed.as_millis(),
        (last - first).as_millis(),
        content_types.lock().unwrap(),
    );
}
