use super::*;

fn resize(terminal: TerminalId, revision: u64, columns: u16) -> Command {
    Command::ResizeTerminal {
        terminal,
        revision,
        viewport: Viewport {
            columns,
            rows: 24,
            pixel_width: u32::from(columns) * 8,
            pixel_height: 384,
        },
    }
}

fn input(terminal: TerminalId, text: &str) -> Command {
    Command::InputTerminal {
        terminal,
        revision: 1,
        inputs: vec![sailry_protocol::terminal::Input::Text { text: text.into() }],
    }
}

#[test]
fn ordering_and_acknowledgement() {
    for (scenario, check) in [
        (
            "acknowledges_only_success",
            (|_scenario: &str| {
                let terminal = TerminalId::new();
                let command = resize(terminal, 1, 80);
                let mut state = State::default();
                assert!(state.push(command.clone()), "acknowledges only success");
                assert!(state.acknowledged.is_none(), "acknowledges only success");
                assert_eq!(
                    state.pop(),
                    Some(command.clone()),
                    "acknowledges only success"
                );
                assert!(state.acknowledged.is_none(), "acknowledges only success");
                state.complete(false);
                assert!(state.acknowledged.is_none(), "acknowledges only success");
                assert!(state.in_flight.is_none(), "acknowledges only success");
                assert!(state.push(command.clone()), "acknowledges only success");
                assert!(state.pop().is_none(), "acknowledges only success");
                assert_eq!(
                    state.pop_at(state.retry_after.unwrap()),
                    Some(command.clone()),
                    "acknowledges only success"
                );
                state.complete(true);
                assert!(
                    state.acknowledged.as_ref() == Resize::from(&command).as_ref(),
                    "acknowledges only success"
                );
                assert!(state.push(command), "acknowledges only success");
                assert!(state.pop().is_none(), "acknowledges only success");
            }) as fn(&str),
        ),
        (
            "coalesces_bursts_without_losing_input",
            (|_scenario: &str| {
                let terminal = TerminalId::new();
                let mut state = State::default();
                assert!(
                    state.push(resize(terminal, 1, 80)),
                    "coalesces bursts without losing input"
                );
                assert_eq!(
                    state.pop(),
                    Some(resize(terminal, 1, 80)),
                    "coalesces bursts without losing input"
                );
                let first = input(terminal, "first");
                let second = input(terminal, "second");
                for columns in (1..80).rev() {
                    assert!(
                        state.push(resize(terminal, 1, columns)),
                        "coalesces bursts without losing input"
                    );
                    if columns == 70 {
                        assert!(
                            state.push(first.clone()),
                            "coalesces bursts without losing input"
                        );
                    }
                    if columns == 40 {
                        assert!(
                            state.push(second.clone()),
                            "coalesces bursts without losing input"
                        );
                    }
                }
                assert_eq!(
                    state.pending.len(),
                    3,
                    "coalesces bursts without losing input"
                );
                assert!(
                    state.acknowledged.is_none(),
                    "coalesces bursts without losing input"
                );
                state.complete(true);
                assert_eq!(
                    state.pop(),
                    Some(resize(terminal, 1, 1)),
                    "coalesces bursts without losing input"
                );
                state.complete(true);
                assert_eq!(
                    state.pop(),
                    Some(Command::InputTerminal {
                        terminal,
                        revision: 1,
                        inputs: ["first", "second"]
                            .map(|text| sailry_protocol::terminal::Input::Text {
                                text: text.into()
                            })
                            .into(),
                    }),
                    "coalesces bursts without losing input"
                );
                state.complete(true);
                assert!(
                    state.pop().is_none(),
                    "coalesces bursts without losing input"
                );
            }),
        ),
        (
            "retains_return_to_acknowledged_size",
            (|_scenario: &str| {
                let terminal = TerminalId::new();
                let mut state = State::default();
                assert!(
                    state.push(resize(terminal, 1, 80)),
                    "retains return to acknowledged size"
                );
                state.pop().unwrap();
                state.complete(true);
                assert!(
                    state.push(resize(terminal, 1, 60)),
                    "retains return to acknowledged size"
                );
                state.pop().unwrap();
                assert!(
                    state.push(resize(terminal, 1, 80)),
                    "retains return to acknowledged size"
                );
                state.complete(true);
                assert_eq!(
                    state.pop(),
                    Some(resize(terminal, 1, 80)),
                    "retains return to acknowledged size"
                );
                state.complete(true);
                assert!(
                    state.push(resize(terminal, 1, 70)),
                    "retains return to acknowledged size"
                );
                assert!(
                    state.push(resize(terminal, 1, 80)),
                    "retains return to acknowledged size"
                );
                assert_eq!(
                    state.pop(),
                    Some(resize(terminal, 1, 80)),
                    "retains return to acknowledged size"
                );
            }),
        ),
        (
            "keeps_target_and_revision_identity",
            (|_scenario: &str| {
                let terminal = TerminalId::new();
                let other = TerminalId::new();
                let mut state = State::default();
                assert!(
                    state.push(resize(terminal, 1, 80)),
                    "keeps target and revision identity"
                );
                assert!(
                    state.push(resize(other, 1, 60)),
                    "keeps target and revision identity"
                );
                assert!(
                    state.push(resize(terminal, 2, 50)),
                    "keeps target and revision identity"
                );
                assert!(
                    state.push(resize(terminal, 1, 70)),
                    "keeps target and revision identity"
                );
                assert_eq!(
                    state.pop(),
                    Some(resize(terminal, 1, 70)),
                    "keeps target and revision identity"
                );
                state.complete(true);
                assert_eq!(
                    state.pop(),
                    Some(resize(other, 1, 60)),
                    "keeps target and revision identity"
                );
                state.complete(true);
                assert_eq!(
                    state.pop(),
                    Some(resize(terminal, 2, 50)),
                    "keeps target and revision identity"
                );
            }),
        ),
        (
            "releases_state_when_tail_is_discarded",
            (|_scenario: &str| {
                let terminal = TerminalId::new();
                let mut state = State::default();
                assert!(
                    state.push(resize(terminal, 1, 80)),
                    "releases state when tail is discarded"
                );
                state.pop().unwrap();
                state.complete(true);
                assert!(
                    state.push(input(terminal, "failed")),
                    "releases state when tail is discarded"
                );
                state.pop().unwrap();
                assert!(
                    state.push(resize(terminal, 1, 60)),
                    "releases state when tail is discarded"
                );
                assert!(
                    state.push(input(terminal, "discarded")),
                    "releases state when tail is discarded"
                );
                state.complete(false);
                assert!(
                    state.pending.is_empty(),
                    "releases state when tail is discarded"
                );
                assert!(
                    state.in_flight.is_none(),
                    "releases state when tail is discarded"
                );
                assert!(
                    state.acknowledged.is_none(),
                    "releases state when tail is discarded"
                );
                assert!(
                    state.push(resize(terminal, 1, 60)),
                    "releases state when tail is discarded"
                );
                assert_eq!(
                    state.pop(),
                    Some(resize(terminal, 1, 60)),
                    "releases state when tail is discarded"
                );
                state.complete(true);
                state.discard();
                assert!(
                    state.push(resize(terminal, 1, 60)),
                    "releases state when tail is discarded"
                );
                assert_eq!(
                    state.pop(),
                    Some(resize(terminal, 1, 60)),
                    "releases state when tail is discarded"
                );
            }),
        ),
        (
            "retains_newer_geometry_after_failure",
            (|_scenario: &str| {
                let terminal = TerminalId::new();
                let mut state = State::default();
                assert!(
                    state.push(resize(terminal, 1, 80)),
                    "retains newer geometry after failure"
                );
                assert_eq!(
                    state.pop(),
                    Some(resize(terminal, 1, 80)),
                    "retains newer geometry after failure"
                );
                assert!(
                    state.push(resize(terminal, 1, 60)),
                    "retains newer geometry after failure"
                );
                assert!(
                    state.push(input(terminal, "discarded")),
                    "retains newer geometry after failure"
                );
                assert!(
                    state.push(resize(terminal, 1, 40)),
                    "retains newer geometry after failure"
                );
                state.complete(false);
                assert!(
                    state.acknowledged.is_none(),
                    "retains newer geometry after failure"
                );
                assert!(
                    state.pop().is_none(),
                    "retains newer geometry after failure"
                );
                assert_eq!(
                    state.pop_at(state.retry_after.unwrap()),
                    Some(resize(terminal, 1, 40)),
                    "retains newer geometry after failure"
                );
                state.complete(true);
                assert!(
                    state.pop().is_none(),
                    "retains newer geometry after failure"
                );
            }),
        ),
        (
            "closes_without_late_acknowledgement",
            (|_scenario: &str| {
                let terminal = TerminalId::new();
                let queue = Queue::default();
                assert!(
                    queue.enqueue(resize(terminal, 1, 80)),
                    "closes without late acknowledgement"
                );
                queue.state.lock().unwrap().pop().unwrap();
                assert!(
                    queue.enqueue(resize(terminal, 1, 60)),
                    "closes without late acknowledgement"
                );
                queue.close();
                queue.complete(true);
                let state = queue.state.lock().unwrap();
                assert!(
                    state.pending.is_empty(),
                    "closes without late acknowledgement"
                );
                assert!(
                    state.in_flight.is_none(),
                    "closes without late acknowledgement"
                );
                assert!(
                    state.acknowledged.is_none(),
                    "closes without late acknowledgement"
                );
                drop(state);
                assert!(
                    !queue.enqueue(resize(terminal, 1, 60)),
                    "closes without late acknowledgement"
                );
            }),
        ),
        (
            "drops_pointer_backlog",
            (|_scenario: &str| {
                let terminal = TerminalId::new();
                let mut state = State::default();
                for _ in 0..CAPACITY * 2 {
                    assert!(state.push(wheel(terminal)), "drops pointer backlog");
                }
                assert_eq!(
                    state.pending.len(),
                    POINTER_BACKLOG,
                    "drops pointer backlog"
                );
                for _ in POINTER_BACKLOG..CAPACITY {
                    assert!(state.push(input(terminal, "a")), "drops pointer backlog");
                }
                assert!(!state.push(input(terminal, "a")), "drops pointer backlog");
            }),
        ),
        (
            "batches_consecutive_input",
            (|_scenario: &str| {
                let terminal = TerminalId::new();
                let other = TerminalId::new();
                let mut state = State::default();
                for text in ["a", "b", "c"] {
                    assert!(
                        state.push(input(terminal, text)),
                        "batches consecutive input"
                    );
                }
                assert!(
                    state.push(resize(terminal, 1, 100)),
                    "batches consecutive input"
                );
                assert!(
                    state.push(input(terminal, "d")),
                    "batches consecutive input"
                );
                assert!(state.push(input(other, "e")), "batches consecutive input");
                let texts = |command: Option<Command>| match command {
                    Some(Command::InputTerminal { inputs, .. }) => inputs
                        .into_iter()
                        .map(|input| match input {
                            sailry_protocol::terminal::Input::Text { text } => text,
                            _ => unreachable!(),
                        })
                        .collect::<Vec<_>>(),
                    _ => panic!("input batch expected"),
                };
                assert_eq!(
                    texts(state.pop()),
                    ["a", "b", "c"],
                    "batches consecutive input"
                );
                assert_eq!(
                    state.pop(),
                    Some(resize(terminal, 1, 100)),
                    "batches consecutive input"
                );
                state.complete(true);
                assert_eq!(texts(state.pop()), ["d"], "batches consecutive input");
                assert_eq!(texts(state.pop()), ["e"], "batches consecutive input");
            }),
        ),
        (
            "separates_revisions_and_batch_limits",
            (|_scenario: &str| {
                let terminal = TerminalId::new();
                let first = input(terminal, "first");
                let mut revised = input(terminal, "second");
                if let Command::InputTerminal { revision, .. } = &mut revised {
                    *revision = 2;
                }
                let maximum = Command::InputTerminal {
                    terminal,
                    revision: 1,
                    inputs: vec![
                        sailry_protocol::terminal::Input::Focus { focused: true };
                        MAX_INPUT_BATCH
                    ],
                };
                let large = input(terminal, &"x".repeat(MAX_INPUT_BYTES));
                for (left, right) in [
                    (first.clone(), revised),
                    (maximum, first.clone()),
                    (large, first),
                ] {
                    let mut state = State::default();
                    assert!(
                        state.push(left.clone()),
                        "separates revisions and batch limits"
                    );
                    assert!(
                        state.push(right.clone()),
                        "separates revisions and batch limits"
                    );
                    assert_eq!(
                        state.pop(),
                        Some(left),
                        "separates revisions and batch limits"
                    );
                    state.complete(true);
                    assert_eq!(
                        state.pop(),
                        Some(right),
                        "separates revisions and batch limits"
                    );
                    state.complete(true);
                    assert!(
                        state.pop().is_none(),
                        "separates revisions and batch limits"
                    );
                }
            }),
        ),
    ] {
        eprintln!("case: {scenario}");
        check(scenario);
    }
}

#[tokio::test(start_paused = true)]
async fn delays_failed_resize_bursts() {
    let terminal = TerminalId::new();
    let queue = Queue::default();
    assert!(queue.enqueue(resize(terminal, 1, 80)));
    assert_eq!(queue.next().await, resize(terminal, 1, 80));
    queue.complete(false);
    for columns in 40..80 {
        assert!(queue.enqueue(resize(terminal, 1, columns)));
    }
    let next = queue.next();
    tokio::pin!(next);
    assert!(
        tokio::time::timeout(RETRY_DELAY / 2, next.as_mut())
            .await
            .is_err()
    );
    assert_eq!(next.await, resize(terminal, 1, 79));
    queue.complete(true);
    assert!(queue.enqueue(resize(terminal, 1, 79)));
    assert!(queue.state.lock().unwrap().pending.is_empty());
}

fn wheel(terminal: TerminalId) -> Command {
    use sailry_protocol::terminal::{Input, Modifiers, MouseAction, MouseButton, MouseEvent};
    Command::InputTerminal {
        terminal,
        revision: 1,
        inputs: vec![Input::Mouse {
            event: MouseEvent {
                action: MouseAction::Press,
                button: Some(MouseButton::WheelDown),
                modifiers: Modifiers::default(),
                column: 0,
                row: 0,
                x: 0,
                y: 0,
            },
        }],
    }
}
