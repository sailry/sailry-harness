use super::*;
use core::prelude::v1::test;
use sailry_client::conversation::tools::{Call, Reference, State};
use sailry_protocol::conversation::{Child, Draft, Page, Part, Run, RunKind, Snapshot};

fn run() -> Run {
    Run {
        worktree: WorktreeId::new(),
        turn: TurnId::new(),
        kind: RunKind::Task,
        session: SessionId::new(),
        sequence: 10,
        revision: 1,
        status: Status::Running,
        error: None,
        started_ms: Some(1),
        finished_ms: None,
        origin: None,
        continuation: None,
        joined: None,
        notice: false,
        notices: vec![],
        input: None,
    }
}

fn history(run: &Run) -> History {
    History {
        snapshot: Some(Arc::new(Snapshot {
            node: NodeId([1; 32]),
            sequence: 1,
            missing: vec![],
            drafts: vec![],
            statistics: Default::default(),
            artifacts: Default::default(),
            page: Arc::new(Page {
                session: run.session,
                revision: 1,
                entries: vec![],
                runs: vec![run.clone()],
                queue: Default::default(),
                approvals: vec![],
                questions: vec![],
                children: vec![],
                background: vec![],
                next_before: None,
            }),
        })),
        ..History::default()
    }
}

fn page(history: &mut History) -> &mut Page {
    Arc::make_mut(&mut Arc::make_mut(history.snapshot.as_mut().unwrap()).page)
}

fn call(run: &Run, returned: bool) -> Call {
    Call {
        turn: run.turn,
        id: Some("call".into()),
        name: "fixture".into(),
        terminal_interaction: false,
        presentation: Default::default(),
        grouping: Default::default(),
        source: Reference {
            entry: "call".into(),
            index: 0,
        },
        response: returned.then(|| Reference {
            entry: "result".into(),
            index: 0,
        }),
        state: if returned {
            State::Returned
        } else {
            State::Running
        },
        failure: None,
        approval: None,
        question: None,
        progress: None,
        content: None,
        resolved: None,
    }
}

#[test]
fn ignores_drafts_and_older_pages() {
    let run = run();
    let before = history(&run);
    let mut next = before.clone();
    Arc::make_mut(next.snapshot.as_mut().unwrap())
        .drafts
        .push(Draft {
            id: "draft".into(),
            turn: run.turn,
            author: "assistant".into(),
            branch: String::new(),
            parts: vec![Part::Text("Partial text".into())],
        });
    assert!(!changed(&before, &next, run.worktree));
    let older = Run {
        turn: TurnId::new(),
        sequence: 1,
        status: Status::Completed,
        ..run.clone()
    };
    page(&mut next).runs.insert(0, older.clone());
    next.calls = Arc::new(vec![call(&older, true)]);
    assert!(!changed(&before, &next, run.worktree));
    assert!(!changed(&next, &next, run.worktree));
}

#[test]
fn refreshes_returned_tools_during_execution() {
    let run = run();
    let mut before = history(&run);
    before.calls = Arc::new(vec![call(&run, false)]);
    let mut next = before.clone();
    next.calls = Arc::new(vec![call(&run, true)]);
    assert!(changed(&before, &next, run.worktree));
    let before = next.clone();
    Arc::make_mut(next.snapshot.as_mut().unwrap()).sequence += 1;
    assert!(!changed(&before, &next, run.worktree));
}

#[test]
fn refreshes_new_and_settled_turns_once() {
    let run = run();
    let before = history(&run);
    let mut next = before.clone();
    page(&mut next).runs[0].status = Status::Completed;
    assert!(changed(&before, &next, run.worktree));
    assert!(!changed(&next, &next, run.worktree));
    let before = next.clone();
    page(&mut next).runs.push(Run {
        turn: TurnId::new(),
        sequence: 11,
        status: Status::Completed,
        ..run.clone()
    });
    assert!(changed(&before, &next, run.worktree));
    assert!(!changed(&next, &next, run.worktree));
}

#[test]
fn scopes_child_settlement_to_checkout() {
    let run = run();
    let mut before = history(&run);
    let child = Child {
        run: Run {
            turn: TurnId::new(),
            session: SessionId::new(),
            sequence: 11,
            ..run.clone()
        },
        origin: sailry_protocol::Delegation {
            session: run.session,
            turn: run.turn,
            entry: "call".into(),
            index: 0,
            role: None,
        },
        name: None,
    };
    page(&mut before).children.push(child);
    let mut next = before.clone();
    page(&mut next).children[0].run.status = Status::Completed;
    assert!(changed(&before, &next, run.worktree));
    assert!(!changed(&next, &next, run.worktree));
    let other = WorktreeId::new();
    page(&mut before).children[0].run.worktree = other;
    page(&mut next).children[0].run.worktree = other;
    assert!(!changed(&before, &next, run.worktree));
}

#[test]
fn rejects_replaced_history_ownership() {
    let run = run();
    let before = history(&run);
    let mut next = before.clone();
    page(&mut next).runs[0].status = Status::Completed;
    page(&mut next).revision += 1;
    assert!(!changed(&before, &next, run.worktree));
    page(&mut next).revision -= 1;
    Arc::make_mut(next.snapshot.as_mut().unwrap()).node = NodeId([2; 32]);
    assert!(!changed(&before, &next, run.worktree));
}
