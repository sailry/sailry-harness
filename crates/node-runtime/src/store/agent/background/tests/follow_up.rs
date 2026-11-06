use super::*;

async fn omit(fixture: &Fixture, id: &str) {
    let record = get(&fixture.database.connection, id)
        .await
        .unwrap()
        .unwrap();
    let mut body = serde_json::to_value(record).unwrap();
    body.as_object_mut().unwrap().remove("follow_up");
    fixture
        .database
        .connection
        .execute(
            "UPDATE agent_background SET body=?2 WHERE id=?1",
            params![id, encode(&body).unwrap()],
        )
        .await
        .unwrap();
}

#[tokio::test]
async fn captures_choice_without_changing_evidence() {
    let mut fixture = Fixture::new().await;
    let source = completed(&mut fixture).await;
    for follow_up in [false, true] {
        let (request, snapshot) = command(&fixture, source, follow_up).await;
        let id = format!("command:{}", request.id);
        assert_eq!(
            get(&fixture.database.connection, &id)
                .await
                .unwrap()
                .unwrap()
                .follow_up,
            follow_up
        );
        let task = {
            let transaction = fixture
                .database
                .connection
                .unchecked_transaction()
                .await
                .unwrap();
            let task = command_finished(&transaction, &snapshot, Some(1), false)
                .await
                .unwrap()
                .unwrap();
            transaction.commit().await.unwrap();
            task
        };
        assert_eq!(task.state, State::Pending);
        assert_eq!(task.outcome, Some(Outcome::Completed));
        assert!(!task.suppressed);
        let record = get(&fixture.database.connection, &id)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(record.follow_up, follow_up);
        assert_eq!(record.stdout.as_deref(), Some("Result"));
    }
}

#[tokio::test]
async fn excludes_quiet_before_batching() {
    let mut fixture = Fixture::new().await;
    let source = completed(&mut fixture).await;
    let mut quiet = Vec::new();
    for _ in 0..17 {
        let id = pending(&fixture, source, "Quiet result").await;
        let mut record = get(&fixture.database.connection, &id)
            .await
            .unwrap()
            .unwrap();
        record.follow_up = false;
        save(&fixture.database.connection, &mut record)
            .await
            .unwrap();
        quiet.push(id);
    }
    omit(&fixture, &quiet[0]).await;
    assert!(
        !get(&fixture.database.connection, &quiet[0])
            .await
            .unwrap()
            .unwrap()
            .follow_up
    );
    assert!(schedule(&fixture).await.is_none());

    let opted_in = pending(&fixture, source, "Requested reply").await;
    let parent = runs::get(&fixture.database.connection, source)
        .await
        .unwrap();
    let child = {
        let transaction = fixture
            .database
            .connection
            .unchecked_transaction()
            .await
            .unwrap();
        let child = child_started(&transaction, &parent, &parent, "Child result")
            .await
            .unwrap();
        child_finished(&transaction, &parent).await.unwrap();
        transaction.commit().await.unwrap();
        child
    };
    assert!(
        !get(&fixture.database.connection, &child.id)
            .await
            .unwrap()
            .unwrap()
            .follow_up
    );
    let queued = schedule(&fixture).await.unwrap();
    let run = runs::get(&fixture.database.connection, queued.id)
        .await
        .unwrap();
    assert_eq!(
        run.notices
            .iter()
            .map(|notice| notice.id.clone())
            .collect::<BTreeSet<_>>(),
        BTreeSet::from([opted_in, child.id]),
    );
    for id in quiet {
        let record = get(&fixture.database.connection, &id)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(record.task.state, State::Pending);
        assert_eq!(record.task.delivery, None);
        assert_eq!(record.stdout.as_deref(), Some("Quiet result"));
    }
}

/// Persists a completion and schedules delivery in one transaction, as the process sink does.
async fn complete(fixture: &Fixture, snapshot: &Snapshot) -> Option<QueuedTurn> {
    let transaction = fixture
        .database
        .connection
        .unchecked_transaction()
        .await
        .unwrap();
    command_finished(&transaction, snapshot, Some(1), false)
        .await
        .unwrap()
        .unwrap();
    let queued = super::super::schedule(&transaction, fixture.database.node, fixture.session)
        .await
        .unwrap();
    transaction.commit().await.unwrap();
    queued
}

#[tokio::test]
async fn continues_consecutive_notice_turns() {
    let mut fixture = Fixture::new().await;
    let user = fixture.queued().await;
    runs::start(&fixture.database.connection, user)
        .await
        .unwrap();
    let invocation = claim(&mut fixture).await;
    let mut input = fixture::message(
        "user",
        canonical::Role::User,
        vec![BlockKind::Text {
            text: "History fixture".into(),
        }],
    );
    input.turn = invocation.turn.id;
    append(&mut fixture, &invocation, input).await;
    let mut snapshot = command(&fixture, invocation.turn.id, true).await.1;
    let mut owner = invocation.turn.id;
    for _ in 0..6 {
        runs::finish(
            &mut fixture.database,
            owner,
            Status::Completed,
            None,
            &fixture.events,
        )
        .await
        .unwrap();
        assert!(
            queue::read(&fixture.database.connection, fixture.session)
                .await
                .unwrap()
                .items
                .is_empty()
        );
        let queued = complete(&fixture, &snapshot)
            .await
            .expect("an idle session continues with the completion");
        let invocation = claim(&mut fixture).await;
        assert_eq!(invocation.turn.id, queued.id);
        assert!(invocation.notice);
        let id = format!("command:{}", snapshot.info.id);
        assert_eq!(
            invocation
                .notices
                .iter()
                .map(|notice| &notice.id)
                .collect::<Vec<_>>(),
            vec![&id]
        );
        append(&mut fixture, &invocation, event(&invocation)).await;
        let task = get(&fixture.database.connection, &id)
            .await
            .unwrap()
            .unwrap()
            .task;
        assert_eq!(task.state, State::Delivered);
        assert_eq!(task.delivery, Some(invocation.turn.id));
        snapshot = command(&fixture, invocation.turn.id, true).await.1;
        owner = invocation.turn.id;
    }
}

#[tokio::test]
async fn retains_reserved_input_after_default_changes() {
    let mut fixture = Fixture::new().await;
    let source = completed(&mut fixture).await;
    let id = pending(&fixture, source, "Reserved result").await;
    let queued = schedule(&fixture).await.unwrap();
    omit(&fixture, &id).await;
    let invocation = claim(&mut fixture).await;
    assert_eq!(invocation.turn.id, queued.id);
    assert!(invocation.notice);
    assert_eq!(invocation.notices.len(), 1);
    assert_eq!(invocation.notices[0].id, id);
    assert!(
        invocation
            .notice_input
            .as_ref()
            .unwrap()
            .contains("Reserved result")
    );
}
