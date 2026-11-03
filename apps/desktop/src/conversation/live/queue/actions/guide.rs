use super::{Command, Context, Panel, Queue, TurnId, Window};

pub(super) fn separate(command: &Command, queue: &Queue) -> bool {
    let Command::GuideQueuedTurn { turn, .. } = command else {
        return false;
    };
    queue
        .items
        .iter()
        .any(|item| item.turn == *turn && item.ready && item.guidance.is_none())
}

fn take(
    intent: &mut Option<(TurnId, u64, bool)>,
    queue: &Queue,
    recipient: Option<TurnId>,
    blocked: bool,
) -> Result<Option<Command>, &'static str> {
    if blocked {
        return Ok(None);
    }
    let Some((turn, revision, immediate)) = intent.take() else {
        return Ok(None);
    };
    let recipient = recipient.ok_or("queue_changed")?;
    if !queue.items.iter().any(|item| {
        item.turn == turn
            && item.revision == revision
            && (item.guidance.is_none() || immediate && item.guidance == Some(recipient))
    }) {
        return Err("queue_changed");
    }
    Ok(Some(Command::GuideQueuedTurn {
        turn,
        recipient,
        expected_revision: revision,
        immediate,
    }))
}

impl Panel {
    pub(in crate::conversation::live::queue) fn flush_guide(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let blocked = self.blocked() || self.guidance_blocked;
        match take(
            &mut self.deferred_guide,
            &self.queue,
            self.recipient,
            blocked,
        ) {
            Ok(Some(command)) if self.allows(&command) => self.start(command, window, cx),
            Ok(Some(_)) | Err(_) => {
                self.error = Some("queue_changed");
                cx.notify();
            }
            Ok(None) => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{Command, Queue, TurnId, separate, take};
    use sailry_protocol::conversation::Pending;

    fn queue() -> Queue {
        Queue {
            items: vec![Pending {
                turn: TurnId::new(),
                revision: 4,
                config_revision: 1,
                kind: sailry_protocol::conversation::RunKind::Task,
                ready: true,
                guidance: None,
                notice: false,
                request: None,
                preview: String::new(),
                truncated: false,
                attachments: Vec::new(),
            }],
            ..Default::default()
        }
    }

    #[test]
    fn drains_once_after_pending_clears() {
        let queue = queue();
        let item = &queue.items[0];
        let mut intent = Some((item.turn, item.revision, false));
        let recipient = Some(TurnId::new());
        assert!(
            take(&mut intent, &queue, recipient, true)
                .unwrap()
                .is_none()
        );
        assert!(intent.is_some());
        assert!(
            matches!(take(&mut intent, &queue, recipient, false), Ok(Some(
            Command::GuideQueuedTurn { turn, expected_revision: 4, .. }
        )) if turn == item.turn)
        );
        assert!(
            take(&mut intent, &queue, recipient, false)
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn rejects_changed_or_missing_items() {
        for change in ["revision", "removed", "guided", "recipient"] {
            let mut queue = queue();
            let mut intent = Some((queue.items[0].turn, queue.items[0].revision, false));
            let mut recipient = Some(TurnId::new());
            match change {
                "revision" => queue.items[0].revision += 1,
                "removed" => queue.items.clear(),
                "guided" => queue.items[0].guidance = recipient,
                "recipient" => recipient = None,
                _ => unreachable!(),
            }
            assert!(matches!(
                take(&mut intent, &queue, recipient, false),
                Err("queue_changed")
            ));
            assert!(intent.is_none());
        }
    }

    #[test]
    fn latest_selection_replaces_waiting_intent() {
        let mut queue = queue();
        let mut second = queue.items[0].clone();
        second.turn = TurnId::new();
        queue.items.push(second.clone());
        let mut intent = Some((queue.items[0].turn, queue.items[0].revision, false));
        let recipient = Some(TurnId::new());
        assert!(
            take(&mut intent, &queue, recipient, true)
                .unwrap()
                .is_none()
        );
        intent = Some((second.turn, second.revision, true));
        assert!(
            matches!(take(&mut intent, &queue, recipient, false), Ok(Some(
            Command::GuideQueuedTurn { turn, immediate: true, .. }
        )) if turn == second.turn)
        );
        assert!(intent.is_none());
    }

    #[test]
    fn immediate_selection_keeps_pending_recipient() {
        let mut queue = queue();
        let recipient = TurnId::new();
        queue.items[0].guidance = Some(recipient);
        let item = &queue.items[0];
        let mut intent = Some((item.turn, item.revision, true));
        assert!(matches!(
            take(&mut intent, &queue, Some(recipient), false),
            Ok(Some(Command::GuideQueuedTurn {
                immediate: true,
                ..
            }))
        ));
    }

    #[test]
    fn acknowledges_separate_ready_input() {
        let mut queue = queue();
        let command = Command::GuideQueuedTurn {
            turn: queue.items[0].turn,
            recipient: TurnId::new(),
            expected_revision: queue.items[0].revision,
            immediate: false,
        };
        // Captured capabilities can differ even with the same session revision.
        for revision in [1, 2] {
            queue.items[0].config_revision = revision;
            assert!(separate(&command, &queue));
        }
    }

    #[test]
    fn ignores_other_guidance_outcomes() {
        let queue = queue();
        let turn = queue.items[0].turn;
        let recipient = TurnId::new();
        let command = Command::GuideQueuedTurn {
            turn,
            recipient,
            expected_revision: queue.items[0].revision,
            immediate: false,
        };
        for change in [
            "guided",
            "other-recipient",
            "unready",
            "missing",
            "other-turn",
        ] {
            let mut queue = queue.clone();
            match change {
                "guided" => queue.items[0].guidance = Some(recipient),
                "other-recipient" => queue.items[0].guidance = Some(TurnId::new()),
                "unready" => queue.items[0].ready = false,
                "missing" => queue.items.clear(),
                "other-turn" => queue.items[0].turn = TurnId::new(),
                _ => unreachable!(),
            }
            assert!(!separate(&command, &queue), "{change}");
        }
        assert!(!separate(
            &Command::SendQueuedTurn {
                turn,
                expected_revision: queue.items[0].revision,
            },
            &queue,
        ));
    }
}
