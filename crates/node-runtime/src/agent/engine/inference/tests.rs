use super::*;
use crate::agent::{
    engine::ConversationEffect,
    model::{Error, ErrorKind, ModelStream, Request, types::ToolCall},
};
use async_trait::async_trait;
use futures::stream;
use sailry_link::CancellationToken;
use std::{
    collections::VecDeque,
    sync::atomic::{AtomicUsize, Ordering},
};
use tokio::sync::mpsc;

struct Provider {
    responses: Mutex<VecDeque<Vec<Result<Event, Error>>>>,
    calls: AtomicUsize,
    hanging: bool,
}

impl Provider {
    fn new(responses: Vec<Vec<Result<Event, Error>>>) -> Self {
        Self {
            responses: Mutex::new(responses.into()),
            calls: AtomicUsize::new(0),
            hanging: false,
        }
    }
}

#[async_trait]
impl model::Model for Provider {
    async fn call(&self, _: Request, _: CancellationToken) -> Result<ModelStream, Error> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        let events = self
            .responses
            .lock()
            .unwrap()
            .pop_front()
            .expect("unexpected inference retry");
        let events = stream::iter(events);
        if self.hanging {
            Ok(Box::pin(events.chain(stream::pending())))
        } else {
            Ok(Box::pin(events))
        }
    }
}

fn emitter() -> (Emitter, mpsc::UnboundedReceiver<Message>) {
    let (sender, receiver) = mpsc::unbounded_channel();
    (Emitter::new(sender, CancellationToken::new()), receiver)
}

fn messages(effects: &[Effect]) -> Vec<&Message> {
    effects
        .iter()
        .filter_map(|effect| match effect {
            Effect::Conversation(ConversationEffect::AppendMessage(message)) => Some(message),
            _ => None,
        })
        .collect()
}

async fn execute(provider: &Provider, successful_tool: bool) -> Vec<Effect> {
    let (emit, _events) = emitter();
    let output = Mutex::default();
    let Outcome::Applied(result) = complete(
        provider,
        "owner",
        &model::Config::default(),
        Request::default(),
        successful_tool,
        &emit,
        &output,
    )
    .await
    .unwrap() else {
        panic!("inference must produce an outcome");
    };
    result.effects
}

#[tokio::test]
async fn bounds_empty_retries_and_keeps_usage() {
    let provider = Provider::new(
        (1..=4)
            .map(|input| {
                vec![
                    Ok(Event::Usage(Usage {
                        input: Some(input),
                        ..Default::default()
                    })),
                    Ok(Event::Finish(Finish::Stop)),
                ]
            })
            .collect(),
    );
    let effects = execute(&provider, false).await;
    assert_eq!(provider.calls.load(Ordering::SeqCst), 4);
    assert_eq!(
        effects
            .iter()
            .filter(|effect| matches!(effect, Effect::Usage(_)))
            .count(),
        4
    );
    let saved = messages(&effects);
    assert_eq!(saved.len(), 1);
    assert_eq!(saved[0].as_concat_text(), EMPTY_RESPONSE);
    assert!(!saved[0].is_user_visible());
    assert!(!saved[0].is_agent_visible());
    assert_eq!(
        saved[0]
            .metadata
            .operation_note("inference", "empty_response"),
        Some(&true.into())
    );
}

#[tokio::test]
async fn successful_tools_allow_empty_completion() {
    let provider = Provider::new(vec![vec![Ok(Event::Finish(Finish::Stop))]]);
    let effects = execute(&provider, true).await;
    assert_eq!(provider.calls.load(Ordering::SeqCst), 1);
    let saved = messages(&effects);
    assert_eq!(saved.len(), 1);
    assert!(saved[0].content.is_empty());
    assert!(!saved[0].is_user_visible());
    assert!(saved[0].is_agent_visible());
}

#[tokio::test]
async fn output_limit_prevents_empty_retries() {
    let provider = Provider::new(vec![vec![Ok(Event::Finish(Finish::Length))]]);
    let effects = execute(&provider, false).await;
    assert_eq!(provider.calls.load(Ordering::SeqCst), 1);
    assert!(messages(&effects)[0].metadata.output_token_limit_reached);
}

#[tokio::test]
async fn retains_partial_output_before_failure() {
    for (kind, expected) in [
        (ErrorKind::Network, ErrorKind::Other),
        (ErrorKind::RateLimited, ErrorKind::RateLimited),
    ] {
        let provider = Provider::new(vec![vec![
            Ok(Event::TextDelta {
                index: 0,
                text: "Partial answer".into(),
            }),
            Ok(Event::Usage(Usage {
                output: Some(3),
                ..Default::default()
            })),
            Err(Error::new(kind, "stream interrupted")),
        ]]);
        let effects = execute(&provider, false).await;
        let saved = messages(&effects);
        assert_eq!(saved.len(), 2);
        assert_eq!(saved[0].as_concat_text(), "Partial answer");
        assert_eq!(saved[1].error_kind(), Some(expected));
        assert!(!saved[1].is_agent_visible());
        assert_eq!(provider.calls.load(Ordering::SeqCst), 1);
    }
}

#[tokio::test]
async fn separates_content_at_shared_indices() {
    let call = ToolCall {
        id: "call".into(),
        name: "read".into(),
        arguments: serde_json::json!({}),
        metadata: None,
        tool_meta: None,
        error: None,
    };
    for reasoning_first in [false, true] {
        let mut incoming = vec![
            Event::TextDelta {
                index: 0,
                text: "Answer".into(),
            },
            Event::ReasoningDelta {
                index: 0,
                text: Some("Consider".into()),
                signature: None,
            },
        ];
        if reasoning_first {
            incoming.reverse();
        }
        incoming.extend([
            Event::TextDelta {
                index: 1,
                text: "Next block".into(),
            },
            Event::ToolCall {
                index: 0,
                call: call.clone(),
            },
            Event::ReasoningDelta {
                index: 0,
                text: Some(" carefully".into()),
                signature: Some("signature".into()),
            },
            Event::RedactedThinking {
                index: 0,
                data: "encrypted".into(),
            },
            Event::TextDelta {
                index: 0,
                text: " continued".into(),
            },
            Event::Finish(Finish::ToolCalls),
        ]);
        let expected = vec![
            Content::Text {
                text: "Answer continued".into(),
            },
            Content::Thinking {
                text: "Consider carefully".into(),
                signature: Some("signature".into()),
            },
            Content::RedactedThinking {
                data: "encrypted".into(),
            },
            Content::ToolCall(call.clone()),
            Content::Text {
                text: "Next block".into(),
            },
        ];
        let provider = Provider::new(vec![incoming.iter().cloned().map(Ok).collect()]);
        let effects = execute(&provider, false).await;
        let saved = messages(&effects);
        assert_eq!(saved.len(), 1);
        assert_eq!(saved[0].content, expected);
        let restored = model::testing::roundtrip(saved[0]).await;
        assert_eq!(restored.content, expected);
        let (collected, _) =
            model::testing::collect(Box::pin(stream::iter(incoming.into_iter().map(Ok))))
                .await
                .unwrap();
        assert_eq!(collected.content, expected);
    }
}

#[tokio::test]
async fn cancellation_preserves_received_output() {
    let mut provider = Provider::new(vec![vec![
        Ok(Event::Usage(Usage {
            output: Some(5),
            ..Default::default()
        })),
        Ok(Event::TextDelta {
            index: 0,
            text: "Received before stop".into(),
        }),
    ]]);
    provider.hanging = true;
    let (emit, mut events) = emitter();
    let pending = Mutex::default();
    let config = model::Config::default();
    {
        let work = complete(
            &provider,
            "owner",
            &config,
            Request::default(),
            false,
            &emit,
            &pending,
        );
        tokio::pin!(work);
        tokio::select! {
            result = &mut work => panic!("model should remain pending: {}", result.is_ok()),
            message = events.recv() => assert_eq!(message.unwrap().as_concat_text(), "Received before stop"),
        }
        emit.cancel_token().cancel();
    }
    let effects = std::mem::take(&mut *pending.lock().unwrap()).take(true);
    assert_eq!(
        messages(&effects)[0].as_concat_text(),
        "Received before stop"
    );
    assert!(matches!(&effects[0], Effect::Usage(usage) if usage.output == Some(5)));
}

#[tokio::test]
async fn assigns_distinct_response_identities() {
    let provider = Provider::new(vec![
        vec![
            Ok(Event::TextDelta {
                index: 0,
                text: "Reply".into(),
            }),
            Ok(Event::Finish(Finish::Stop)),
        ],
        vec![
            Ok(Event::TextDelta {
                index: 0,
                text: "Reply".into(),
            }),
            Ok(Event::Finish(Finish::Stop)),
        ],
    ]);
    let first = execute(&provider, false).await;
    let second = execute(&provider, false).await;
    assert_ne!(messages(&first)[0].id, messages(&second)[0].id);
    assert_eq!(messages(&second)[0].as_concat_text(), "Reply");
}

#[test]
fn joins_text_and_retains_limit_metadata() {
    let (emit, mut events) = emitter();
    let mut output = Output::default();
    output.begin("owner", &model::Config::default());
    for event in [
        Event::TextDelta {
            index: 1,
            text: "First".into(),
        },
        Event::TextDelta {
            index: 1,
            text: " second".into(),
        },
        Event::Usage(Usage {
            output: Some(2),
            ..Default::default()
        }),
        Event::Finish(Finish::Length),
    ] {
        output.receive(event, &emit);
    }
    let effects = output.take(false);
    let saved = messages(&effects);
    assert_eq!(saved.len(), 1);
    assert_eq!(saved[0].as_concat_text(), "First second");
    assert!(saved[0].metadata.output_token_limit_reached);
    let first = events.try_recv().unwrap();
    let second = events.try_recv().unwrap();
    assert_eq!(first.id, saved[0].id);
    assert_eq!(second.id, saved[0].id);
    assert_eq!(first.as_concat_text(), "First");
    assert_eq!(second.as_concat_text(), " second");
}

#[tokio::test]
async fn retains_signed_reasoning_during_pending_read() {
    use futures::FutureExt;
    let mut provider = Provider::new(vec![vec![
        Ok(Event::ReasoningDelta {
            index: 0,
            text: Some("Plan ".into()),
            signature: None,
        }),
        Ok(Event::ReasoningDelta {
            index: 0,
            text: Some("carefully".into()),
            signature: None,
        }),
        Ok(Event::ReasoningDelta {
            index: 0,
            text: None,
            signature: Some("complete-signature".into()),
        }),
    ]]);
    provider.hanging = true;
    let (emit, mut events) = emitter();
    let pending = Mutex::default();
    let config = model::Config::default();
    {
        let work = complete(
            &provider,
            "owner",
            &config,
            Request::default(),
            false,
            &emit,
            &pending,
        );
        tokio::pin!(work);
        assert!(work.as_mut().now_or_never().is_none());
        assert!(events.try_recv().is_ok());
        emit.cancel_token().cancel();
    }
    let effects = std::mem::take(&mut *pending.lock().unwrap()).take(true);
    let saved = messages(&effects);
    let [Content::Thinking { text, signature }] = saved[0].content.as_slice() else {
        panic!("signed reasoning expected")
    };
    assert_eq!(text, "Plan carefully");
    assert_eq!(signature.as_deref(), Some("complete-signature"));
}

#[tokio::test]
async fn retains_reasoning_before_failure() {
    for kind in [ErrorKind::Network, ErrorKind::Cancelled] {
        let provider = Provider::new(vec![vec![
            Ok(Event::ReasoningDelta {
                index: 0,
                text: Some("Completed plan".into()),
                signature: None,
            }),
            Ok(Event::ReasoningDelta {
                index: 0,
                text: None,
                signature: Some("signature".into()),
            }),
            Ok(Event::ReasoningDelta {
                index: 1,
                text: Some("Partial follow-up".into()),
                signature: None,
            }),
            Err(Error::new(kind, "fixture interruption")),
        ]]);
        let effects = execute(&provider, false).await;
        let saved = messages(&effects);
        assert_eq!(saved.len(), 2);
        let [
            Content::Thinking {
                text: completed,
                signature,
            },
            Content::Thinking {
                text: partial,
                signature: absent,
            },
        ] = saved[0].content.as_slice()
        else {
            panic!("both reasoning blocks must precede the failure")
        };
        assert_eq!(completed, "Completed plan");
        assert_eq!(signature.as_deref(), Some("signature"));
        assert_eq!(partial, "Partial follow-up");
        assert!(absent.is_none());
        assert!(saved[1].error_kind().is_some());
    }
}

#[test]
fn preserves_reasoning_blocks_and_completed_calls() {
    let (emit, mut events) = emitter();
    let mut output = Output::default();
    output.begin("owner", &model::Config::default());
    for event in [
        Event::ReasoningDelta {
            index: 0,
            text: Some("First".into()),
            signature: None,
        },
        Event::ReasoningDelta {
            index: 0,
            text: Some(" thought".into()),
            signature: Some("sig-".into()),
        },
        Event::ReasoningDelta {
            index: 0,
            text: None,
            signature: Some("one".into()),
        },
        Event::ReasoningDelta {
            index: 1,
            text: Some("Second".into()),
            signature: Some("sig-two".into()),
        },
        Event::ToolCallDelta {
            index: 2,
            id: Some("incomplete".into()),
            name: Some("write".into()),
            arguments: "{".into(),
        },
        Event::ToolCall {
            index: 3,
            call: ToolCall {
                id: "complete".into(),
                name: "read".into(),
                arguments: serde_json::json!({}),
                metadata: Some(
                    serde_json::json!({"signature":"call"})
                        .as_object()
                        .unwrap()
                        .clone(),
                ),
                tool_meta: None,
                error: None,
            },
        },
        Event::TextDelta {
            index: 4,
            text: "Answer".into(),
        },
        Event::Usage(Usage {
            output: Some(1),
            ..Default::default()
        }),
        Event::Usage(Usage {
            output: Some(9),
            ..Default::default()
        }),
        Event::Finish(Finish::ToolCalls),
    ] {
        output.receive(event, &emit);
    }
    let effects = output.take(false);
    let saved = messages(&effects);
    assert_eq!(saved.len(), 1);
    assert_eq!(saved[0].content.len(), 4);
    assert!(
        matches!(&saved[0].content[0], Content::Thinking { text, signature }
        if text == "First thought" && signature.as_deref() == Some("sig-one"))
    );
    assert!(
        matches!(&saved[0].content[1], Content::Thinking { text, signature }
        if text == "Second" && signature.as_deref() == Some("sig-two"))
    );
    assert!(matches!(&saved[0].content[2], Content::ToolCall(call) if call.id == "complete"));
    assert!(matches!(&effects[0], Effect::Usage(usage) if usage.output == Some(9)));
    while let Ok(partial) = events.try_recv() {
        assert_eq!(partial.id, saved[0].id);
    }
}

mod projection {
    use super::*;

    #[test]
    fn filters_audiences_without_mutating_history() {
        use rmcp::model::{CallToolRequestParams, CallToolResult};
        let result: CallToolResult = serde_json::from_value(serde_json::json!({
            "content":[
                {"type":"text","text":"Only the user sees this","annotations":{"audience":["user"]}},
                {"type":"text","text":"Only the model sees this","annotations":{"audience":["assistant"]}},
                {"type":"text","text":"Both audiences","annotations":{"audience":["user","assistant"]}},
                {"type":"text","text":"Unannotated"},
                {"type":"text","text":"No audience","annotations":{"audience":[]}},
                {"type":"image","data":"AQID","mimeType":"image/png","annotations":{"audience":["user"]}},
                {"type":"audio","data":"AQID","mimeType":"audio/wav","annotations":{"audience":["user"]}},
                {"type":"resource","resource":{"uri":"file:///saved","text":"Private resource"},"annotations":{"audience":["user"]}},
                {"type":"resource_link","uri":"file:///saved","name":"Private link","annotations":{"audience":["user"]}}
            ],
            "isError":false
        }))
        .unwrap();
        let messages = vec![
            Message::user().with_text("Read the result"),
            Message::user().with_content(Content::Image {
                media_type: "image/png".into(),
                data: model::types::Image::Inline {
                    data_base64: "AQID".into(),
                },
                annotations: Some(serde_json::json!({"audience":["user"]})),
            }),
            Message::assistant()
                .with_tool_request("read", Ok(CallToolRequestParams::new("read_file"))),
            Message::user().with_tool_response("read", Ok(result)),
        ];
        let original = messages.clone();
        let visible = super::super::projection::visible(&messages).unwrap();
        assert_eq!(visible.len(), 3);
        let request = model::Config::default().request(
            String::new(),
            super::super::projection::prepare(visible, "owner"),
            Vec::new(),
        );
        let Content::ToolResult(response) = &request.messages.last().unwrap().content[0] else {
            panic!("tool response expected")
        };
        let result = response.result.as_ref().unwrap();
        assert_eq!(
            result
                .content
                .iter()
                .map(|content| content.as_text().unwrap().text.as_str())
                .collect::<Vec<_>>(),
            ["Only the model sees this", "Both audiences", "Unannotated"]
        );
        assert_eq!(messages, original);
    }

    #[test]
    fn retains_empty_filtered_tool_responses() {
        let result = serde_json::from_value(serde_json::json!({
            "content":[{"type":"text","text":"User only","annotations":{"audience":["user"]}}],
            "isError":false
        }))
        .unwrap();
        let messages = vec![
            Message::user().with_text("Read the result"),
            Message::assistant().with_tool_request(
                "read",
                Ok(rmcp::model::CallToolRequestParams::new("read_file")),
            ),
            Message::user().with_tool_response("read", Ok(result)),
        ];
        let visible = super::super::projection::visible(&messages).unwrap();
        assert_eq!(visible.len(), 3);
        let Content::ToolResult(response) = &visible[2].content[0] else {
            panic!("empty tool response expected")
        };
        assert_eq!(response.id, "read");
        assert!(response.result.as_ref().unwrap().content.is_empty());
        assert!(super::super::projection::applies(&messages));
        let prepared = super::super::projection::prepare(visible, "owner");
        assert_eq!(prepared.len(), 3);
        assert!(matches!(&prepared[1].content[0], Content::ToolCall(call) if call.id == "read"));
        let Content::ToolResult(response) = &prepared[2].content[0] else {
            panic!("paired tool response expected")
        };
        assert_eq!(response.id, "read");
        assert_eq!(
            response.result.as_ref().unwrap().content[0]
                .as_text()
                .unwrap()
                .text,
            "(empty result)"
        );
    }

    #[test]
    fn removes_old_orphans_without_mutating_history() {
        let messages = vec![
            Message::user().with_text("Previous task"),
            Message::assistant()
                .with_text("Partial")
                .with_tool_request("old", Ok(rmcp::model::CallToolRequestParams::new("read"))),
            Message::user().with_text("Current task"),
        ];
        let before = messages.clone();
        let visible = super::super::projection::visible(&messages).unwrap();
        let projected = super::super::projection::prepare(visible, "owner");
        assert_eq!(projected.len(), 3);
        assert!(
            !projected
                .iter()
                .flat_map(|message| &message.content)
                .any(|content| matches!(content, Content::ToolCall(_)))
        );
        assert_eq!(messages, before);
    }

    #[test]
    fn removes_foreign_signed_reasoning() {
        let mut response = Message::assistant()
            .with_thinking("foreign", "signature")
            .with_content(Content::RedactedThinking {
                data: "encrypted".into(),
            })
            .with_thinking("unsigned", "")
            .with_text("Answer");
        response.metadata.inference = Some(model::types::Inference {
            provider: "other".into(),
            ..Default::default()
        });
        let projected = super::super::projection::prepare(
            vec![
                Message::user().with_text("Task"),
                response,
                Message::user().with_text("Continue"),
            ],
            "owner",
        );
        assert_eq!(projected[1].content.len(), 2);
        assert!(
            matches!(&projected[1].content[0], Content::Thinking { text, .. } if text == "unsigned")
        );
    }

    #[test]
    fn empty_assistant_ends_inference() {
        let messages = vec![
            Message::user().with_text("Task"),
            Message::assistant().with_visibility(false, true),
        ];
        assert!(!super::super::projection::applies(&messages));
    }
}
