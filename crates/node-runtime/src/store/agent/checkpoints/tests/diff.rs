use super::*;

async fn write(fixture: &mut Fixture, turn: TurnId, path: &str, before: Option<&str>, after: &str) {
    let request = approved(fixture, turn, path, before, after).await;
    capture(&mut fixture.database, turn, &request, before)
        .await
        .unwrap();
    let (reply, _) = oneshot::channel();
    let caller = fixture.database.node;
    fixture
        .database
        .dispatch(caller, request.clone(), reply, &fixture.events)
        .await;
    let outcome = Ok(Output::FileWritten(FileWritten {
        path: path.into(),
        revision: version(after).revision,
        size: after.len() as u64,
    }));
    assert_eq!(
        fixture
            .database
            .finish_external(caller, &request, outcome.clone(), &fixture.events)
            .await,
        outcome
    );
}

async fn finish(fixture: &mut Fixture, turn: TurnId) {
    runs::finish(
        &mut fixture.database,
        turn,
        Status::Completed,
        None,
        &fixture.events,
    )
    .await
    .unwrap();
}

#[tokio::test]
async fn merges_same_turn_writes() {
    let mut fixture = Fixture::new().await;
    let turn = running(&mut fixture).await;
    write(
        &mut fixture,
        turn,
        "source.txt",
        Some("old\nkeep\n"),
        "middle\nkeep\n",
    )
    .await;
    write(
        &mut fixture,
        turn,
        "source.txt",
        Some("middle\nkeep\n"),
        "new\nkeep\n",
    )
    .await;
    finish(&mut fixture, turn).await;
    let first = super::super::diff(&fixture.database.connection, fixture.session, turn)
        .await
        .unwrap();
    assert_eq!(first.files.len(), 1);
    assert_eq!((first.files[0].additions, first.files[0].deletions), (1, 1));
    assert!(!first.files[0].text.contains("middle"));
    let next = running(&mut fixture).await;
    write(&mut fixture, next, "other.txt", None, "later\n").await;
    finish(&mut fixture, next).await;
    assert_eq!(
        super::super::diff(&fixture.database.connection, fixture.session, turn)
            .await
            .unwrap(),
        first
    );
    assert_eq!(
        super::super::diff(&fixture.database.connection, SessionId::new(), turn)
            .await
            .unwrap_err()
            .code,
        ErrorCode::WrongTarget
    );
}

#[tokio::test]
async fn excludes_manual_edits() {
    let mut fixture = Fixture::new().await;
    let turn = running(&mut fixture).await;
    write(
        &mut fixture,
        turn,
        "source.txt",
        Some("original\n"),
        "first\n",
    )
    .await;
    write(
        &mut fixture,
        turn,
        "source.txt",
        Some("first\nmanual\n"),
        "second\nmanual\n",
    )
    .await;
    finish(&mut fixture, turn).await;
    let diff = super::super::diff(&fixture.database.connection, fixture.session, turn)
        .await
        .unwrap();
    assert_eq!((diff.files[0].additions, diff.files[0].deletions), (2, 2));
    assert!(!diff.files[0].text.contains("+manual"));
    assert!(!diff.files[0].text.contains("-manual"));
}

#[tokio::test]
async fn excludes_unconfirmed_changes() {
    let mut fixture = Fixture::new().await;
    let turn = running(&mut fixture).await;
    let request = approved(&mut fixture, turn, "pending.txt", None, "pending\n").await;
    capture(&mut fixture.database, turn, &request, None)
        .await
        .unwrap();
    write(
        &mut fixture,
        turn,
        "unchanged.txt",
        Some("same\n"),
        "same\n",
    )
    .await;
    assert_eq!(
        super::super::diff(&fixture.database.connection, fixture.session, turn)
            .await
            .unwrap_err()
            .code,
        ErrorCode::Busy
    );
    finish(&mut fixture, turn).await;
    assert!(
        super::super::diff(&fixture.database.connection, fixture.session, turn)
            .await
            .unwrap()
            .files
            .is_empty()
    );
}
