use super::*;
use conversation::{Child, History};

fn setup() -> (Projection, Snapshot, Child) {
    let (projection, mut snapshot, run, _) = fixture();
    let origin = Delegation {
        session: run.session,
        turn: run.turn,
        entry: "parent-call".into(),
        index: 0,
        role: Some(RoleId::new()),
    };
    Arc::make_mut(&mut snapshot.page).entries.push(Entry {
        sequence: 1,
        id: origin.entry.clone(),
        turn: run.turn,
        author: "assistant".into(),
        branch: String::new(),
        timestamp_ms: 1,
        usage: None,
        citations: vec![],
        search_suggestions: None,
        parts: vec![Part::ToolCall {
            display: None,
            presentation: Default::default(),
            grouping: Default::default(),
            id: Some("call".into()),
            name: "third_party_delegate".into(),
            arguments: serde_json::json!({"task":"Review"}),
        }],
    });
    let child = Child {
        run: Run {
            worktree: run.worktree,
            session: SessionId::new(),
            turn: TurnId::new(),
            sequence: 2,
            revision: 1,
            ..run
        },
        origin,
        name: Some("Frozen review".into()),
    };
    (projection, snapshot, child)
}

#[test]
fn orders_updates_and_recovers_gaps() {
    let (mut projection, snapshot, mut child) = setup();
    projection
        .apply(1, Update::ConversationSnapshot(snapshot.clone()))
        .unwrap();
    let started = frame(&snapshot, 11, Change::Child(child.clone()));
    assert_eq!(
        projection.apply(1, started.clone()).unwrap(),
        Apply::Applied
    );
    assert_eq!(projection.apply(1, started).unwrap(), Apply::Ignored);
    child.run.status = Status::Stopping;
    projection
        .apply(1, frame(&snapshot, 12, Change::Child(child.clone())))
        .unwrap();
    child.run.status = Status::Running;
    let stable = projection.snapshot().unwrap().clone();
    assert!(
        projection
            .apply(1, frame(&snapshot, 13, Change::Child(child.clone())))
            .is_err()
    );
    assert_eq!(projection.snapshot().unwrap(), &stable);
    child.run.status = Status::Cancelled;
    assert_eq!(
        projection
            .apply(1, frame(&snapshot, 14, Change::Child(child.clone())))
            .unwrap(),
        Apply::Recover
    );
    projection.reconnect(2).unwrap();
    assert_eq!(
        projection
            .apply(1, frame(&snapshot, 13, Change::Child(child.clone())))
            .unwrap(),
        Apply::Ignored
    );
    let mut recovered = snapshot.clone();
    recovered.sequence = 0;
    Arc::make_mut(&mut recovered.page).children = vec![child.clone()];
    projection
        .apply(2, Update::ConversationSnapshot(recovered.clone()))
        .unwrap();
    child.run.status = Status::Completed;
    assert!(
        projection
            .apply(2, frame(&snapshot, 1, Change::Child(child)))
            .is_err()
    );
    assert_eq!(projection.snapshot().unwrap(), &recovered);
}

#[test]
fn rejects_forged_identity() {
    let (mut projection, mut snapshot, child) = setup();
    Arc::make_mut(&mut snapshot.page)
        .children
        .push(child.clone());
    projection
        .apply(1, Update::ConversationSnapshot(snapshot.clone()))
        .unwrap();
    let mut variants = vec![];
    let mut forged = child.clone();
    forged.run.session = SessionId::new();
    variants.push(forged);
    let mut forged = child.clone();
    forged.run.turn = TurnId::new();
    variants.push(forged);
    let mut forged = child.clone();
    forged.run.sequence += 1;
    variants.push(forged);
    let mut forged = child.clone();
    forged.run.revision += 1;
    variants.push(forged);
    let mut forged = child.clone();
    forged.run.kind = conversation::RunKind::Compaction;
    variants.push(forged);
    let mut forged = child.clone();
    forged.origin.session = SessionId::new();
    variants.push(forged);
    let mut forged = child.clone();
    forged.origin.index = 1;
    variants.push(forged);
    let mut forged = child.clone();
    forged.name = Some("Changed".into());
    variants.push(forged);
    for forged in variants {
        assert!(
            projection
                .apply(1, frame(&snapshot, 11, Change::Child(forged)))
                .is_err()
        );
        assert_eq!(projection.snapshot().unwrap(), &snapshot);
    }
    let mut repeated = snapshot.clone();
    Arc::make_mut(&mut repeated.page).children.push(child);
    assert!(
        projection
            .apply(1, Update::ConversationSnapshot(repeated))
            .is_err()
    );
    assert_eq!(projection.snapshot().unwrap(), &snapshot);
}

#[test]
fn preserves_live_state_on_merge() {
    let (mut projection, mut snapshot, mut child) = setup();
    Arc::make_mut(&mut snapshot.page)
        .children
        .push(child.clone());
    projection
        .apply(1, Update::ConversationSnapshot(snapshot.clone()))
        .unwrap();
    child.run.status = Status::Completed;
    projection
        .apply(1, frame(&snapshot, 11, Change::Child(child.clone())))
        .unwrap();
    let old = History {
        sequence: 10,
        page: (*snapshot.page).clone(),
        missing: vec![],
    };
    assert_eq!(
        projection.merge_history(1, None, old.clone()).unwrap(),
        Apply::Applied
    );
    assert_eq!(
        projection.snapshot().unwrap().page.children,
        [child.clone()]
    );
    let mut conflicting = old;
    conflicting.page.children[0].run.status = Status::Failed;
    assert!(projection.merge_history(1, None, conflicting).is_err());
    assert_eq!(projection.snapshot().unwrap().page.children, [child]);

    let branch = SessionId::new();
    let inherited = Arc::make_mut(&mut snapshot.page);
    inherited.session = branch;
    inherited.runs[0].origin = Some(inherited.runs[0].session);
    inherited.runs[0].session = branch;
    inherited.runs[0].status = Status::Completed;
    inherited.children[0].run.status = Status::Completed;
    let mut projection = Projection::new(snapshot.node, branch, 1);
    projection
        .apply(1, Update::ConversationSnapshot(snapshot.clone()))
        .unwrap();
    assert_ne!(snapshot.page.children[0].origin.session, branch);
    assert_eq!(projection.snapshot().unwrap(), &snapshot);
}

#[test]
fn hydrates_from_earlier_call() {
    let (_, snapshot, child) = setup();
    let mut page = (*snapshot.page).clone();
    page.entries.clear();
    let mut chunk = (*snapshot.page).clone();
    chunk.children.push(child.clone());
    super::super::super::history::merge(&mut page, chunk.clone(), false).unwrap();
    assert_eq!(page.children, [child]);
    assert_eq!(page.entries, chunk.entries);
    super::super::super::history::merge(&mut page, chunk.clone(), false).unwrap();
    assert_eq!(page, chunk);
}

#[test]
fn validates_worktree_selection() {
    let (mut projection, mut snapshot, mut child) = setup();
    let target = WorktreeId::new();
    let page = Arc::make_mut(&mut snapshot.page);
    let Part::ToolCall { arguments, .. } = &mut page.entries.last_mut().unwrap().parts[0] else {
        panic!("delegation call expected")
    };
    arguments["worktree"] = serde_json::json!(target);
    child.run.worktree = target;
    projection
        .apply(1, Update::ConversationSnapshot(snapshot.clone()))
        .unwrap();
    projection
        .apply(1, frame(&snapshot, 11, Change::Child(child.clone())))
        .unwrap();
    let stable = projection.snapshot().unwrap().clone();
    child.run.worktree = WorktreeId::new();
    assert!(
        projection
            .apply(1, frame(&snapshot, 12, Change::Child(child)))
            .is_err()
    );
    assert_eq!(projection.snapshot().unwrap(), &stable);
}

#[test]
fn rejects_parent_relocation() {
    let (mut projection, snapshot, _) = setup();
    projection
        .apply(1, Update::ConversationSnapshot(snapshot.clone()))
        .unwrap();
    let mut run = snapshot.page.runs[0].clone();
    run.worktree = WorktreeId::new();
    assert!(
        projection
            .apply(1, frame(&snapshot, 11, Change::Run(run.clone())))
            .is_err()
    );
    assert_eq!(projection.snapshot().unwrap().page, snapshot.page);
    let mut page = (*snapshot.page).clone();
    let mut incoming = page.clone();
    incoming.runs[0] = run;
    assert!(crate::conversation::history::merge(&mut page, incoming, true).is_err());
    assert_eq!(page, *snapshot.page);
}
