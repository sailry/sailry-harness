use super::*;
use serde_json::json;
use std::time::Duration;

mod logs;

fn capture(timeline: Option<Arc<crate::agent::observation::Timeline>>) -> Arc<Mutex<Capture>> {
    Capture::new(
        TurnId::new(),
        "fixture".into(),
        super::super::call::identity(crate::logging::Context::default()),
        timeline,
        Default::default(),
    )
}

fn started(observer: &wire::Observer) {
    observer(wire::Event {
        attempt: 1,
        kind: wire::Kind::Started {
            transport: wire::Transport::Http,
            reused: Some(false),
        },
    });
}

fn finished(observer: &wire::Observer, outcome: wire::Outcome) {
    observer(wire::Event {
        attempt: 1,
        kind: wire::Kind::Finished {
            elapsed: Duration::from_millis(2),
            outcome,
            failure: None,
        },
    });
}

fn acknowledge(capture: &Arc<Mutex<Capture>>, record: &Request) {
    let mut value = capture.lock().unwrap();
    value.announced = true;
    value
        .persisted
        .extend(record.attempts.iter().map(|attempt| attempt.index));
}

#[test]
fn persists_each_recovered_attempt_once() {
    let capture = capture(None);
    let announcement = capture.lock().unwrap().pending().pop().unwrap();
    assert!(announcement.attempts.is_empty());
    acknowledge(&capture, &announcement);
    let mut ids = BTreeSet::new();
    for (index, outcome) in [
        wire::Outcome::Failed,
        wire::Outcome::Failed,
        wire::Outcome::Succeeded,
    ]
    .into_iter()
    .enumerate()
    {
        let observer = Capture::observer(&capture);
        started(&observer);
        assert!(
            capture.lock().unwrap().pending().is_empty(),
            "active attempts are not durable snapshots"
        );
        finished(&observer, outcome);
        let diagnostics = capture.lock().unwrap().diagnostics();
        assert_eq!(diagnostics.len(), 2);
        assert!(matches!(diagnostics[0].phase, crate::logging::Phase::Begin));
        assert_eq!(diagnostics[1].attempt, Some(index as u64 + 1));
        assert!(capture.lock().unwrap().diagnostics().is_empty());
        let pending = capture.lock().unwrap().pending();
        assert_eq!(pending.len(), 1);
        let record = &pending[0];
        assert_eq!(record.id, announcement.id);
        assert_eq!(record.attempts.len(), 1);
        assert_eq!(record.attempts[0].index, index as u64 + 1);
        let repeated = capture.lock().unwrap().pending();
        assert_eq!(
            serde_json::to_value(event(record)).unwrap(),
            serde_json::to_value(event(&repeated[0])).unwrap()
        );
        assert!(ids.insert(event(record).id));
        acknowledge(&capture, record);
        assert!(capture.lock().unwrap().pending().is_empty());
    }
    assert_eq!(capture.lock().unwrap().request.attempts.len(), 3);
}

#[test]
fn retains_actual_helper_usage_after_failure() {
    let capture = capture(None);
    capture.lock().unwrap().compaction = true;
    let observer = Capture::observer(&capture);
    started(&observer);
    {
        let mut value = capture.lock().unwrap();
        for output in [3, 7] {
            value.response(&LlmResponse {
                usage_metadata: Some(adk_core::UsageMetadata {
                    prompt_token_count: 12,
                    candidates_token_count: output,
                    thinking_token_count: Some(2),
                    cost: Some(0.125),
                    ..Default::default()
                }),
                ..Default::default()
            });
        }
    }
    finished(&observer, wire::Outcome::Failed);
    let value = capture.lock().unwrap();
    let records = value.pending();
    assert!(
        value
            .project(&records[0])
            .llm_response
            .usage_metadata
            .is_none()
    );
    let event = value.project(&records[1]);
    let usage = event.llm_response.usage_metadata.as_ref().unwrap();
    assert_eq!(usage.candidates_token_count, 7);
    assert_eq!(usage.thinking_token_count, Some(2));
    assert_eq!(usage.cost, Some(0.125));
    assert_eq!(
        event.provider_metadata["sailry_model_purpose"],
        "compaction"
    );
}

#[test]
fn ignores_duplicate_and_late_callbacks() {
    let timeline = crate::agent::observation::Timeline::new();
    let capture = capture(Some(timeline.clone()));
    let first = Capture::observer(&capture);
    started(&first);
    {
        let mut value = capture.lock().unwrap();
        value
            .starts
            .insert(1, Instant::now() - Duration::from_millis(20));
    }
    started(&first);
    finished(&first, wire::Outcome::Failed);
    std::thread::sleep(Duration::from_millis(3));
    let first_attempt = capture.lock().unwrap().request.attempts[0].clone();
    let waiting = timeline.summary().waiting_us;
    let next = Capture::observer(&capture);
    started(&next);
    started(&first);
    finished(&first, wire::Outcome::Succeeded);
    let value = capture.lock().unwrap();
    assert_eq!(value.active, Some(2));
    assert_eq!(value.request.attempts.len(), 2);
    assert_eq!(value.request.attempts[0], first_attempt);
    assert_eq!(timeline.summary().waiting_us, waiting);
    assert_eq!(waiting, Some(2000));
}

#[test]
fn keeps_final_usage_and_cancellation() {
    let capture = capture(None);
    let observer = Capture::observer(&capture);
    started(&observer);
    finished(&observer, wire::Outcome::Cancelled);
    let mut value = capture.lock().unwrap();
    value.response(&LlmResponse {
        usage_metadata: Some(adk_core::UsageMetadata {
            prompt_token_count: 12,
            candidates_token_count: 3,
            ..Default::default()
        }),
        ..Default::default()
    });
    let pending = value.pending();
    assert_eq!(pending.len(), 2);
    let attempt = &pending[1].attempts[0];
    assert_eq!(attempt.outcome, Outcome::Cancelled);
    assert_eq!(attempt.usage.as_ref().unwrap().input, 12);
    assert_eq!(attempt.usage.as_ref().unwrap().output, 3);
}

#[test]
fn records_headers_as_http_response() {
    let capture = capture(None);
    let observer = Capture::observer(&capture);
    started(&observer);
    capture.lock().unwrap().responded(Instant::now());
    assert_eq!(
        capture.lock().unwrap().request.attempts[0].response_us,
        None
    );
    observer(wire::Event {
        attempt: 1,
        kind: wire::Kind::Headers {
            status: 503,
            elapsed: Duration::from_millis(2),
            reused: None,
        },
    });
    finished(&observer, wire::Outcome::Failed);
    let value = capture.lock().unwrap();
    let attempt = &value.request.attempts[0];
    assert_eq!(attempt.headers_us, Some(2_000));
    assert_eq!(attempt.response_us, Some(2_000));
    assert_eq!(attempt.status, Some(503));
    assert_eq!(attempt.outcome, Outcome::Failed);
}

#[test]
fn separates_socket_response_from_handshake() {
    for reused in [false, true] {
        let capture = capture(None);
        let observer = Capture::observer(&capture);
        observer(wire::Event {
            attempt: 1,
            kind: wire::Kind::Started {
                transport: wire::Transport::WebSocket,
                reused: Some(reused),
            },
        });
        let origin = Instant::now();
        capture.lock().unwrap().starts.insert(1, origin);
        if !reused {
            observer(wire::Event {
                attempt: 1,
                kind: wire::Kind::Headers {
                    status: 101,
                    elapsed: Duration::from_millis(2),
                    reused: Some(false),
                },
            });
        }
        assert_eq!(
            capture.lock().unwrap().request.attempts[0].response_us,
            None
        );
        {
            let mut value = capture.lock().unwrap();
            value.responded(origin + Duration::from_millis(8));
            value.responded(origin + Duration::from_millis(9));
        }
        observer(wire::Event {
            attempt: 1,
            kind: wire::Kind::Finished {
                elapsed: Duration::from_millis(20),
                outcome: wire::Outcome::Failed,
                failure: None,
            },
        });
        let mut value = capture.lock().unwrap();
        value.responded(origin + Duration::from_millis(10));
        let attempt = &value.request.attempts[0];
        assert_eq!(attempt.response_us, Some(8_000));
        assert_eq!(attempt.headers_us, (!reused).then_some(2_000));
        assert_eq!(attempt.outcome, Outcome::Failed);
    }
}

#[test]
fn leaves_unreceived_socket_responses_unknown() {
    let capture = capture(None);
    let observer = Capture::observer(&capture);
    observer(wire::Event {
        attempt: 1,
        kind: wire::Kind::Started {
            transport: wire::Transport::WebSocket,
            reused: Some(true),
        },
    });
    finished(&observer, wire::Outcome::Cancelled);
    capture.lock().unwrap().responded(Instant::now());
    assert_eq!(
        capture.lock().unwrap().request.attempts[0].response_us,
        None
    );
}

#[test]
fn resets_response_time_for_each_attempt() {
    let capture = capture(None);
    for (index, elapsed, outcome) in [
        (1, 8, wire::Outcome::Failed),
        (2, 3, wire::Outcome::Succeeded),
    ] {
        let observer = Capture::observer(&capture);
        observer(wire::Event {
            attempt: 1,
            kind: wire::Kind::Started {
                transport: wire::Transport::WebSocket,
                reused: Some(true),
            },
        });
        let origin = Instant::now();
        {
            let mut value = capture.lock().unwrap();
            value.starts.insert(index, origin);
            value.responded(origin + Duration::from_millis(elapsed));
        }
        observer(wire::Event {
            attempt: 1,
            kind: wire::Kind::Finished {
                elapsed: Duration::from_millis(20),
                outcome,
                failure: None,
            },
        });
    }
    let value = capture.lock().unwrap();
    assert_eq!(value.request.attempts[0].response_us, Some(8_000));
    assert_eq!(value.request.attempts[1].response_us, Some(3_000));
    assert_eq!(value.request.attempts[0].outcome, Outcome::Failed);
    assert_eq!(value.request.attempts[1].outcome, Outcome::Succeeded);
}

#[tokio::test]
async fn flushes_terminal_response_metadata() {
    let capture = capture(None);
    let observer = Capture::observer(&capture);
    wire::scope(observer.clone(), async {
        let attempt = wire::Attempt::start(wire::Transport::Http, None);
        attempt.finish(wire::Outcome::Succeeded);
        // The transport owner can finish before its response reaches the caller.
        attempt.prepared_json(&json!({"input":"PRIVATE_INPUT"}));
        attempt.connection(true);
        attempt.headers(200, Some(true));
        capture.lock().unwrap().response(&LlmResponse {
            usage_metadata: Some(adk_core::UsageMetadata {
                prompt_token_count: 12,
                candidates_token_count: 3,
                cache_read_input_token_count: Some(2),
                thinking_token_count: Some(1),
                ..Default::default()
            }),
            ..Default::default()
        });
        attempt.finish(wire::Outcome::Failed);
        Ok::<_, ()>(())
    })
    .await
    .unwrap();
    let records = capture.lock().unwrap().pending();
    assert_eq!(records.len(), 2);
    let attempt = &records[1].attempts[0];
    assert_eq!(attempt.outcome, Outcome::Succeeded);
    assert!(attempt.elapsed_us.is_some());
    assert_eq!(attempt.status, Some(200));
    assert!(attempt.headers_us.is_some());
    assert_eq!(attempt.response_us, attempt.headers_us);
    assert_eq!(attempt.reused, Some(true));
    assert!(attempt.complete_context);
    assert_eq!(
        attempt.usage,
        Some(Usage {
            input: 12,
            output: 3,
            cached_input: 2,
            reasoning: 1
        })
    );
    assert!(attempt.cache_reported && attempt.reasoning_reported);
    let diagnostics = capture.lock().unwrap().diagnostics();
    assert_eq!(diagnostics.len(), 2);
    assert_eq!(diagnostics[1].connection_reused, Some(true));
    assert_eq!(diagnostics[1].upstream_status, Some(200));
    assert_eq!(diagnostics[1].duration_us, attempt.elapsed_us);
    assert!(
        !serde_json::to_string(&records)
            .unwrap()
            .contains("PRIVATE_INPUT")
    );
    for record in &records {
        acknowledge(&capture, record);
    }
    observer(wire::Event {
        attempt: 1,
        kind: wire::Kind::Headers {
            status: 503,
            elapsed: Duration::from_secs(10),
            reused: Some(false),
        },
    });
    assert_eq!(capture.lock().unwrap().request.attempts[0], *attempt);
    assert!(capture.lock().unwrap().pending().is_empty());
}

#[tokio::test]
async fn drains_cancelled_and_dropped_streams() {
    for cancelled in [false, true] {
        let capture = capture(None);
        let observer = Capture::observer(&capture);
        let mut operation = Box::pin(wire::scope(observer, async {
            let _attempt = wire::Attempt::start(wire::Transport::Http, None);
            if cancelled {
                futures::future::pending::<Result<(), ()>>().await
            } else {
                let responses: adk_core::LlmResponseStream = Box::pin(futures::stream::pending());
                drop(wire::observe_stream(responses));
                Ok(())
            }
        }));
        if cancelled {
            assert!(futures::poll!(operation.as_mut()).is_pending());
            drop(operation);
        } else {
            operation.await.unwrap();
        }
        let diagnostics = capture.lock().unwrap().diagnostics();
        assert_eq!(diagnostics.len(), 2);
        assert!(matches!(
            diagnostics[1].phase,
            crate::logging::Phase::Cancelled
        ));
        assert!(diagnostics[1].duration_us.is_some());
        assert!(capture.lock().unwrap().diagnostics().is_empty());
        assert_eq!(
            capture.lock().unwrap().pending()[1].attempts[0].outcome,
            Outcome::Cancelled
        );
    }
}
