use super::*;
use sailry_link::CancellationToken;

#[tokio::test]
async fn closes_without_answer() {
    let mut fixture = Fixture::with_input(
        WorkMode::Code,
        Input::Choice {
            options: vec!["Recommended".into(), "Alternative".into()],
            multiple: false,
            allow_other: false,
        },
    )
    .await;
    let question = fixture.begin().await;
    let (reply, response) = oneshot::channel();
    fixture
        .database
        .questions
        .lock()
        .unwrap()
        .insert(question.id, (fixture.turn, reply));
    let expired = expire(
        &mut fixture.database,
        fixture.turn,
        fixture.request.function_call_id.as_deref().unwrap(),
        &CancellationToken::new(),
        &fixture.events,
    )
    .await
    .unwrap();
    assert_eq!(expired.id, question.id);
    assert_eq!(expired.state, State::TimedOut);
    assert!(fixture.database.questions.lock().unwrap().is_empty());
    assert!(response.await.is_err());
    assert_eq!(
        resolve(
            &fixture.database.connection,
            fixture.session,
            question.id,
            &Response::Answer(Answer::Choices {
                selected: vec![0],
                other: None,
            }),
        )
        .await
        .unwrap_err()
        .code,
        ErrorCode::Conflict
    );
    assert!(
        close(&fixture.database.connection, fixture.turn, State::Cancelled)
            .await
            .unwrap()
            .is_empty()
    );
    assert_eq!(
        get(&fixture.database.connection, question.id)
            .await
            .unwrap(),
        expired
    );
}

#[tokio::test]
async fn keeps_explicit_responses() {
    for response in [
        Response::Answer(Answer::Text("Use the available context".into())),
        Response::Cancel,
    ] {
        let mut fixture = Fixture::new().await;
        let question = fixture.begin().await;
        let resolved = resolve(
            &fixture.database.connection,
            fixture.session,
            question.id,
            &response,
        )
        .await
        .unwrap();
        assert_eq!(
            expire(
                &mut fixture.database,
                fixture.turn,
                fixture.request.function_call_id.as_deref().unwrap(),
                &CancellationToken::new(),
                &fixture.events,
            )
            .await
            .unwrap(),
            resolved
        );
    }
}

#[tokio::test]
async fn preserves_stopped_waits() {
    let mut fixture = Fixture::new().await;
    let question = fixture.begin().await;
    let stop = CancellationToken::new();
    stop.cancel();
    assert_eq!(
        expire(
            &mut fixture.database,
            fixture.turn,
            fixture.request.function_call_id.as_deref().unwrap(),
            &stop,
            &fixture.events,
        )
        .await
        .unwrap_err()
        .code,
        ErrorCode::Cancelled
    );
    assert_eq!(
        get(&fixture.database.connection, question.id)
            .await
            .unwrap(),
        question
    );
}
