use super::*;
use sailry_protocol::conversation::{Approval, ApprovalState};

#[test]
fn recovers_decisions() {
    let (mut projection, snapshot, run, _) = fixture();
    let approval = Approval {
        id: ApprovalId::new(),
        session: run.session,
        turn: run.turn,
        entry: "canonical-call".into(),
        index: 0,
        state: ApprovalState::Pending,
        source: sailry_protocol::conversation::ApprovalSource::User,
    };
    projection
        .apply(1, Update::ConversationSnapshot(snapshot.clone()))
        .unwrap();
    let update = frame(&snapshot, 11, Change::Approval(approval.clone()));
    assert_eq!(projection.apply(1, update.clone()).unwrap(), Apply::Applied);
    assert_eq!(projection.apply(1, update).unwrap(), Apply::Ignored);
    let mut approved = approval.clone();
    approved.state = ApprovalState::Approved;
    assert_eq!(
        projection
            .apply(1, frame(&snapshot, 13, Change::Approval(approved.clone())))
            .unwrap(),
        Apply::Recover
    );
    assert_eq!(
        projection.snapshot().unwrap().page.approvals,
        vec![approval]
    );
    projection.reconnect(2).unwrap();
    let mut recovered = snapshot.clone();
    Arc::make_mut(&mut recovered.page)
        .approvals
        .push(approved.clone());
    recovered.sequence = 1;
    projection
        .apply(2, Update::ConversationSnapshot(recovered.clone()))
        .unwrap();
    assert_eq!(
        projection.snapshot().unwrap().page.approvals,
        vec![approved.clone()]
    );
    let mut changed = approved.clone();
    changed.state = ApprovalState::Denied;
    assert!(
        projection
            .apply(2, frame(&snapshot, 2, Change::Approval(changed)))
            .is_err()
    );
    let mut wrong = approved.clone();
    wrong.session = SessionId::new();
    assert!(
        projection
            .apply(2, frame(&snapshot, 2, Change::Approval(wrong)))
            .is_err()
    );
    let mut changed = approved;
    changed.index = 1;
    assert!(
        projection
            .apply(2, frame(&snapshot, 2, Change::Approval(changed)))
            .is_err()
    );
}
