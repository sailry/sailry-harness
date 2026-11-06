use super::*;
use sailry_protocol::ErrorCode;

const SESSION: &str = "0f0e0d0c-0b0a-4908-8706-050403020100";

#[tokio::test]
async fn model_execution_excludes_delegation() {
    let mut fixture = crate::agent::delegation::tests::Fixture::new().await;
    assert!(available(&fixture.invocation));
    fixture.invocation.turn.roles.execution = sailry_protocol::conversation::Execution::Model;
    assert!(!available(&fixture.invocation));
}

#[tokio::test]
async fn model_execution_keeps_application_context() {
    let mut fixture = crate::agent::delegation::tests::Fixture::new().await;
    fixture.invocation.turn.roles = sailry_protocol::role::Snapshot {
        execution: sailry_protocol::conversation::Execution::Model,
        ..Default::default()
    };
    let tools = crate::agent::roles::bind(
        &fixture.store.ingress,
        &fixture.invocation,
        &CancellationToken::new(),
    );
    for name in crate::agent::roles::APPLY {
        let selected = tools
            .iter()
            .find(|entry| entry.tool.name.as_ref() == name)
            .unwrap();
        let text = context(&fixture.invocation, std::slice::from_ref(selected));
        assert!(text.starts_with(&format!(
            "Captured conversation revision: {}.",
            fixture.invocation.turn.revision
        )));
        assert!(!text.contains("delegation roles"));
    }
    assert!(context(&fixture.invocation, &[]).is_empty());
}

#[tokio::test]
async fn context_reports_concurrency_budget() {
    let mut fixture = crate::agent::delegation::tests::Fixture::new().await;
    assert_eq!(
        fixture.invocation.child_concurrency,
        sailry_protocol::team::CHILD_CONCURRENCY
    );
    fixture.invocation.child_concurrency = 3;
    let supervisor = crate::tasks::Supervisor::start(1);
    let tools = bind(
        &fixture.store.ingress,
        &supervisor.tasks,
        &fixture.invocation,
        &CancellationToken::new(),
    );
    let text = context(&fixture.invocation, &tools);
    assert!(text.contains("up to 3 children run at once"), "{text}");
    assert!(!context(&fixture.invocation, &[]).contains("Concurrency budget"));
    supervisor
        .shutdown(std::time::Duration::from_secs(5))
        .await
        .unwrap();
    fixture.store.shutdown().await.unwrap();
}

mod prepare {
    use super::*;

    fn rejected(arguments: Value) -> String {
        let error = prepare(&arguments, &["review".into()]).unwrap_err();
        assert_eq!(error.code, ErrorCode::InvalidRequest);
        error.message
    }

    #[test]
    fn accepts_listed_roles_and_reuse() {
        let roles = ["review".to_owned()];
        for arguments in [
            json!({"task":"Inspect"}),
            json!({"task":"Inspect","role":null,"background":true}),
            json!({"task":"Inspect","role":"review","title":"Inspect files"}),
            json!({"task":"Continue","session":SESSION,"worktree":null}),
            json!({"task":"Continue","session":SESSION.to_uppercase()}),
        ] {
            assert!(prepare(&arguments, &roles).is_ok(), "{arguments}");
        }
    }

    #[test]
    fn reports_the_first_invalid_field() {
        for (arguments, message) in [
            (json!([]), "Invalid delegation arguments"),
            (
                json!({"task":"x","extra":1}),
                "Invalid delegation arguments",
            ),
            (
                json!({"task":"x","background":"yes"}),
                "Invalid delegation background mode",
            ),
            (
                json!({"task":"x","session":"child"}),
                "Invalid child session ID",
            ),
            (
                json!({"task":"x","role":"writer"}),
                "Select a role listed in this tool",
            ),
            (json!({"task":"  "}), "A delegation task is required"),
            (
                json!({"task":"x","title":"Two\nlines"}),
                "Invalid delegation title",
            ),
            (json!({"task":"x","title":" "}), "Invalid delegation title"),
            (json!({"task":"x","worktree":"main"}), "Invalid worktree ID"),
        ] {
            assert_eq!(rejected(arguments), message);
        }
    }
}

#[test]
fn schema_matches_the_retired_package() {
    let schema = parameters();
    assert_eq!(schema["required"], json!(["task"]));
    assert_eq!(schema["additionalProperties"], json!(false));
    assert_eq!(schema["properties"]["session"]["pattern"], json!(UUID));
    assert_eq!(
        schema["properties"]["role"]["type"],
        json!(["string", "null"])
    );
    assert_eq!(schema["properties"]["title"]["minLength"], json!(1));
    assert_eq!(NAMES, ["spawn_agent", "guide_agent", "wait_agent"]);
}

#[test]
fn cards_keep_package_input_paths() {
    let spawn = serde_json::to_value(card(SPAWN)).unwrap();
    assert_eq!(spawn["input"]["content"]["path"], json!("/task"));
    let guide = serde_json::to_value(card(GUIDE)).unwrap();
    assert_eq!(guide["input"]["summary"]["format"], json!("session"));
    let wait = serde_json::to_value(card(WAIT)).unwrap();
    assert!(wait["input"]["content"].is_null());
}
