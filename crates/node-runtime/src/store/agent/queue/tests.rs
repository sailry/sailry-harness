use super::*;
use crate::store::agent::tests::{command, fixture::Fixture};

#[test]
fn pauses_unclaimed_input_after_restart() {
    for ready in [false, true] {
        let mut fixture = Fixture::new();
        let turn = fixture.queued();
        if ready {
            runs::start(&fixture.database.connection, turn).unwrap();
        }
        let message = message(&fixture.database.connection, turn).unwrap();
        let before = read(&fixture.database.connection, fixture.session).unwrap();
        assert!(!before.paused);
        assert_eq!(before.items[0].ready, ready);
        let path = fixture
            ._root
            .path()
            .canonicalize()
            .unwrap()
            .join("node.sqlite3");
        let node = fixture.database.node;
        fixture.database.close().unwrap();
        let mut database = Database::open(&path, node, None).unwrap();
        let recovered = read(&database.connection, fixture.session).unwrap();
        assert!(recovered.paused);
        assert_eq!(recovered.revision, before.revision + 1);
        assert_eq!(recovered.items, before.items);
        assert_eq!(super::message(&database.connection, turn).unwrap(), message);
        assert_eq!(
            runs::get(&database.connection, turn).unwrap().status,
            Status::Queued
        );
        assert!(
            runs::claim(&mut database, &fixture.events)
                .unwrap()
                .is_none()
        );

        database.close().unwrap();
        let mut database = Database::open(&path, node, None).unwrap();
        assert_eq!(
            read(&database.connection, fixture.session).unwrap(),
            recovered
        );
        runs::start(&database.connection, turn).unwrap();
        assert!(
            runs::claim(&mut database, &fixture.events)
                .unwrap()
                .is_none()
        );
        command(
            &mut database,
            &fixture.events,
            Command::SubmitTurn {
                session: fixture.session,
                expected_revision: message.turn.revision,
                message: "Fresh input".into(),
            },
        );
        let invocation = runs::claim(&mut database, &fixture.events)
            .unwrap()
            .unwrap();
        assert_eq!(invocation.turn, message.turn);
        assert_eq!(invocation.message, message.message);
        database.close().unwrap();
    }
}
