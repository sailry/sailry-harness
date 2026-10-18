use crate::store::database::Database;
use sailry_protocol::NodeId;

#[test]
fn uses_current_state_indexes() {
    let directory = tempfile::tempdir().unwrap();
    let database = Database::open(
        &directory
            .path()
            .canonicalize()
            .unwrap()
            .join("node.sqlite3"),
        NodeId([79; 32]),
        None,
    )
    .unwrap();
    for (sql, index) in [
        (
            "SELECT body FROM events WHERE json_extract(body,'$.kind')='sessions_reordered' ORDER BY cursor DESC LIMIT 1",
            "events_kind",
        ),
        (
            "SELECT body FROM agent_runs WHERE json_extract(body,'$.status') IN ('running','stopping')",
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
            "SELECT body FROM agent_background WHERE state IN ('running','pending')",
            "agent_background_state",
        ),
        (
            "SELECT body FROM dispatch_jobs WHERE status='running'",
            "dispatch_jobs_status",
        ),
        (
            "SELECT result FROM requests WHERE status IN ('admitted','unknown')",
            "requests_status",
        ),
    ] {
        let mut query = database
            .connection
            .prepare(&format!("EXPLAIN QUERY PLAN {sql}"))
            .unwrap();
        let plan = query
            .query_map([], |row| row.get::<_, String>(3))
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        assert!(
            plan.iter().any(|step| step.contains(index)),
            "{index}: {plan:?}"
        );
    }
}

#[test]
fn includes_live_records_during_upgrades() {
    for table in ["agent_runs", "agent_questions"] {
        let directory = tempfile::tempdir().unwrap();
        let database = Database::open(
            &directory
                .path()
                .canonicalize()
                .unwrap()
                .join("node.sqlite3"),
            NodeId([80; 32]),
            None,
        )
        .unwrap();
        database
            .connection
            .pragma_update(None, "foreign_keys", false)
            .unwrap();
        let turn = sailry_protocol::TurnId::new().to_string();
        if table == "agent_runs" {
            database
                .connection
                .execute(
                    "INSERT INTO agent_runs(turn,ready,body,hooks) VALUES(?1,1,?2,?3)",
                    rusqlite::params![
                        turn,
                        br#"{"status":"running","private_marker":true}"#.as_slice(),
                        serde_json::to_vec(&sailry_protocol::hooks::Settings::default()).unwrap()
                    ],
                )
                .unwrap();
        } else {
            database.connection.execute(
                "INSERT INTO agent_questions(id,turn,call,body) VALUES('question',?1,'call',?2)",
                rusqlite::params![turn, br#"{"state":{"kind":"pending"},"private_marker":true}"#.as_slice()],
            ).unwrap();
        }
        for archives in [false, true] {
            let error = if archives {
                super::stored(&database.connection)
            } else {
                super::startup(&database.connection)
            }
            .unwrap_err()
            .to_string();
            assert!(error.contains(&format!("{table}.body")), "{error}");
            assert!(!error.contains("private_marker"));
        }
    }
}
