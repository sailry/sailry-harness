use super::*;
use adk_core::{Content, EventCompaction, FunctionResponseData, LlmResponse};
use chrono::Utc;
use std::sync::Mutex;

fn capture(base: Effort, previous: Effort, current: Effort) -> Capture {
    Capture {
        origin: ProviderOrigin {
            provider: "responses".into(),
            model: "gpt-6-luna".into(),
            scope: "fixture".into(),
        },
        key: format!("{}fixture", Capture::PREFIX),
        state: State { base, current },
        previous,
    }
}

fn summary() -> Event {
    let mut event = Event::new("summary-fixture");
    event.author = "system".into();
    event.actions.compaction = Some(EventCompaction {
        start_timestamp: Utc::now(),
        end_timestamp: Utc::now(),
        compacted_content: Content::new("user").with_text("Captured summary"),
    });
    event
}

#[test]
fn appends_only_changed_discrete_settings() {
    let first = capture(Effort::Max, Effort::Max, Effort::Max);
    assert_eq!(first.base(), Effort::Max);
    assert!(first.input().is_none());
    let changed = capture(Effort::Max, Effort::Max, Effort::High);
    assert!(matches!(
        changed.input().unwrap().provider_part(),
        Part::ConfigurationUpdate {
            configuration_update: ConfigurationUpdate {
                effort: ReasoningEffort::High
            }
        }
    ));
    assert!(
        capture(Effort::Max, Effort::High, Effort::High)
            .input()
            .is_none()
    );
    assert_eq!(discrete(Effort::Default), None);
    assert_eq!(discrete(Effort::Budget(1024)), None);
}

#[test]
fn preserves_existing_prefix_and_update_position() {
    let changed = capture(Effort::Max, Effort::Max, Effort::High);
    let first = Content::new("user").with_text("Original request");
    let answer = Content::new("model").with_text("Original answer");
    let mut next = Content::new("user").with_text("Next request");
    next.parts.insert(0, changed.input().unwrap());
    let mut request = LlmRequest::new("gpt-6-luna", vec![first, answer, next]);
    let original = serde_json::to_vec(&request.contents).unwrap();
    changed.restore(&mut request);
    assert_eq!(serde_json::to_vec(&request.contents).unwrap(), original);
    capture(Effort::Max, Effort::High, Effort::High).restore(&mut request);
    assert_eq!(serde_json::to_vec(&request.contents).unwrap(), original);
    request
        .contents
        .push(Content::new("user").with_text("Another request"));
    capture(Effort::Max, Effort::High, Effort::High).restore(&mut request);
    assert_eq!(request.contents[2].parts.len(), 2);
    assert_eq!(request.contents[3].parts.len(), 1);
    // A summary can retain an earlier owner update in its uncompressed suffix.
    // Restoring that owner's state adds one new input item after the old text.
    let mut resumed = Content::new("user").with_text("Restored owner input");
    resumed.parts.insert(0, changed.input().unwrap());
    request.contents.push(resumed);
    let original = serde_json::to_vec(&request.contents).unwrap();
    changed.restore(&mut request);
    assert_eq!(serde_json::to_vec(&request.contents).unwrap(), original);
    assert_eq!(request.contents[2].parts[1].text(), Some("Next request"));
    assert_eq!(
        request.contents[4].parts[1].text(),
        Some("Restored owner input")
    );
}

#[test]
fn restores_rebuilt_and_summary_requests() {
    let changed = capture(Effort::Max, Effort::High, Effort::High);
    let first = Content::new("user").with_text("Rebuilt prefix");
    let response = Content {
        role: "function".into(),
        parts: vec![Part::FunctionResponse {
            id: Some("call".into()),
            function_response: FunctionResponseData::new("fixture", serde_json::json!({})),
            annotations: None,
        }],
    };
    let mut request = LlmRequest::new("gpt-6-luna", vec![first, response]);
    changed.restore(&mut request);
    assert!(matches!(
        request.contents[1].parts[1].provider_part(),
        Part::ConfigurationUpdate { .. }
    ));
    assert_eq!(request.contents[0].parts.len(), 1);
    let original = serde_json::to_vec(&request.contents).unwrap();
    changed.restore(&mut request);
    assert_eq!(serde_json::to_vec(&request.contents).unwrap(), original);

    let older = capture(Effort::Max, Effort::Max, Effort::Low);
    let mut input = Content::new("user").with_text("Earlier request");
    input.parts.insert(0, older.input().unwrap());
    let before = input.parts.clone();
    let mut request = LlmRequest::new("gpt-6-luna", vec![input]);
    changed.restore(&mut request);
    assert_eq!(&request.contents[0].parts[..before.len()], before);
    assert!(matches!(
        request.contents[0].parts.last().unwrap().provider_part(),
        Part::ConfigurationUpdate {
            configuration_update: ConfigurationUpdate {
                effort: ReasoningEffort::High
            }
        }
    ));
    assert_eq!(request.contents[0].parts[1].text(), Some("Earlier request"));
}

#[test]
fn resumes_base_after_retained_update() {
    let older = capture(Effort::Max, Effort::Max, Effort::High);
    let resumed = capture(Effort::Max, Effort::High, Effort::Max);
    let mut retained = Content::new("user").with_text("Retained owner input");
    retained.parts.insert(0, older.input().unwrap());
    let mut current = Content::new("user").with_text("Resume at base effort");
    current.parts.insert(0, resumed.input().unwrap());
    let mut event = Event::new("resumed-input");
    event.author = "user".into();
    event.set_content(current.clone());
    resumed.record(&mut event);
    let mut request = LlmRequest::new(
        "gpt-6-luna",
        vec![
            Content::new("user").with_text("Foreign summary"),
            retained,
            current,
        ],
    );
    let prefix = serde_json::to_vec(&request.contents).unwrap();
    resumed.restore(&mut request);
    assert_eq!(serde_json::to_vec(&request.contents).unwrap(), prefix);
    assert_eq!(
        request
            .contents
            .iter()
            .flat_map(|content| &content.parts)
            .filter(|part| matches!(part.provider_part(), Part::ConfigurationUpdate { .. }))
            .count(),
        2,
    );
    let state: State =
        serde_json::from_value(event.actions.state_delta[&resumed.key].clone()).unwrap();
    let same = Capture {
        previous: state.current,
        state,
        ..resumed
    };
    assert!(same.input().is_none());
    request
        .contents
        .push(Content::new("user").with_text("Ordinary following input"));
    let next = serde_json::to_vec(&request.contents).unwrap();
    same.restore(&mut request);
    assert_eq!(serde_json::to_vec(&request.contents).unwrap(), next);
    assert_eq!(request.contents[3].parts.len(), 1);
    assert_eq!(serde_json::to_vec(&request.contents[..3]).unwrap(), prefix,);
}

#[test]
fn persists_compacted_settings_with_owned_content() {
    let changed = capture(Effort::Max, Effort::Max, Effort::High);
    let mut event = summary();
    changed.compact(&mut event);
    let encoded = serde_json::to_vec(&event).unwrap();
    let event: Event = serde_json::from_slice(&encoded).unwrap();
    let state: State =
        serde_json::from_value(event.actions.state_delta[&changed.key].clone()).unwrap();
    assert_eq!(state.base, Effort::Max);
    assert_eq!(state.current, Effort::High);
    let reset: State = serde_json::from_value(
        Capture::compact_state(
            serde_json::to_value(State {
                base: Effort::Low,
                current: Effort::Medium,
            })
            .unwrap(),
            None,
        )
        .unwrap(),
    )
    .unwrap();
    assert_eq!(reset.current, reset.base);
    let content = event.actions.compaction.unwrap().compacted_content;
    assert_eq!(content.parts.len(), 2);
    let mut owned = vec![content.clone()];
    adk_core::history::for_provider(&mut owned, &changed.origin);
    assert!(matches!(
        owned[0].parts[0],
        Part::ConfigurationUpdate { .. }
    ));
    let mut foreign_origin = changed.origin.clone();
    foreign_origin.model = "unsupported".into();
    let mut foreign = vec![content];
    adk_core::history::for_provider(&mut foreign, &foreign_origin);
    assert_eq!(foreign[0].parts.len(), 1);
    assert_eq!(foreign[0].parts[0].text(), Some("Captured summary"));
    let mut unchanged = summary();
    capture(Effort::Max, Effort::Max, Effort::Max).compact(&mut unchanged);
    assert_eq!(
        unchanged
            .actions
            .compaction
            .unwrap()
            .compacted_content
            .parts
            .len(),
        1
    );
}

struct Recording(Arc<Mutex<Vec<LlmRequest>>>);
#[async_trait]
impl Llm for Recording {
    fn name(&self) -> &str {
        "configuration-fixture"
    }
    async fn generate_content(
        &self,
        request: LlmRequest,
        _: bool,
    ) -> adk_core::Result<LlmResponseStream> {
        self.0.lock().unwrap().push(request);
        Ok(Box::pin(futures::stream::empty::<
            adk_core::Result<LlmResponse>,
        >()))
    }
}

#[tokio::test]
async fn unsupported_capture_omits_updates() {
    let current = capture(Effort::Max, Effort::Max, Effort::High);
    let mut input = Content::new("user").with_text("Preserved input");
    input.parts.insert(0, current.input().unwrap());
    let records = Arc::new(Mutex::new(Vec::new()));
    wrap(Arc::new(Recording(records.clone())), None)
        .generate_content(LlmRequest::new("fixture", vec![input]), false)
        .await
        .unwrap();
    let records = records.lock().unwrap();
    assert_eq!(records[0].contents[0].parts.len(), 1);
    assert_eq!(
        records[0].contents[0].parts[0].text(),
        Some("Preserved input")
    );
}
