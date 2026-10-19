use super::*;
use std::sync::Mutex;

struct Context {
    session: &'static str,
    cancelled: bool,
}
#[async_trait]
impl adk_core::ReadonlyContext for Context {
    fn invocation_id(&self) -> &str {
        "invocation"
    }
    fn agent_name(&self) -> &str {
        "neutral"
    }
    fn user_id(&self) -> &str {
        "user"
    }
    fn app_name(&self) -> &str {
        "app"
    }
    fn session_id(&self) -> &str {
        self.session
    }
    fn branch(&self) -> &str {
        ""
    }
    fn user_content(&self) -> &adk_core::Content {
        static CONTENT: std::sync::OnceLock<adk_core::Content> = std::sync::OnceLock::new();
        CONTENT.get_or_init(|| adk_core::Content::new("user"))
    }
}
impl adk_core::CallbackContext for Context {
    fn artifacts(&self) -> Option<Arc<dyn adk_core::Artifacts>> {
        None
    }
}
#[async_trait]
impl ToolContext for Context {
    fn function_call_id(&self) -> &str {
        "call"
    }
    fn actions(&self) -> adk_core::EventActions {
        Default::default()
    }
    fn set_actions(&self, _: adk_core::EventActions) {}
    fn is_cancelled(&self) -> bool {
        self.cancelled
    }
    async fn search_memory(&self, _: &str) -> adk_core::Result<Vec<adk_core::MemoryEntry>> {
        Ok(vec![])
    }
}

struct Probe {
    schema: Value,
    calls: Arc<Mutex<Vec<Value>>>,
}
#[async_trait]
impl Tool for Probe {
    fn name(&self) -> &str {
        "neutral"
    }
    fn description(&self) -> &str {
        "Neutral schema probe"
    }
    fn parameters_schema(&self) -> Option<Value> {
        Some(self.schema.clone())
    }
    fn declaration(&self) -> Value {
        json!({"custom":"preserved","parameters":self.schema})
    }
    fn enhanced_description(&self) -> String {
        "Enhanced probe".into()
    }
    fn response_schema(&self) -> Option<Value> {
        Some(json!({"type":"object"}))
    }
    fn required_scopes(&self) -> &[&str] {
        &["scope"]
    }
    fn annotations(&self) -> Option<&Value> {
        static ANNOTATIONS: std::sync::OnceLock<Value> = std::sync::OnceLock::new();
        Some(ANNOTATIONS.get_or_init(|| json!({"readOnlyHint":true})))
    }
    fn origin(&self) -> &str {
        "fixture:origin"
    }
    fn effect(&self) -> adk_core::ToolEffect {
        adk_core::ToolEffect::Idempotent
    }
    fn timeout_override(&self) -> Option<Option<std::time::Duration>> {
        Some(Some(std::time::Duration::from_secs(17)))
    }
    fn is_long_running(&self) -> bool {
        true
    }
    fn is_builtin(&self) -> bool {
        true
    }
    fn is_read_only(&self) -> bool {
        true
    }
    fn is_concurrency_safe(&self) -> bool {
        true
    }
    fn is_agent_delegation(&self) -> bool {
        true
    }
    async fn execute(&self, _: Arc<dyn ToolContext>, arguments: Value) -> adk_core::Result<Value> {
        self.calls.lock().unwrap().push(arguments.clone());
        Ok(arguments)
    }
}

fn schema() -> Value {
    json!({"type":"object","additionalProperties":false,"required":["query"],"properties":{
        "query":{"type":"string"},"context":{"type":"integer","minimum":0,"default":0},
        "choice":{"enum":["public-option"]},"settings":{"type":"object","properties":{"enabled":{"type":"boolean"}},"additionalProperties":false}
    }})
}
fn probe(schema: Value) -> (Arc<Probe>, Arc<Mutex<Vec<Value>>>) {
    let calls = Arc::new(Mutex::new(vec![]));
    (
        Arc::new(Probe {
            schema,
            calls: calls.clone(),
        }),
        calls,
    )
}
fn context(session: &'static str) -> Arc<dyn ToolContext> {
    Arc::new(Context {
        session,
        cancelled: false,
    })
}

#[tokio::test]
async fn preserves_defaults_and_large_values() {
    for session in ["parent", "child"] {
        let (probe, calls) = probe(schema());
        let tool = Validated::bind(probe, session.into(), CancellationToken::new()).unwrap();
        for arguments in [
            json!({"query":"x".repeat(8193)}),
            json!({"query":"x","context":8}),
            json!({"query":"x","context":1000}),
        ] {
            assert_eq!(
                tool.execute(context(session), arguments.clone())
                    .await
                    .unwrap(),
                arguments
            );
            assert_eq!(calls.lock().unwrap().last(), Some(&arguments));
        }
        assert!(calls.lock().unwrap()[0].get("context").is_none());
    }
}

#[tokio::test]
async fn rejects_before_execution_without_leaking_values() {
    let (probe, calls) = probe(schema());
    let tool = Validated::bind(probe, "parent".into(), CancellationToken::new()).unwrap();
    const SECRET: &str = "private-fixture-value";
    for (arguments, field, reason) in [
        (
            json!({"query":SECRET,"context":-1}),
            "/context",
            "at least 0",
        ),
        (
            json!({"query":SECRET,"context":SECRET}),
            "/context",
            "expected",
        ),
        (
            json!({"query":SECRET,"context":null}),
            "/context",
            "expected",
        ),
        (
            json!({"query":SECRET,"choice":SECRET}),
            "/choice",
            "declared option",
        ),
        (json!({"context":0}), "/query", "required"),
        (
            json!({"query":SECRET,(SECRET):true}),
            "/",
            "unsupported field",
        ),
        (
            json!({"query":SECRET,"settings":{"enabled":SECRET}}),
            "/settings/enabled",
            "expected",
        ),
        (
            json!({"query":SECRET,"settings":{(SECRET):true}}),
            "/settings",
            "unsupported field",
        ),
    ] {
        let result = tool.execute(context("parent"), arguments).await.unwrap();
        assert_eq!(result["isError"], true, "{result}");
        let message = result["error"]["message"].as_str().unwrap();
        assert!(
            message.contains(field) && message.contains(reason),
            "{result}"
        );
        assert!(!result.to_string().contains(SECRET));
    }
    assert!(calls.lock().unwrap().is_empty());
}

#[tokio::test]
async fn preserves_session_and_cancellation_guards() {
    let (probe, calls) = probe(schema());
    let stop = CancellationToken::new();
    let tool = Validated::bind(probe, "parent".into(), stop.clone()).unwrap();
    assert!(
        tool.execute(context("other"), json!({}))
            .await
            .unwrap_err()
            .to_string()
            .contains("session")
    );
    assert!(
        tool.execute(
            Arc::new(Context {
                session: "parent",
                cancelled: true
            }),
            json!({})
        )
        .await
        .unwrap_err()
        .to_string()
        .contains("cancelled")
    );
    stop.cancel();
    assert!(
        tool.execute(context("parent"), json!({}))
            .await
            .unwrap_err()
            .to_string()
            .contains("cancelled")
    );
    assert!(calls.lock().unwrap().is_empty());
}

#[tokio::test]
async fn uses_final_schema_and_preserves_metadata() {
    let mut schema = schema();
    schema["properties"]["context"]["maximum"] = json!(8);
    let (probe, calls) = probe(schema.clone());
    let tool = Validated::bind(probe.clone(), "parent".into(), CancellationToken::new()).unwrap();
    assert_eq!(tool.parameters_schema(), Some(schema));
    assert_eq!(tool.declaration(), probe.declaration());
    assert_eq!(tool.enhanced_description(), probe.enhanced_description());
    assert_eq!(tool.response_schema(), probe.response_schema());
    assert_eq!(tool.required_scopes(), ["scope"]);
    assert_eq!(tool.annotations(), probe.annotations());
    assert_eq!(tool.origin(), probe.origin());
    assert_eq!(tool.effect(), probe.effect());
    assert_eq!(tool.timeout_override(), probe.timeout_override());
    assert!(
        tool.is_builtin()
            && tool.is_long_running()
            && tool.is_agent_delegation()
            && tool.is_read_only()
            && tool.is_concurrency_safe()
    );
    assert_eq!(
        tool.execute(context("parent"), json!({"query":"x","context":9}))
            .await
            .unwrap()["isError"],
        true
    );
    assert!(calls.lock().unwrap().is_empty());
    assert_eq!(
        tool.execute(context("parent"), json!({"query":"x","context":8}))
            .await
            .unwrap()["context"],
        8
    );
}

#[tokio::test]
async fn resolves_local_references_and_conditions() {
    let schema = json!({"$defs":{"count":{"type":"integer","minimum":0}},"type":"object","properties":{"action":{"enum":["read","write"]},"count":{"$ref":"#/$defs/count"}},"allOf":[{"if":{"properties":{"action":{"const":"write"}},"required":["action"]},"then":{"required":["count"]}}]});
    let (probe, calls) = probe(schema);
    let tool = Validated::bind(probe, "parent".into(), CancellationToken::new()).unwrap();
    assert_eq!(
        tool.execute(context("parent"), json!({"action":"read"}))
            .await
            .unwrap(),
        json!({"action":"read"})
    );
    for arguments in [
        json!({"action":"write"}),
        json!({"action":"write","count":-1}),
    ] {
        assert_eq!(
            tool.execute(context("parent"), arguments).await.unwrap()["isError"],
            true
        );
    }
    assert_eq!(calls.lock().unwrap().len(), 1);
}

#[test]
fn refuses_invalid_or_external_schemas() {
    for schema in [
        json!({"type":"private-invalid-type"}),
        json!({"$ref":"https://example.invalid/private-schema"}),
        json!({"$ref":"file:///private/schema"}),
    ] {
        let (probe, _) = probe(schema);
        let error = Validated::bind(probe, "parent".into(), CancellationToken::new())
            .err()
            .unwrap();
        assert_eq!(
            error.message,
            "tool parameter schema is invalid or unresolved"
        );
    }
}
