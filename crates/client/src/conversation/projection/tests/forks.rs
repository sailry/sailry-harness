use super::*;

#[test]
fn preserves_the_original_turn_owner() {
    let (mut projection, mut snapshot, mut run, _) = fixture();
    run.status = Status::Completed;
    run.origin = Some(SessionId::new());
    Arc::make_mut(&mut snapshot.page).runs = vec![run.clone()];
    projection
        .apply(1, Update::ConversationSnapshot(snapshot.clone()))
        .unwrap();
    let mut changed = run.clone();
    changed.origin = Some(SessionId::new());
    let mut history = conversation::History {
        sequence: snapshot.sequence,
        page: (*snapshot.page).clone(),
        missing: Vec::new(),
    };
    history.page.runs = vec![changed.clone()];
    assert!(projection.merge_history(1, None, history).is_err());
    assert!(
        projection
            .apply(
                1,
                Update::ConversationFrame(Frame {
                    node: snapshot.node,
                    session: run.session,
                    sequence: snapshot.sequence + 1,
                    change: Change::Run(changed),
                })
            )
            .is_err()
    );
    for (origin, status) in [
        (Some(run.session), Status::Completed),
        (run.origin, Status::Running),
    ] {
        let mut invalid = snapshot.clone();
        Arc::make_mut(&mut invalid.page).runs[0].origin = origin;
        Arc::make_mut(&mut invalid.page).runs[0].status = status;
        assert!(
            projection
                .apply(1, Update::ConversationSnapshot(invalid))
                .is_err()
        );
    }
    assert_eq!(projection.snapshot().unwrap(), &snapshot);
}
