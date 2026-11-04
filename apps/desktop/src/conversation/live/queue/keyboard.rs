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
    fn guides_first_eligible_input_in_order() {
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
        assert!(matches!(
            next(&queue, Some(recipient)),
            Some(Command::GuideQueuedTurn { turn, recipient: actual, expected_revision: 4 })
                if turn == first.turn && actual == recipient
        ));
        let mut reordered = queue.clone();
        reordered.items.swap(3, 4);
        assert!(matches!(
            next(&reordered, Some(recipient)),
            Some(Command::GuideQueuedTurn { turn, .. }) if turn == second.turn
        ));
        assert_eq!(queue.items[3], first);
    }

    #[test]
    fn sends_idle_input_using_item_revision() {
        let first = item();
        let queue = Queue {
            revision: 19,
            paused: true,
            items: vec![first.clone()],
        };
        assert!(matches!(
            next(&queue, None),
            Some(Command::SendQueuedTurn { turn, expected_revision: 4 }) if turn == first.turn
        ));
    }

    #[test]
    fn ignores_empty_and_already_guided_queues() {
        assert!(next(&Queue::default(), None).is_none());
        let mut guided = item();
        guided.guidance = Some(TurnId::new());
        let queue = Queue {
            items: vec![guided],
            ..Default::default()
        };
        assert!(next(&queue, Some(TurnId::new())).is_none());
    }
}
