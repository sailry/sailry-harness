use super::*;
use crate::conversation::live::tests::fixture::Fixture;
use core::prelude::v1::test;

fn observe(fixture: &Fixture) -> NodeView {
    NodeView {
        snapshot: Some(
            fixture
                .runtime
                .block_on(read(&fixture.binding.client))
                .unwrap(),
        ),
        connected: true,
        ..Default::default()
    }
}

fn create(fixture: &Fixture, source: &sailry_protocol::Session) -> sailry_protocol::Session {
    let Output::Session(session) = fixture.execute(Command::CreateSession {
        project: source.project,
        worktree: Some(source.worktree),
        config: Some(source.config.clone()),
    }) else {
        panic!("session expected")
    };
    session
}

#[test]
fn reconciles_created_and_removed_sessions() {
    for remote in [false, true] {
        let fixture = Fixture::with_tools(remote, vec![]);
        let stale = observe(&fixture);
        let original = stale.snapshot.as_ref().unwrap().sessions[0].clone();
        let created = create(&fixture, &original);
        let mut next = stale.clone();
        assert!(missing(&next, created.id));
        assert!(!confirm(
            &mut next,
            fixture.node.id(),
            created.id,
            fixture.runtime.block_on(read(&fixture.binding.client))
        ));
        assert_eq!(removed(&next, None, Some(created.id)), None);
        assert_eq!(
            next.snapshot, stale.snapshot,
            "confirmation must not replace Client's projection"
        );
        let current = observe(&fixture);
        assert!(!missing(&current, created.id));
        let mut projection = sailry_client::Projection::new(fixture.node.id(), 0);
        assert_eq!(
            projection
                .apply(
                    0,
                    sailry_protocol::Update::Snapshot(current.snapshot.clone().unwrap())
                )
                .unwrap(),
            sailry_client::Apply::Applied
        );
        assert_eq!(
            projection
                .apply(
                    0,
                    sailry_protocol::Update::Snapshot(stale.snapshot.unwrap())
                )
                .unwrap(),
            sailry_client::Apply::Ignored
        );
        fixture.execute(Command::RemoveSession {
            session: created.id,
            expected_revision: created.revision,
        });
        let mut absent = observe(&fixture);
        assert!(confirm(
            &mut absent,
            fixture.node.id(),
            created.id,
            fixture.runtime.block_on(read(&fixture.binding.client))
        ));
        assert_eq!(
            removed(&absent, Some(created.id), Some(created.id)),
            Some(created.id)
        );
        assert_eq!(
            removed(&absent, Some(created.id), Some(original.id)),
            None,
            "a late confirmation cannot remove another selected session"
        );
        let transient = create(&fixture, &original);
        fixture.execute(Command::RemoveSession {
            session: transient.id,
            expected_revision: transient.revision,
        });
        let mut unseen = current;
        assert!(missing(&unseen, transient.id));
        assert!(confirm(
            &mut unseen,
            fixture.node.id(),
            transient.id,
            fixture.runtime.block_on(read(&fixture.binding.client))
        ));
        assert_eq!(
            removed(&unseen, Some(transient.id), Some(transient.id)),
            Some(transient.id),
            "removal before first observation must not retain the session forever"
        );
        let mut restored = absent.snapshot.clone().unwrap();
        restored.cursor = 0;
        assert!(
            !confirm(
                &mut unseen,
                fixture.node.id(),
                transient.id,
                Ok(restored.clone())
            ),
            "a lower reply must wait for Client recovery"
        );
        projection.reconnect(1).unwrap();
        assert_eq!(
            projection
                .apply(1, sailry_protocol::Update::Snapshot(restored.clone()))
                .unwrap(),
            sailry_client::Apply::Applied
        );
        unseen.snapshot = projection.snapshot().cloned();
        assert!(
            confirm(&mut unseen, fixture.node.id(), transient.id, Ok(restored)),
            "recovered observations use the restored cursor"
        );
        let before = unseen.snapshot.clone();
        assert!(!confirm(
            &mut unseen,
            fixture.node.id(),
            transient.id,
            Err(Fault::new(
                ErrorCode::Unavailable,
                "injected snapshot failure"
            ))
        ));
        assert_eq!(unseen.error.as_ref().unwrap().code, ErrorCode::Unavailable);
        assert_eq!(unseen.snapshot, before);
        assert_eq!(removed(&unseen, None, Some(transient.id)), None);
        let snapshot = unseen.snapshot.clone().unwrap();
        assert!(!confirm(
            &mut unseen,
            NodeId([99; 32]),
            transient.id,
            Ok(snapshot)
        ));
        assert_eq!(unseen.error.as_ref().unwrap().code, ErrorCode::WrongTarget);
        fixture.close();
    }
}
