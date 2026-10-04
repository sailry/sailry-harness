use super::*;

fn write(fixture: &mut Fixture, turn: TurnId, path: &str, before: Option<&str>, after: &str) {
    let request = approved(fixture, turn, path, before, after);
    capture(&mut fixture.database, turn, &request, before).unwrap();
    let (reply, _) = oneshot::channel();
    let caller = fixture.database.node;
    fixture
        .database
        .dispatch(caller, request.clone(), reply, &fixture.events);
    let outcome = Ok(Output::FileWritten(FileWritten {
        path: path.into(),
        revision: version(after).revision,
        size: after.len() as u64,
    }));
    assert_eq!(
        fixture
            .database
            .finish_external(caller, &request, outcome.clone(), &fixture.events),
        outcome
    );
}

fn finish(fixture: &mut Fixture, turn: TurnId) {
    runs::finish(
        &mut fixture.database,
        turn,
        Status::Completed,
        None,
        &fixture.events,
    )
    .unwrap();
}

#[test]
fn merges_same_turn_writes() {
    let mut fixture = Fixture::new();
    let turn = running(&mut fixture);
    write(
        &mut fixture,
        turn,
        "source.txt",
        Some("old\nkeep\n"),
        "middle\nkeep\n",
    );
    write(
        &mut fixture,
        turn,
        "source.txt",
        Some("middle\nkeep\n"),
        "new\nkeep\n",
    );
    finish(&mut fixture, turn);
    let first = super::super::diff(&fixture.database.connection, fixture.session, turn).unwrap();
    assert_eq!(first.files.len(), 1);
    assert_eq!((first.files[0].additions, first.files[0].deletions), (1, 1));
    assert!(!first.files[0].text.contains("middle"));
    let next = running(&mut fixture);
    write(&mut fixture, next, "other.txt", None, "later\n");
    finish(&mut fixture, next);
    assert_eq!(
        super::super::diff(&fixture.database.connection, fixture.session, turn).unwrap(),
        first
    );
    assert_eq!(
        super::super::diff(&fixture.database.connection, SessionId::new(), turn)
            .unwrap_err()
            .code,
        ErrorCode::WrongTarget
    );
}

#[test]
fn excludes_manual_edits() {
    let mut fixture = Fixture::new();
    let turn = running(&mut fixture);
    write(
        &mut fixture,
        turn,
        "source.txt",
        Some("original\n"),
        "first\n",
    );
    write(
        &mut fixture,
        turn,
        "source.txt",
        Some("first\nmanual\n"),
        "second\nmanual\n",
    );
    finish(&mut fixture, turn);
    let diff = super::super::diff(&fixture.database.connection, fixture.session, turn).unwrap();
    assert_eq!((diff.files[0].additions, diff.files[0].deletions), (2, 2));
    assert!(!diff.files[0].text.contains("+manual"));
    assert!(!diff.files[0].text.contains("-manual"));
}

#[test]
fn excludes_unconfirmed_changes() {
    let mut fixture = Fixture::new();
    let turn = running(&mut fixture);
    let request = approved(&mut fixture, turn, "pending.txt", None, "pending\n");
    capture(&mut fixture.database, turn, &request, None).unwrap();
    write(
        &mut fixture,
        turn,
        "unchanged.txt",
        Some("same\n"),
        "same\n",
    );
    assert_eq!(
        super::super::diff(&fixture.database.connection, fixture.session, turn)
            .unwrap_err()
            .code,
        ErrorCode::Busy
    );
    finish(&mut fixture, turn);
    assert!(
        super::super::diff(&fixture.database.connection, fixture.session, turn)
            .unwrap()
            .files
            .is_empty()
    );
}
