use super::*;

fn silent(mut snapshot: Snapshot, stdout: &str) -> Snapshot {
    snapshot.info.status = process::Status::Running;
    snapshot.stdout.text = stdout.into();
    snapshot
}

async fn stalled(fixture: &Fixture, snapshot: &Snapshot) -> Option<Task> {
    let transaction = fixture
        .database
        .connection
        .unchecked_transaction()
        .await
        .unwrap();
    let task = command_stalled(&transaction, snapshot, 900_000)
        .await
        .unwrap();
    transaction.commit().await.unwrap();
    task
}

#[tokio::test]
async fn notifies_follow_up_session() {
    let mut fixture = Fixture::new().await;
    let source = completed(&mut fixture).await;
    let (request, snapshot) = command(&fixture, source, true).await;
    let task = stalled(&fixture, &silent(snapshot.clone(), "Waiting for receipt"))
        .await
        .unwrap();
    assert_eq!(task.id, format!("stall:{}", request.id));
    assert_eq!(task.kind, Kind::Command { id: request.id });
    assert_eq!(task.state, State::Pending);
    assert_eq!(task.outcome, Some(Outcome::Stalled));
    assert_eq!(task.summary.as_deref(), Some("Waiting for receipt"));
    // The command keeps its own running evidence for the eventual completion.
    assert_eq!(
        get(
            &fixture.database.connection,
            &format!("command:{}", request.id)
        )
        .await
        .unwrap()
        .unwrap()
        .task
        .state,
        State::Running
    );
    let queued = schedule(&fixture).await.unwrap();
    assert!(
        stalled(&fixture, &silent(snapshot, "Later output"))
            .await
            .is_none()
    );
    let invocation = claim(&mut fixture).await;
    assert_eq!(invocation.turn.id, queued.id);
    assert!(invocation.notice);
    assert_eq!(invocation.notices.len(), 1);
    assert_eq!(invocation.notices[0].id, task.id);
    assert_eq!(invocation.notices[0].outcome, Outcome::Stalled);
    let input = invocation.notice_input.as_ref().unwrap();
    let data: Value = serde_json::from_str(input.split_once('\n').unwrap().1).unwrap();
    assert_eq!(data[0]["notice"]["outcome"], "stalled");
    assert_eq!(data[0]["output"]["stdout_tail"], "Waiting for receipt");
    assert_eq!(data[0]["output"]["elapsed_ms"], 900_000);
}

#[tokio::test]
async fn retains_quiet_stall_for_next_input() {
    let mut fixture = Fixture::new().await;
    let source = completed(&mut fixture).await;
    let (_, snapshot) = command(&fixture, source, false).await;
    let task = stalled(&fixture, &silent(snapshot, "")).await.unwrap();
    assert_eq!(task.summary, None);
    assert!(schedule(&fixture).await.is_none());
    let user = fixture.queued().await;
    runs::start(&fixture.database.connection, user)
        .await
        .unwrap();
    let invocation = claim(&mut fixture).await;
    assert!(!invocation.notice);
    assert_eq!(
        invocation
            .notices
            .iter()
            .map(|notice| &notice.id)
            .collect::<Vec<_>>(),
        vec![&task.id]
    );
}

#[tokio::test]
async fn rearms_after_delivery_until_completion() {
    let mut fixture = Fixture::new().await;
    let source = completed(&mut fixture).await;
    let (request, finished) = command(&fixture, source, true).await;
    let first = stalled(&fixture, &silent(finished.clone(), "First"))
        .await
        .unwrap();
    schedule(&fixture).await.unwrap();
    let invocation = claim(&mut fixture).await;
    append(&mut fixture, &invocation, event(&invocation)).await;
    let delivered = get(&fixture.database.connection, &first.id)
        .await
        .unwrap()
        .unwrap()
        .task;
    assert_eq!(delivered.state, State::Delivered);
    let second = stalled(&fixture, &silent(finished.clone(), "Second"))
        .await
        .unwrap();
    assert_eq!(second.id, format!("stall:{}", request.id));
    assert_eq!(second.state, State::Pending);
    assert_eq!(second.delivery, None);
    assert!(second.revision > delivered.revision);
    assert_eq!(second.summary.as_deref(), Some("Second"));
    {
        let transaction = fixture
            .database
            .connection
            .unchecked_transaction()
            .await
            .unwrap();
        command_finished(&transaction, &finished, Some(1), false)
            .await
            .unwrap()
            .unwrap();
        transaction.commit().await.unwrap();
    }
    assert!(stalled(&fixture, &silent(finished, "Late")).await.is_none());
}
