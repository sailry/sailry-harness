use crate::store::database::Database;
use sailry_protocol::NodeId;

#[tokio::test]
async fn uses_current_state_indexes() {
    let directory = tempfile::tempdir().unwrap();
    let database = Database::open(
        &directory.path().canonicalize().unwrap().join("node.turso"),
        NodeId([79; 32]),
        None,
    )
    .await
    .unwrap();
    for (sql, index) in [
        (
            "SELECT body FROM events WHERE json_extract(body,'$.kind')='sessions_reordered' ORDER BY cursor DESC LIMIT 1",
            "events_kind",
        ),
        (
            "SELECT body FROM agent_runs WHERE json_extract(body,'$.status')='running' UNION ALL SELECT body FROM agent_runs WHERE json_extract(body,'$.status')='stopping'",
            "agent_runs_status",
        ),
        (
            "SELECT body FROM agent_approvals WHERE json_extract(body,'$.state')='pending'",
            "agent_approvals_state",
        ),
        (
            "SELECT body FROM agent_questions WHERE json_extract(body,'$.state.kind')='pending'",
            "agent_questions_state",
        ),
        (
            "SELECT body FROM agent_background WHERE state='running' UNION ALL SELECT body FROM agent_background WHERE state='pending'",
            "agent_background_state",
        ),
        (
            "SELECT body FROM dispatch_jobs WHERE status='running'",
            "dispatch_jobs_status",
        ),
        (
            "SELECT result FROM requests WHERE status='admitted' UNION ALL SELECT result FROM requests WHERE status='unknown'",
            "requests_status",
        ),
    ] {
        let mut query = database
            .connection
            .prepare(&format!("EXPLAIN QUERY PLAN {sql}"))
            .await
            .unwrap();
        let mut rows = query.query(()).await.unwrap();
        let mut plan = Vec::new();
        while let Some(row) = rows.next().await.unwrap() {
            plan.push(row.get::<String>(3).unwrap());
        }
        assert!(
            plan.iter().any(|step| step.contains(index)),
            "{index}: {plan:?}"
        );
    }
}

#[tokio::test]
async fn includes_live_records_during_updates() {
    for table in ["agent_runs", "agent_questions"] {
        let directory = tempfile::tempdir().unwrap();
        let database = Database::open(&directory.path().join("node.turso"), NodeId([80; 32]), None)
            .await
            .unwrap();
        database
            .connection
            .pragma_update("foreign_keys", "OFF")
            .await
            .unwrap();
        let turn = sailry_protocol::TurnId::new().to_string();
        if table == "agent_runs" {
            database
                .connection
                .execute(
                    "INSERT INTO agent_runs(turn,ready,body,hooks) VALUES(?1,1,?2,?3)",
                    (
                        turn,
                        br#"{"status":"running","private_marker":true}"#.as_slice(),
                        serde_json::to_vec(&sailry_protocol::hooks::Settings::default()).unwrap(),
                    ),
                )
                .await
                .unwrap();
        } else {
            database.connection.execute(
                "INSERT INTO agent_questions(id,turn,call,body) VALUES('question',?1,'call',?2)",
                (turn, br#"{"state":{"kind":"pending"},"private_marker":true}"#.as_slice())
            ).await.unwrap();
        }
        for archives in [false, true] {
            let error = if archives {
                super::stored(&database.connection).await
            } else {
                super::startup(&database.connection).await
            }
            .unwrap_err()
            .to_string();
            assert!(error.contains(&format!("{table}.body")), "{error}");
            assert!(!error.contains("private_marker"));
        }
    }
}
