use super::*;
use futures::FutureExt;
use goose_provider_types::conversation::Conversation;

struct Fixture {
    events: Vec<Result<Event, Error>>,
    pending: bool,
}

#[async_trait]
impl Model for Fixture {
    async fn call(&self, _: Request, _: CancellationToken) -> Result<ModelStream, Error> {
        let events = futures::stream::iter(self.events.clone());
        if self.pending {
            Ok(Box::pin(events.chain(futures::stream::pending())))
        } else {
            Ok(Box::pin(events))
        }
    }
}

async fn responses(events: Vec<Result<Event, Error>>, pending: bool) -> MessageStream {
    provider(Arc::new(Fixture { events, pending }), "fixture".into())
        .stream(&ModelConfig::new("fixture"), "", &[], &[])
        .await
        .unwrap()
}

fn text(value: &str) -> Event {
    Event::TextDelta {
        index: 1,
        text: value.into(),
    }
}

fn reasoning(index: usize, text: Option<&str>, signature: Option<&str>) -> Event {
    Event::ReasoningDelta {
        index,
        text: text.map(str::to_owned),
        signature: signature.map(str::to_owned),
    }
}

#[tokio::test]
async fn joins_deltas_and_finish_metadata() {
    let mut stream = responses(
        vec![
            Ok(text("First")),
            Ok(text(" second")),
            Ok(Event::Usage(Usage {
                output: Some(2),
                ..Usage::default()
            })),
            Ok(Event::Finish(Finish::Length)),
        ],
        false,
    )
    .await;
    let mut conversation = Conversation::empty();
    let mut ids = Vec::new();
    while let Some(chunk) = stream.next().await {
        if let Some(message) = chunk.unwrap().0 {
            ids.push(message.id.clone().expect("response identity"));
            conversation.push(message);
        }
    }
    assert_eq!(ids.len(), 3);
    assert!(ids.iter().all(|id| id == &ids[0]));
    assert_eq!(conversation.len(), 1);
    let message = conversation.last().unwrap();
    assert_eq!(message.as_concat_text(), "First second");
    assert!(message.metadata.output_token_limit_reached);
}

#[tokio::test]
async fn assigns_distinct_response_identities() {
    let adapter = provider(
        Arc::new(Fixture {
            events: vec![Ok(text("Reply")), Ok(Event::Finish(Finish::Stop))],
            pending: false,
        }),
        "fixture".into(),
    );
    let mut conversation = Conversation::empty();
    for _ in 0..2 {
        let mut stream = adapter
            .stream(&ModelConfig::new("fixture"), "", &[], &[])
            .await
            .unwrap();
        while let Some(chunk) = stream.next().await {
            if let Some(message) = chunk.unwrap().0 {
                conversation.push(message);
            }
        }
    }
    assert_eq!(conversation.len(), 2);
    assert_ne!(conversation.messages()[0].id, conversation.messages()[1].id);
    assert_eq!(conversation.last().unwrap().as_concat_text(), "Reply");
}

#[tokio::test]
async fn emits_signed_reasoning_before_pending_read() {
    let mut stream = responses(
        vec![
            Ok(reasoning(0, Some("Plan "), None)),
            Ok(reasoning(0, Some("carefully"), None)),
            Ok(reasoning(0, None, Some("complete-signature"))),
        ],
        true,
    )
    .await;
    let message = stream
        .next()
        .now_or_never()
        .expect("completed reasoning must not wait for another event")
        .unwrap()
        .unwrap()
        .0
        .unwrap();
    let mut conversation = Conversation::empty();
    conversation.push(message);
    assert!(stream.next().now_or_never().is_none());
    drop(stream);
    let [Block::Thinking(thinking)] = conversation.last().unwrap().content.as_slice() else {
        panic!("signed reasoning expected")
    };
    assert_eq!(thinking.thinking, "Plan carefully");
    assert_eq!(thinking.signature, "complete-signature");
}

#[tokio::test]
async fn drains_reasoning_before_failure() {
    for kind in [ErrorKind::Network, ErrorKind::Cancelled] {
        let mut stream = responses(
            vec![
                Ok(reasoning(0, Some("Completed plan"), None)),
                Ok(reasoning(0, None, Some("signature"))),
                Ok(reasoning(1, Some("Partial follow-up"), None)),
                Err(Error::new(kind, "fixture interruption")),
            ],
            false,
        )
        .await;
        let mut conversation = Conversation::empty();
        for _ in 0..2 {
            let (message, _) = stream.next().await.unwrap().unwrap();
            conversation.push(message.unwrap());
        }
        assert!(stream.next().await.unwrap().is_err());
        assert!(stream.next().await.is_none());
        assert_eq!(conversation.len(), 1);
        let [Block::Thinking(completed), Block::Thinking(partial)] =
            conversation.last().unwrap().content.as_slice()
        else {
            panic!("both reasoning blocks must precede the failure")
        };
        assert_eq!(completed.thinking, "Completed plan");
        assert_eq!(completed.signature, "signature");
        assert_eq!(partial.thinking, "Partial follow-up");
        assert!(partial.signature.is_empty());
    }
}
