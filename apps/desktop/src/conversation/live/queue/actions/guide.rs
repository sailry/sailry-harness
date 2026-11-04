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
    if !queue.items.iter().any(|item| {
        item.turn == turn
            && item.revision == revision
            && (item.guidance.is_none() || item.guidance == Some(recipient))
    }) {
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
    fn pending_intents_and_outcomes() {
        for (scenario, check) in [
            (
                "drains_once_after_pending_clears",
                (|_scenario: &str| {
                    let queue = queue();
                    let item = &queue.items[0];
                    let mut intent = Some((item.turn, item.revision));
                    let recipient = Some(TurnId::new());
                    assert!(
                        take(&mut intent, &queue, recipient, true)
                            .unwrap()
                            .is_none(),
                        "drains once after pending clears"
                    );
                    assert!(intent.is_some(), "drains once after pending clears");
                    assert!(
                        matches!(take(&mut intent, &queue, recipient, false), Ok(Some(
                    Command::GuideQueuedTurn { turn, expected_revision: 4, .. }
                )) if turn == item.turn),
                        "drains once after pending clears"
                    );
                    assert!(
                        take(&mut intent, &queue, recipient, false)
                            .unwrap()
                            .is_none(),
                        "drains once after pending clears"
                    );
                }) as fn(&str),
            ),
            (
                "rejects_changed_or_missing_items",
                (|_scenario: &str| {
                    for change in ["revision", "removed", "guided", "recipient"] {
                        let mut queue = queue();
                        let mut intent = Some((queue.items[0].turn, queue.items[0].revision));
                        let mut recipient = Some(TurnId::new());
                        match change {
                            "revision" => queue.items[0].revision += 1,
                            "removed" => queue.items.clear(),
                            "guided" => queue.items[0].guidance = Some(TurnId::new()),
                            "recipient" => recipient = None,
                            _ => unreachable!(),
                        }
                        assert!(
                            matches!(
                                take(&mut intent, &queue, recipient, false),
                                Err("queue_changed")
                            ),
                            "rejects changed or missing items"
                        );
                        assert!(intent.is_none(), "rejects changed or missing items");
                    }
                }),
            ),
            (
                "latest_selection_replaces_waiting_intent",
                (|_scenario: &str| {
                    let mut queue = queue();
                    let mut second = queue.items[0].clone();
                    second.turn = TurnId::new();
                    queue.items.push(second.clone());
                    let mut intent = Some((queue.items[0].turn, queue.items[0].revision));
                    let recipient = Some(TurnId::new());
                    assert!(
                        take(&mut intent, &queue, recipient, true)
                            .unwrap()
                            .is_none(),
                        "latest selection replaces waiting intent"
                    );
                    intent = Some((second.turn, second.revision));
                    assert!(
                        matches!(take(&mut intent, &queue, recipient, false), Ok(Some(
                    Command::GuideQueuedTurn { turn, expected_revision: 4, .. }
                )) if turn == second.turn),
                        "latest selection replaces waiting intent"
                    );
                    assert!(intent.is_none(), "latest selection replaces waiting intent");
                }),
            ),
            (
                "selection_keeps_pending_recipient",
                (|_scenario: &str| {
                    let mut queue = queue();
                    let recipient = TurnId::new();
                    queue.items[0].guidance = Some(recipient);
                    let item = &queue.items[0];
                    let mut intent = Some((item.turn, item.revision));
                    assert!(
                        matches!(
                            take(&mut intent, &queue, Some(recipient), false),
                            Ok(Some(Command::GuideQueuedTurn {
                                recipient: selected,
                                ..
                            })) if selected == recipient
                        ),
                        "selection keeps pending recipient"
                    );
                }),
            ),
            (
                "acknowledges_separate_ready_input",
                (|_scenario: &str| {
                    let mut queue = queue();
                    let command = Command::GuideQueuedTurn {
                        turn: queue.items[0].turn,
                        recipient: TurnId::new(),
                        expected_revision: queue.items[0].revision,
                    };
                    // Captured capabilities can differ even with the same session revision.
                    for revision in [1, 2] {
                        queue.items[0].config_revision = revision;
                        assert!(
                            separate(&command, &queue),
                            "acknowledges separate ready input"
                        );
                    }
                }),
            ),
            (
                "ignores_other_guidance_outcomes",
                (|_scenario: &str| {
                    let queue = queue();
                    let turn = queue.items[0].turn;
                    let recipient = TurnId::new();
                    let command = Command::GuideQueuedTurn {
                        turn,
                        recipient,
                        expected_revision: queue.items[0].revision,
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
                        assert!(
                            !separate(&command, &queue),
                            "ignores other guidance outcomes: {change}"
                        );
                    }
                    assert!(
                        !separate(
                            &Command::SendQueuedTurn {
                                turn,
                                expected_revision: queue.items[0].revision,
                            },
                            &queue,
                        ),
                        "ignores other guidance outcomes"
                    );
                }),
            ),
        ] {
            eprintln!("case: {scenario}");
            check(scenario);
        }
    }
}
