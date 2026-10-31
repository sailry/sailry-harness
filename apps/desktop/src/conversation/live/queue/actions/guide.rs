use super::{Command, Context, Panel, Queue, TurnId, Window};

fn take(
    intent: &mut Option<(TurnId, u64)>,
    queue: &Queue,
    recipient: Option<TurnId>,
    blocked: bool,
) -> Result<Option<Command>, &'static str> {
    if blocked {
        return Ok(None);
    }
    let Some((turn, revision)) = intent.take() else {
        return Ok(None);
    };
    let recipient = recipient.ok_or("queue_changed")?;
    if !queue
        .items
        .iter()
        .any(|item| item.turn == turn && item.revision == revision && item.guidance.is_none())
    {
        return Err("queue_changed");
    }
    Ok(Some(Command::GuideQueuedTurn {
        turn,
        recipient,
        expected_revision: revision,
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
    use super::{Command, Queue, TurnId, take};
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
        let mut intent = Some((item.turn, item.revision));
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
            let mut intent = Some((queue.items[0].turn, queue.items[0].revision));
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
        let mut intent = Some((queue.items[0].turn, queue.items[0].revision));
        let recipient = Some(TurnId::new());
        assert!(
            take(&mut intent, &queue, recipient, true)
                .unwrap()
                .is_none()
        );
        intent = Some((second.turn, second.revision));
        assert!(
            matches!(take(&mut intent, &queue, recipient, false), Ok(Some(
            Command::GuideQueuedTurn { turn, .. }
        )) if turn == second.turn)
        );
        assert!(intent.is_none());
    }
}
