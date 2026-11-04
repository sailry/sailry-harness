use sailry_protocol::{
    Command, TurnId,
    conversation::{Queue, RunKind},
};

pub(super) fn next(queue: &Queue, recipient: Option<TurnId>) -> Option<Command> {
    let item = queue
        .items
        .iter()
        .find(|item| !item.notice && item.kind == RunKind::Task && item.guidance.is_none())?;
    Some(match recipient {
        Some(recipient) => Command::GuideQueuedTurn {
            turn: item.turn,
            recipient,
            expected_revision: item.revision,
        },
        None => Command::SendQueuedTurn {
            turn: item.turn,
            expected_revision: item.revision,
        },
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use sailry_protocol::conversation::Pending;

    fn item() -> Pending {
        Pending {
            turn: TurnId::new(),
            notice: false,
            request: None,
            kind: RunKind::Task,
            revision: 4,
            config_revision: 2,
            ready: true,
            guidance: None,
            preview: "Queued input".into(),
            truncated: false,
            attachments: Vec::new(),
        }
    }

    #[test]
    fn input_order_and_revisions() {
        for (scenario, check) in [
            (
                "guides_first_eligible_input_in_order",
                (|_scenario: &str| {
                    let recipient = TurnId::new();
                    let mut notice = item();
                    notice.notice = true;
                    let mut compaction = item();
                    compaction.kind = RunKind::Compaction;
                    let mut guided = item();
                    guided.guidance = Some(recipient);
                    let first = item();
                    let second = item();
                    let queue = Queue {
                        revision: 17,
                        paused: true,
                        items: vec![notice, compaction, guided, first.clone(), second.clone()],
                    };
                    assert!(
                        matches!(
                            next(&queue, Some(recipient)),
                            Some(Command::GuideQueuedTurn { turn, recipient: actual, expected_revision: 4 })
                                if turn == first.turn && actual == recipient
                        ),
                        "guides first eligible input in order"
                    );
                    let mut reordered = queue.clone();
                    reordered.items.swap(3, 4);
                    assert!(
                        matches!(
                            next(&reordered, Some(recipient)),
                            Some(Command::GuideQueuedTurn { turn, .. }) if turn == second.turn
                        ),
                        "guides first eligible input in order"
                    );
                    assert_eq!(
                        queue.items[3], first,
                        "guides first eligible input in order"
                    );
                }) as fn(&str),
            ),
            (
                "sends_idle_input_using_item_revision",
                (|_scenario: &str| {
                    let first = item();
                    let queue = Queue {
                        revision: 19,
                        paused: true,
                        items: vec![first.clone()],
                    };
                    assert!(
                        matches!(
                            next(&queue, None),
                            Some(Command::SendQueuedTurn { turn, expected_revision: 4 }) if turn == first.turn
                        ),
                        "sends idle input using item revision"
                    );
                }),
            ),
            (
                "ignores_empty_and_already_guided_queues",
                (|_scenario: &str| {
                    assert!(
                        next(&Queue::default(), None).is_none(),
                        "ignores empty and already guided queues"
                    );
                    let mut guided = item();
                    guided.guidance = Some(TurnId::new());
                    let queue = Queue {
                        items: vec![guided],
                        ..Default::default()
                    };
                    assert!(
                        next(&queue, Some(TurnId::new())).is_none(),
                        "ignores empty and already guided queues"
                    );
                }),
            ),
        ] {
            eprintln!("case: {scenario}");
            check(scenario);
        }
    }
}
