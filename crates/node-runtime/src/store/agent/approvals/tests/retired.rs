use super::*;

async fn startup(fixture: &mut Fixture) -> Approval {
    let mut approval = fixture.begin().await;
    let run = super::super::super::runs::get(&fixture.database.connection, fixture.turn)
        .await
        .unwrap();
    approval.startup = Some(sailry_protocol::code::Startup {
        worktree: run.worktree,
        root: String::new(),
        server: "retired".into(),
        executable: "/retired/language-server".into(),
        revision: "a".repeat(64),
    });
    put(&fixture.database.connection, &approval).await.unwrap();
    approval
}

#[tokio::test]
async fn retains_startup_data_without_execution() {
    let mut fixture = Fixture::new().await;
    let approval = startup(&mut fixture).await;
    fixture.approve(approval.id).await;
    let before = get(&fixture.database.connection, approval.id)
        .await
        .unwrap();
    let error = authorize(&mut fixture.database, fixture.turn, &fixture.request, None)
        .await
        .unwrap_err();
    assert_eq!(error.code, ErrorCode::Unavailable);
    let after = get(&fixture.database.connection, approval.id)
        .await
        .unwrap();
    assert_eq!(before, after);
    assert_eq!(after.startup, approval.startup);
    let claimed: bool = fixture
        .database
        .connection
        .prepare("SELECT claimed FROM agent_approvals WHERE id=?1")
        .await
        .unwrap()
        .query_row([approval.id.to_string()])
        .await
        .unwrap()
        .get(0)
        .unwrap();
    assert!(!claimed);
}

#[tokio::test]
async fn recovery_settles_pending_startups() {
    let mut fixture = Fixture::new().await;
    let mut approval = startup(&mut fixture).await;
    let node = fixture.database.node;
    fixture.database.close().await.unwrap();
    let database = Database::open(&fixture.path, node, None).await.unwrap();
    let settled = get(&database.connection, approval.id).await.unwrap();
    approval.state = ApprovalState::Interrupted;
    assert_eq!(settled, approval);
    assert_eq!(
        runs::get(&database.connection, fixture.turn)
            .await
            .unwrap()
            .status,
        Status::Interrupted
    );
    database.close().await.unwrap();
}
