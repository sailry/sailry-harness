use super::*;

mod cancellation {
    use super::*;

    #[tokio::test]
    async fn normalizes_interrupted_stage_failure() {
        let stop = CancellationToken::new();
        let work = async {
            stop.cancel();
            Err(anyhow::anyhow!("operation was interrupted"))
        };
        let (transition, approval) = advance(work, &stop).await.unwrap();
        assert_eq!(transition.status, RunStatus::Cancelled);
        assert!(transition.effects.is_empty());
        assert!(!approval);
    }

    #[tokio::test]
    async fn preserves_unrelated_stage_failure() {
        let stop = CancellationToken::new();
        let error = advance(async { Err(anyhow::anyhow!("operation failed")) }, &stop)
            .await
            .err()
            .unwrap();
        assert_eq!(error.to_string(), "operation failed");
        assert!(!stop.is_cancelled());
    }

    #[tokio::test]
    async fn keeps_effects_completed_before_stop() {
        let stop = CancellationToken::new();
        let work = async {
            stop.cancel();
            Ok((
                operation::Transition {
                    effects: vec![model::Message::assistant().with_text("Completed").into()],
                    status: RunStatus::Continuing,
                },
                false,
            ))
        };
        let (transition, _) = advance(work, &stop).await.unwrap();
        assert_eq!(transition.status, RunStatus::Continuing);
        assert_eq!(transition.effects.len(), 1);
    }
}
use crate::agent::canonical::{Kind, Message as StoredMessage, Record};
use crate::agent::model::Message;
use sailry_protocol::TurnId;

pub(super) fn session(messages: Vec<(TurnId, Message)>) -> Session {
    let records = messages
        .iter()
        .map(|(turn, message)| {
            let mut record = Record::new(*turn, Kind::Message(StoredMessage::default()));
            record.id = message.id.clone().unwrap();
            record
        })
        .collect();
    Session {
        id: "output-fixture".into(),
        records,
        tool_names: Default::default(),
        conversation: messages.into_iter().map(|(_, message)| message).collect(),
        context: Default::default(),
        capability: None,
        reviewed: false,
        state: Default::default(),
        thinking: None,
        invocation: None,
    }
}

#[test]
fn ignores_previous_answer_without_current_text() {
    let turn = TurnId::new();
    let session = session(vec![
        (
            TurnId::new(),
            Message::assistant()
                .with_generated_id()
                .with_text("Previous answer"),
        ),
        (
            turn,
            Message::user().with_generated_id().with_text("Next task"),
        ),
        (
            turn,
            Message::assistant()
                .with_generated_id()
                .with_thinking("Still thinking", ""),
        ),
        (
            turn,
            Message::assistant()
                .with_generated_id()
                .with_text("Internal continuation")
                .with_visibility(false, true),
        ),
    ]);
    assert_eq!(output(&session, turn), None);
}

#[test]
fn returns_current_visible_answer() {
    let turn = TurnId::new();
    let session = session(vec![
        (
            TurnId::new(),
            Message::assistant()
                .with_generated_id()
                .with_text("Previous answer"),
        ),
        (
            turn,
            Message::assistant()
                .with_generated_id()
                .with_text("Current answer"),
        ),
        (
            TurnId::new(),
            Message::assistant()
                .with_generated_id()
                .with_text("Later answer"),
        ),
    ]);
    assert_eq!(output(&session, turn).as_deref(), Some("Current answer"));
}

#[test]
fn returns_only_current_error() {
    let turn = TurnId::new();
    let session = session(vec![(
        turn,
        Message::assistant()
            .with_generated_id()
            .with_error(model::ErrorKind::Authentication, "Current request failed"),
    )]);
    assert!(last_error(&session, turn).is_some());
    assert!(last_error(&session, TurnId::new()).is_none());
}

mod trigger {
    use super::super::{budget, trigger};

    #[test]
    fn defaults_to_four_fifths() {
        for (context, expected) in [
            (200_000, 160_000),
            (500_000, 400_000),
            (1_050_000, 840_000),
            (4_097, 3_277),
        ] {
            assert_eq!(trigger(context, None, 80), expected);
        }
    }

    #[test]
    fn keeps_explicit_tokens() {
        assert_eq!(trigger(1_050_000, Some(200_000), 65), 200_000);
    }

    #[test]
    fn budget_matches_execution() {
        for (context, configured) in [(1_050_000u32, None), (200_000, Some(120_000))] {
            let reported = budget("fixture", context, 8_000, configured, 80).unwrap();
            let threshold = trigger(u64::from(context), configured, 80) as f64 / f64::from(context);
            assert_eq!(
                (threshold * f64::from(context)).round() as u64,
                reported.trigger_tokens
            );
        }
    }
}
