use super::*;
use sailry_protocol::conversation::{Approval, ApprovalState};

#[test]
fn recovers_complete_canonical_decisions() {
    for (case_name, run) in [
        (
            "recovers_decisions",
            (|case_name| {
                let (mut projection, snapshot, run, _) = fixture();
                let approval = Approval {
                    id: ApprovalId::new(),
                    session: run.session,
                    turn: run.turn,
                    entry: "canonical-call".into(),
                    index: 0,
                    state: ApprovalState::Pending,
                    source: sailry_protocol::conversation::ApprovalSource::User,
                    access: Some(sailry_protocol::process::Access::Unrestricted),
                    startup: None,
                    ssh: None,
                    changes: None,
                    can_remember: false,
                    prefix: None,
                    choice: None,
                    reason: None,
                };
                projection
                    .apply(1, Update::ConversationSnapshot(Box::new(snapshot.clone())))
                    .unwrap();
                let update = frame(&snapshot, 11, Change::Approval(approval.clone()));
                assert_eq!(
                    projection.apply(1, update.clone()).unwrap(),
                    Apply::Applied,
                    "{case_name}"
                );
                assert_eq!(
                    projection.apply(1, update).unwrap(),
                    Apply::Ignored,
                    "{case_name}"
                );
                let mut approved = approval.clone();
                approved.state = ApprovalState::Approved;
                approved.choice = Some(sailry_protocol::conversation::ApprovalChoice::Once);
                assert_eq!(
                    projection
                        .apply(1, frame(&snapshot, 13, Change::Approval(approved.clone())))
                        .unwrap(),
                    Apply::Recover,
                    "{case_name}"
                );
                assert_eq!(
                    projection.snapshot().unwrap().page.approvals,
                    vec![approval],
                    "{case_name}"
                );
                projection.reconnect(2).unwrap();
                let mut recovered = snapshot.clone();
                Arc::make_mut(&mut recovered.page)
                    .approvals
                    .push(approved.clone());
                recovered.sequence = 1;
                projection
                    .apply(2, Update::ConversationSnapshot(Box::new(recovered.clone())))
                    .unwrap();
                assert_eq!(
                    projection.snapshot().unwrap().page.approvals,
                    vec![approved.clone()],
                    "{case_name}"
                );
                let mut changed = approved.clone();
                changed.state = ApprovalState::Denied;
                assert!(
                    projection
                        .apply(2, frame(&snapshot, 2, Change::Approval(changed)))
                        .is_err(),
                    "{case_name}"
                );
                let mut wrong = approved.clone();
                wrong.session = SessionId::new();
                assert!(
                    projection
                        .apply(2, frame(&snapshot, 2, Change::Approval(wrong)))
                        .is_err(),
                    "{case_name}"
                );
                let mut changed = approved;
                changed.index = 1;
                assert!(
                    projection
                        .apply(2, frame(&snapshot, 2, Change::Approval(changed)))
                        .is_err(),
                    "{case_name}"
                );
            }) as fn(&str),
        ),
        (
            "preserves_rule_identity",
            (|case_name| {
                for change in 0..7 {
                    let (mut projection, mut snapshot, run, _) = fixture();
                    let approval = Approval {
                        id: ApprovalId::new(),
                        session: run.session,
                        turn: run.turn,
                        entry: "canonical-call".into(),
                        index: 0,
                        state: ApprovalState::Pending,
                        source: conversation::ApprovalSource::User,
                        access: None,
                        startup: None,
                        ssh: None,
                        changes: None,
                        can_remember: true,
                        prefix: Some("cargo test".into()),
                        choice: None,
                        reason: None,
                    };
                    Arc::make_mut(&mut snapshot.page)
                        .approvals
                        .push(approval.clone());
                    projection
                        .apply(1, Update::ConversationSnapshot(Box::new(snapshot.clone())))
                        .unwrap();
                    let mut changed = approval;
                    match change {
                        0 => changed.source = conversation::ApprovalSource::SessionRule,
                        1 => changed.prefix = Some("cargo build".into()),
                        2 => changed.can_remember = false,
                        3 => changed.access = Some(sailry_protocol::process::Access::Unrestricted),
                        4 => changed.source = conversation::ApprovalSource::Sandbox,
                        5 => {
                            changed.startup = Some(sailry_protocol::code::Startup {
                                worktree: sailry_protocol::WorktreeId::new(),
                                root: String::new(),
                                server: "fixture".into(),
                                executable: "tool".into(),
                                revision: "a".repeat(64),
                            })
                        }
                        _ => {
                            changed.ssh = Some(sailry_protocol::ssh::Trust {
                                profile: sailry_protocol::SshId::new(),
                                expected_revision: 1,
                                host: "ssh.example.invalid".into(),
                                port: 22,
                                key: sailry_protocol::ssh::HostKey {
                                    algorithm: "ssh-ed25519".into(),
                                    fingerprint: "SHA256:fixture".into(),
                                },
                                changed: false,
                            })
                        }
                    }
                    assert!(
                        projection
                            .apply(1, frame(&snapshot, 11, Change::Approval(changed)))
                            .is_err(),
                        "{case_name}"
                    );
                    assert_eq!(projection.snapshot().unwrap(), &snapshot, "{case_name}");
                }
            }) as fn(&str),
        ),
        (
            "preserves_complete_decision",
            (|case_name| {
                use conversation::ApprovalChoice;
                for (state, choice, reason) in [
                    (ApprovalState::Approved, Some(ApprovalChoice::Session), None),
                    (
                        ApprovalState::Denied,
                        None,
                        Some("Keep 原文 🙂 unchanged".to_owned()),
                    ),
                ] {
                    for change in 0..2 {
                        let (mut projection, mut snapshot, run, _) = fixture();
                        let approval = Approval {
                            id: ApprovalId::new(),
                            session: run.session,
                            turn: run.turn,
                            entry: "canonical-call".into(),
                            index: 0,
                            state,
                            source: conversation::ApprovalSource::User,
                            access: None,
                            startup: None,
                            ssh: None,
                            changes: None,
                            can_remember: true,
                            prefix: Some("cargo test".into()),
                            choice: choice.clone(),
                            reason: reason.clone(),
                        };
                        Arc::make_mut(&mut snapshot.page)
                            .approvals
                            .push(approval.clone());
                        projection
                            .apply(1, Update::ConversationSnapshot(Box::new(snapshot.clone())))
                            .unwrap();
                        projection
                            .apply(1, frame(&snapshot, 11, Change::Approval(approval.clone())))
                            .unwrap();
                        let before = projection.snapshot().unwrap().clone();
                        let mut changed = approval;
                        if change == 0 {
                            changed.choice = Some(ApprovalChoice::Prefix("cargo test".into()));
                        } else {
                            changed.reason = Some("Changed constraint".into());
                        }
                        assert!(
                            projection
                                .apply(1, frame(&snapshot, 12, Change::Approval(changed)))
                                .is_err(),
                            "{case_name}"
                        );
                        assert_eq!(projection.snapshot().unwrap(), &before, "{case_name}");
                    }
                }
            }) as fn(&str),
        ),
    ] {
        run(case_name);
    }
}
