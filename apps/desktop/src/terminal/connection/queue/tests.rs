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
fn acknowledges_only_success() {
    let terminal = TerminalId::new();
    let command = resize(terminal, 1, 80);
    let mut state = State::default();
    assert!(state.push(command.clone()));
    assert!(state.acknowledged.is_none());
    assert_eq!(state.pop(), Some(command.clone()));
    assert!(state.acknowledged.is_none());
    state.complete(false);
    assert!(state.acknowledged.is_none());
    assert!(state.in_flight.is_none());
    assert!(state.push(command.clone()));
    assert!(state.pop().is_none());
    assert_eq!(
        state.pop_at(state.retry_after.unwrap()),
        Some(command.clone())
    );
    state.complete(true);
    assert!(state.acknowledged.as_ref() == Resize::from(&command).as_ref());
    assert!(state.push(command));
    assert!(state.pop().is_none());
}

#[test]
fn coalesces_bursts_without_losing_input() {
    let terminal = TerminalId::new();
    let mut state = State::default();
    assert!(state.push(resize(terminal, 1, 80)));
    assert_eq!(state.pop(), Some(resize(terminal, 1, 80)));
    let first = input(terminal, "first");
    let second = input(terminal, "second");
    for columns in (1..80).rev() {
        assert!(state.push(resize(terminal, 1, columns)));
        if columns == 70 {
            assert!(state.push(first.clone()));
        }
        if columns == 40 {
            assert!(state.push(second.clone()));
        }
    }
    assert_eq!(state.pending.len(), 3);
    assert!(state.acknowledged.is_none());
    state.complete(true);
    assert_eq!(state.pop(), Some(resize(terminal, 1, 1)));
    state.complete(true);
    assert_eq!(
        state.pop(),
        Some(Command::InputTerminal {
            terminal,
            revision: 1,
            inputs: ["first", "second"]
                .map(|text| sailry_protocol::terminal::Input::Text { text: text.into() })
                .into(),
        })
    );
    state.complete(true);
    assert!(state.pop().is_none());
}

#[test]
fn retains_return_to_acknowledged_size() {
    let terminal = TerminalId::new();
    let mut state = State::default();
    assert!(state.push(resize(terminal, 1, 80)));
    state.pop().unwrap();
    state.complete(true);
    assert!(state.push(resize(terminal, 1, 60)));
    state.pop().unwrap();
    assert!(state.push(resize(terminal, 1, 80)));
    state.complete(true);
    assert_eq!(state.pop(), Some(resize(terminal, 1, 80)));
    state.complete(true);
    assert!(state.push(resize(terminal, 1, 70)));
    assert!(state.push(resize(terminal, 1, 80)));
    assert_eq!(state.pop(), Some(resize(terminal, 1, 80)));
}

#[test]
fn keeps_target_and_revision_identity() {
    let terminal = TerminalId::new();
    let other = TerminalId::new();
    let mut state = State::default();
    assert!(state.push(resize(terminal, 1, 80)));
    assert!(state.push(resize(other, 1, 60)));
    assert!(state.push(resize(terminal, 2, 50)));
    assert!(state.push(resize(terminal, 1, 70)));
    assert_eq!(state.pop(), Some(resize(terminal, 1, 70)));
    state.complete(true);
    assert_eq!(state.pop(), Some(resize(other, 1, 60)));
    state.complete(true);
    assert_eq!(state.pop(), Some(resize(terminal, 2, 50)));
}

#[test]
fn releases_state_when_tail_is_discarded() {
    let terminal = TerminalId::new();
    let mut state = State::default();
    assert!(state.push(resize(terminal, 1, 80)));
    state.pop().unwrap();
    state.complete(true);
    assert!(state.push(input(terminal, "failed")));
    state.pop().unwrap();
    assert!(state.push(resize(terminal, 1, 60)));
    assert!(state.push(input(terminal, "discarded")));
    state.complete(false);
    assert!(state.pending.is_empty());
    assert!(state.in_flight.is_none());
    assert!(state.acknowledged.is_none());
    assert!(state.push(resize(terminal, 1, 60)));
    assert_eq!(state.pop(), Some(resize(terminal, 1, 60)));
    state.complete(true);
    state.discard();
    assert!(state.push(resize(terminal, 1, 60)));
    assert_eq!(state.pop(), Some(resize(terminal, 1, 60)));
}

#[test]
fn retains_newer_geometry_after_failure() {
    let terminal = TerminalId::new();
    let mut state = State::default();
    assert!(state.push(resize(terminal, 1, 80)));
    assert_eq!(state.pop(), Some(resize(terminal, 1, 80)));
    assert!(state.push(resize(terminal, 1, 60)));
    assert!(state.push(input(terminal, "discarded")));
    assert!(state.push(resize(terminal, 1, 40)));
    state.complete(false);
    assert!(state.acknowledged.is_none());
    assert!(state.pop().is_none());
    assert_eq!(
        state.pop_at(state.retry_after.unwrap()),
        Some(resize(terminal, 1, 40))
    );
    state.complete(true);
    assert!(state.pop().is_none());
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

#[test]
fn closes_without_late_acknowledgement() {
    let terminal = TerminalId::new();
    let queue = Queue::default();
    assert!(queue.enqueue(resize(terminal, 1, 80)));
    queue.state.lock().unwrap().pop().unwrap();
    assert!(queue.enqueue(resize(terminal, 1, 60)));
    queue.close();
    queue.complete(true);
    let state = queue.state.lock().unwrap();
    assert!(state.pending.is_empty());
    assert!(state.in_flight.is_none());
    assert!(state.acknowledged.is_none());
    drop(state);
    assert!(!queue.enqueue(resize(terminal, 1, 60)));
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

#[test]
fn drops_pointer_backlog() {
    let terminal = TerminalId::new();
    let mut state = State::default();
    for _ in 0..CAPACITY * 2 {
        assert!(state.push(wheel(terminal)));
    }
    assert_eq!(state.pending.len(), POINTER_BACKLOG);
    for _ in POINTER_BACKLOG..CAPACITY {
        assert!(state.push(input(terminal, "a")));
    }
    assert!(!state.push(input(terminal, "a")));
}

#[test]
fn batches_consecutive_input() {
    let terminal = TerminalId::new();
    let other = TerminalId::new();
    let mut state = State::default();
    for text in ["a", "b", "c"] {
        assert!(state.push(input(terminal, text)));
    }
    assert!(state.push(resize(terminal, 1, 100)));
    assert!(state.push(input(terminal, "d")));
    assert!(state.push(input(other, "e")));
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
    assert_eq!(texts(state.pop()), ["a", "b", "c"]);
    assert_eq!(state.pop(), Some(resize(terminal, 1, 100)));
    state.complete(true);
    assert_eq!(texts(state.pop()), ["d"]);
    assert_eq!(texts(state.pop()), ["e"]);
}

#[test]
fn separates_revisions_and_batch_limits() {
    let terminal = TerminalId::new();
    let first = input(terminal, "first");
    let mut revised = input(terminal, "second");
    if let Command::InputTerminal { revision, .. } = &mut revised {
        *revision = 2;
    }
    let maximum = Command::InputTerminal {
        terminal,
        revision: 1,
        inputs: vec![sailry_protocol::terminal::Input::Focus { focused: true }; MAX_INPUT_BATCH],
    };
    let large = input(terminal, &"x".repeat(MAX_INPUT_BYTES));
    for (left, right) in [
        (first.clone(), revised),
        (maximum, first.clone()),
        (large, first),
    ] {
        let mut state = State::default();
        assert!(state.push(left.clone()));
        assert!(state.push(right.clone()));
        assert_eq!(state.pop(), Some(left));
        state.complete(true);
        assert_eq!(state.pop(), Some(right));
        state.complete(true);
        assert!(state.pop().is_none());
    }
}
