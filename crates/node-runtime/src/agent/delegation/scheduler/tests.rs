use super::*;
use crate::{agent::delegation::tests::Fixture, tasks::Supervisor};
use sailry_client::Client;
use sailry_link::Local;
use sailry_protocol::{Command, Output};

async fn claim(fixture: &Fixture, invocation: &Invocation, token: CancellationToken) {
    assert_eq!(
        fixture
            .store
            .ingress
            .claim_child(invocation.turn.id, token)
            .await
            .unwrap(),
        Status::Running
    );
}

async fn completed(fixture: &Fixture, invocation: &Invocation) {
    let ingress = &fixture.store.ingress;
    let turn = invocation.turn.id;
    ingress
        .finish_run(turn, Status::Completed, None, None)
        .await
        .unwrap();
    ingress.agents.finish(turn);
}

async fn guided(fixture: &Fixture, invocation: &Invocation) -> TurnId {
    let ingress = &fixture.store.ingress;
    let client = Client::new(Arc::new(Local::new(
        ingress.node,
        ingress.node,
        ingress.clone(),
    )));
    let state = ingress
        .child_state(fixture.invocation.turn.id, invocation.turn.session)
        .await
        .unwrap();
    let Output::QueuedTurn(turn) = client
        .execute(client.prepare(Command::GuideTurn {
            recipient: invocation.turn.id,
            expected_revision: state.run.revision,
            message: "Continue the admitted child".into(),
        }))
        .await
        .unwrap()
    else {
        panic!("guided turn expected")
    };
    turn.id
}

async fn inject(fixture: &Fixture, sql: String) {
    fixture
        .store
        .ingress
        .with_storage(move |connection| {
            Box::pin(async move {
                connection
                    .execute_batch(&sql)
                    .await
                    .map_err(crate::store::database::storage_error)
            })
        })
        .await
        .unwrap();
}

async fn close(fixture: Fixture, supervisor: Supervisor) {
    fixture
        .store
        .ingress
        .finish_run(fixture.invocation.turn.id, Status::Cancelled, None, None)
        .await
        .unwrap();
    fixture
        .store
        .ingress
        .agents
        .finish(fixture.invocation.turn.id);
    supervisor.shutdown(Duration::from_secs(5)).await.unwrap();
    fixture.store.shutdown().await.unwrap();
}

async fn outcome(
    fixture: &Fixture,
    supervisor: &Supervisor,
    invocation: Invocation,
    token: CancellationToken,
    owner: CancellationToken,
) -> (TurnId, Result<(Status, Option<String>), Fault>) {
    tokio::time::timeout(
        Duration::from_secs(10),
        resume(
            &fixture.store.ingress,
            &supervisor.tasks,
            invocation,
            token,
            owner,
        ),
    )
    .await
    .expect("child resume deadline")
}

#[tokio::test]
async fn preserves_run_failure() {
    let fixture = Box::pin(Fixture::new()).await;
    let supervisor = Supervisor::start(1);
    let token = CancellationToken::new();
    let invocation = fixture.child(token.clone()).await;
    let session = invocation.turn.session;
    let turn = invocation.turn.id;
    let (ended, result) = outcome(
        &fixture,
        &supervisor,
        invocation,
        token,
        CancellationToken::new(),
    )
    .await;
    assert_eq!(ended, turn);
    let error = result.unwrap_err();
    let stored = fixture
        .store
        .ingress
        .child_state(fixture.invocation.turn.id, session)
        .await
        .unwrap()
        .run;
    assert_eq!(stored.status, Status::Failed);
    let original = stored.error.unwrap();
    assert_eq!(error, original);
    close(fixture, supervisor).await;
}

#[tokio::test]
async fn preserves_claim_failure() {
    let fixture = Box::pin(Fixture::new()).await;
    let supervisor = Supervisor::start(1);
    let token = CancellationToken::new();
    let invocation = fixture.child(token.clone()).await;
    let session = invocation.turn.session;
    let turn = invocation.turn.id;
    claim(&fixture, &invocation, token.clone()).await;
    let next = guided(&fixture, &invocation).await;
    completed(&fixture, &invocation).await;
    inject(&fixture, format!(
        "CREATE TRIGGER lose_claim BEFORE UPDATE ON agent_runs WHEN NEW.turn='{next}' BEGIN SELECT RAISE(ABORT,'injected claim failure'); END;
         CREATE TRIGGER lose_retention BEFORE UPDATE ON agent_queues WHEN NEW.session='{session}' AND NEW.paused=1 BEGIN SELECT RAISE(ABORT,'injected retention failure'); END;"
    )).await;
    let (ended, result) = outcome(
        &fixture,
        &supervisor,
        invocation,
        token,
        CancellationToken::new(),
    )
    .await;
    assert_eq!(ended, turn);
    let error = result.unwrap_err();
    assert_eq!(error.code, ErrorCode::Internal);
    assert_eq!(
        error.message,
        "Node storage operation failed; Node storage operation failed"
    );
    inject(
        &fixture,
        "DROP TRIGGER lose_claim; DROP TRIGGER lose_retention;".into(),
    )
    .await;
    let claimed = fixture
        .store
        .ingress
        .claim_guidance(turn, CancellationToken::new())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(claimed.turn.id, next);
    fixture
        .store
        .ingress
        .finish_queued_child(claimed.turn.id, Status::Cancelled, None)
        .await
        .unwrap();
    fixture.store.ingress.agents.finish(claimed.turn.id);
    close(fixture, supervisor).await;
}

#[tokio::test]
async fn reports_retention_failure() {
    let fixture = Box::pin(Fixture::new()).await;
    let supervisor = Supervisor::start(1);
    let owner = CancellationToken::new();
    let token = owner.child_token();
    let invocation = fixture.child(token.clone()).await;
    let session = invocation.turn.session;
    let turn = invocation.turn.id;
    claim(&fixture, &invocation, token.clone()).await;
    completed(&fixture, &invocation).await;
    inject(&fixture, format!(
        "CREATE TRIGGER lose_retention BEFORE UPDATE ON agent_queues WHEN NEW.session='{session}' AND NEW.paused=1 BEGIN SELECT RAISE(ABORT,'injected retention failure'); END;"
    )).await;
    owner.cancel();
    let (ended, result) = outcome(&fixture, &supervisor, invocation, token, owner).await;
    assert_eq!(ended, turn);
    let error = result.unwrap_err();
    assert_eq!(error.code, ErrorCode::Internal);
    assert_eq!(error.message, "Node storage operation failed");
    inject(&fixture, "DROP TRIGGER lose_retention;".into()).await;
    close(fixture, supervisor).await;
}

#[tokio::test]
async fn returns_last_guided_turn() {
    let fixture = Box::pin(Fixture::new()).await;
    let supervisor = Supervisor::start(1);
    let token = CancellationToken::new();
    let invocation = fixture.child(token.clone()).await;
    let session = invocation.turn.session;
    claim(&fixture, &invocation, token.clone()).await;
    let next = guided(&fixture, &invocation).await;
    completed(&fixture, &invocation).await;
    let (ended, result) = outcome(
        &fixture,
        &supervisor,
        invocation,
        token,
        CancellationToken::new(),
    )
    .await;
    assert_eq!(ended, next);
    let error = result.unwrap_err();
    let state = fixture
        .store
        .ingress
        .child_state(fixture.invocation.turn.id, session)
        .await
        .unwrap();
    assert_eq!(state.run.turn, next);
    assert_eq!(state.run.status, Status::Failed);
    assert_eq!(state.run.error.unwrap(), error);
    let pending = fixture
        .store
        .ingress
        .with_storage(move |connection| {
            Box::pin(async move {
                let row = connection
                    .prepare("SELECT count(*) FROM agent_pending WHERE turn=?1")
                    .await
                    .map_err(crate::store::database::storage_error)?
                    .query_row([next.to_string()])
                    .await
                    .map_err(crate::store::database::storage_error)?;
                row.get::<i64>(0)
                    .map_err(crate::store::database::storage_error)
            })
        })
        .await
        .unwrap();
    assert_eq!(pending, 1);
    close(fixture, supervisor).await;
}

#[tokio::test]
async fn preserves_stop_outcomes() {
    for stopped in [false, true] {
        let fixture = Box::pin(Fixture::new()).await;
        let supervisor = Supervisor::start(1);
        let owner = CancellationToken::new();
        let token = owner.child_token();
        let invocation = fixture.child(token.clone()).await;
        let session = invocation.turn.session;
        let turn = invocation.turn.id;
        if stopped {
            fixture.store.ingress.agents.stopped.cancel();
        } else {
            owner.cancel();
        }
        let (ended, result) = outcome(&fixture, &supervisor, invocation, token, owner).await;
        let expected = if stopped {
            Status::Interrupted
        } else {
            Status::Cancelled
        };
        assert_eq!(ended, turn);
        assert_eq!(result.unwrap(), (expected, None));
        let run = fixture
            .store
            .ingress
            .child_state(fixture.invocation.turn.id, session)
            .await
            .unwrap()
            .run;
        assert_eq!(run.status, expected);
        if stopped {
            assert_eq!(run.error.unwrap().code, ErrorCode::OutcomeUnknown);
        } else {
            assert!(run.error.is_none());
        }
        close(fixture, supervisor).await;
    }
}
