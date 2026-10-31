use super::*;
use crate::store::agent::tests::fixture::Fixture;

#[tokio::test]
async fn pages_without_hydrating_private_fields() {
    let mut fixture = Fixture::new().await;
    let first = fixture.queued().await;
    let second = fixture.queued().await;
    let third = fixture.queued().await;
    // Invalid capture/notice shapes cannot affect a metadata-only reader.
    fixture.database.connection.execute(
        "UPDATE agent_runs SET body=json_set(body,'$.notices','private notice body','$.input','private input','$.error',json(?1)) WHERE turn=?2",
        params![r#"{"code":"unavailable","message":"private diagnostic"}"#, third.to_string()],
    ).await.unwrap();
    let latest = page(&fixture.database.connection, fixture.session, None, 2)
        .await
        .unwrap();
    assert_eq!(
        latest.runs.iter().map(|run| run.turn).collect::<Vec<_>>(),
        [second, third]
    );
    assert_eq!(latest.next_before, Some(second));
    assert!(latest.queue_revision > 0);
    assert_eq!(latest.runs[1].error_code, Some(ErrorCode::Unavailable));
    let encoded = serde_json::to_string(&latest).unwrap();
    assert!(!encoded.contains("private"));
    let oldest = page(
        &fixture.database.connection,
        fixture.session,
        latest.next_before,
        2,
    )
    .await
    .unwrap();
    assert_eq!(
        oldest.runs.iter().map(|run| run.turn).collect::<Vec<_>>(),
        [first]
    );
    assert_eq!(oldest.next_before, None);
    assert_eq!(
        read(
            &fixture.database.connection,
            fixture.session,
            third,
            Some(latest.revision)
        )
        .await
        .unwrap(),
        latest.runs[1]
    );
    assert_eq!(
        read(
            &fixture.database.connection,
            fixture.session,
            third,
            Some(latest.revision + 1)
        )
        .await
        .unwrap_err()
        .code,
        ErrorCode::RevisionConflict
    );
    assert_eq!(
        read(
            &fixture.database.connection,
            fixture.session,
            TurnId::new(),
            None
        )
        .await
        .unwrap_err()
        .code,
        ErrorCode::NotFound
    );
    assert!(
        page(&fixture.database.connection, fixture.session, None, 0)
            .await
            .is_err()
    );
}

#[tokio::test]
async fn follows_fork_and_rewind_membership() {
    let mut fixture = Fixture::new().await;
    let first = fixture.completed(&["private first response"]).await;
    let second = fixture.completed(&["private second response"]).await;
    let fork = fixture.fork(fixture.session, first).await;
    let inherited = page(&fixture.database.connection, fork.id, None, 100)
        .await
        .unwrap();
    assert_eq!(inherited.runs.len(), 1);
    assert_eq!(inherited.runs[0].turn, first);
    assert_eq!(inherited.runs[0].origin, Some(fixture.session));
    assert_eq!(
        read(&fixture.database.connection, fork.id, second, None)
            .await
            .unwrap_err()
            .code,
        ErrorCode::NotFound
    );
    fixture
        .database
        .connection
        .execute(
            "DELETE FROM conversation_turns WHERE session=?1 AND turn=?2",
            params![fork.id.to_string(), first.to_string()],
        )
        .await
        .unwrap();
    fixture
        .database
        .connection
        .execute(
            "UPDATE sessions SET history_revision=history_revision+1 WHERE id=?1",
            [fork.id.to_string()],
        )
        .await
        .unwrap();
    assert_eq!(
        read(
            &fixture.database.connection,
            fork.id,
            first,
            Some(inherited.revision)
        )
        .await
        .unwrap_err()
        .code,
        ErrorCode::RevisionConflict
    );
    assert_eq!(
        read(&fixture.database.connection, fork.id, first, None)
            .await
            .unwrap_err()
            .code,
        ErrorCode::NotFound
    );
}
