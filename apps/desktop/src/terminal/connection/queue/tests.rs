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
        input: sailry_protocol::terminal::Input::Text { text: text.into() },
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
    assert_eq!(state.pop(), Some(command.clone()));
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
    assert_eq!(state.pop(), Some(first));
    state.complete(true);
    assert_eq!(state.pop(), Some(second));
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
    assert_eq!(state.pop(), Some(resize(terminal, 1, 40)));
    state.complete(true);
    assert!(state.pop().is_none());
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
