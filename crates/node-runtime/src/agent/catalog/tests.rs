use super::*;
use serde_json::json;

#[test]
fn moves_context_without_changing_output() {
    let context = crate::agent::canonical::Context {
        loaded_tools: Some(["read_file".into()].into()),
        ..Default::default()
    };
    let raw = json!({"data":{"lines":[{"text":"unchanged"}]},"sailry_context":context});
    let result = json(raw).unwrap();
    assert_eq!(
        result.structured_content,
        Some(json!({"data":{"lines":[{"text":"unchanged"}]}}))
    );
    assert_eq!(
        result.meta.unwrap()["sailry_context"]["loaded_tools"],
        json!(["read_file"])
    );
    let raw = json!({"sailry_context":"ordinary marker","output":7});
    assert_eq!(json(raw.clone()).unwrap().structured_content, Some(raw));
    assert!(json(json!({"sailry_context":{"captured":false,"loaded_tools":7}})).is_err());
}

mod concurrency {
    use super::*;

    struct Idle;
    #[async_trait]
    impl ToolProvider<CallContext> for Idle {
        async fn tools(&self, _: &CallContext) -> anyhow::Result<Vec<Tool>> {
            Ok(Vec::new())
        }
        async fn call(
            &self,
            _: &CallContext,
            _: &str,
            _: CallToolRequestParams,
            _: &Emitter,
        ) -> Result<CallToolResult, ErrorData> {
            unreachable!("concurrency checks never dispatch")
        }
    }

    fn registration(name: &'static str, read_only: Option<bool>) -> Registration {
        let mut tool = Tool::new(name, name, serde_json::Map::new());
        if let Some(read_only) = read_only {
            tool = tool.with_annotations(rmcp::model::ToolAnnotations::new().read_only(read_only));
        }
        Registration::new(tool, Arc::new(Idle), Presentation::default())
    }

    #[test]
    fn follows_read_only_registrations() {
        let mut ordered = registration("navigate", Some(true));
        ordered.ordered = true;
        let mut catalog = Catalog::new("session".into(), CancellationToken::new(), None);
        for entry in [
            registration("read", Some(true)),
            registration("write", Some(false)),
            registration("unmarked", None),
            ordered,
        ] {
            catalog.register(entry).unwrap();
        }

        assert!(catalog.concurrent("read"));
        for name in ["write", "unmarked", "navigate", "missing"] {
            assert!(!catalog.concurrent(name), "{name}");
        }
        assert!(catalog.entries["navigate"].read_only());
    }
}

#[test]
fn rejects_anonymous_calls() {
    assert!(
        CallContext::new(
            "session".into(),
            "",
            &CallToolRequestParams::new("write"),
            CancellationToken::new(),
            Default::default()
        )
        .is_err()
    );
}

#[test]
fn classifies_authoritative_tool_failures() {
    use crate::telemetry::metrics::ToolResult;
    use sailry_protocol::{ErrorCode, tool::Failure};
    let mut output = CallToolResult::structured(json!({"error":"PRIVATE_BUSINESS_DATA"}));
    let result = Ok(output.clone());
    assert_eq!(classify(&result, None), (ToolResult::Success, None));
    assert_eq!(
        classify(
            &result,
            Some(&Failure {
                code: Some(ErrorCode::NotFound)
            })
        ),
        (ToolResult::Error, Some(ErrorCode::NotFound))
    );
    assert_eq!(
        classify(&result, Some(&Failure { code: None })),
        (ToolResult::Error, None)
    );
    assert_eq!(
        classify(
            &result,
            Some(&Failure {
                code: Some(ErrorCode::PermissionDenied)
            })
        ),
        (ToolResult::Denied, Some(ErrorCode::PermissionDenied))
    );
    output.is_error = Some(true);
    assert_eq!(classify(&Ok(output), None), (ToolResult::Error, None));
    assert_eq!(
        classify(
            &Err(fault(Fault::new(ErrorCode::NotFound, "PRIVATE_DIAGNOSTIC"))),
            None
        ),
        (ToolResult::Error, Some(ErrorCode::NotFound))
    );
    assert_eq!(
        classify(
            &Err(ErrorData::internal_error(
                "PRIVATE_DIAGNOSTIC",
                Some(json!({"code":"PRIVATE_CODE"}))
            )),
            None
        ),
        (ToolResult::Error, None)
    );
}
