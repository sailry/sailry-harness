use super::*;
use serde_json::{Value, json};

fn unknown(data: Value) -> Value {
    json!({ "kind": "future_result", "data": data })
}

mod history {
    use super::*;

    #[test]
    fn unknown_kinds_discard_payload() {
        for data in [
            json!({ "private": "value" }),
            json!([1, true]),
            json!("text"),
            json!(42),
            json!(null),
        ] {
            let event: Event = serde_json::from_value(unknown(data)).unwrap();
            assert_eq!(event, Event::Unsupported);
            assert_eq!(
                serde_json::to_value(event).unwrap(),
                json!({ "kind": "unsupported" })
            );
        }
        assert_eq!(
            serde_json::from_value::<Event>(json!({ "kind": "future_event" })).unwrap(),
            Event::Unsupported
        );
    }

    #[test]
    fn malformed_known_kinds_remain_errors() {
        for value in [
            json!({ "kind": "worktree_removed", "data": {} }),
            json!({ "kind": "worktree_removed", "data": { "id": 1 } }),
            json!({ "kind": "worktree_removed", "data": { "id": "invalid" } }),
            json!({ "kind": "notifications_dismissed", "data": { "ids": "invalid" } }),
            json!({ "data": {} }),
            json!({ "kind": 1, "data": {} }),
        ] {
            assert!(serde_json::from_value::<Event>(value).is_err());
        }
    }
}

mod receipts {
    use super::*;

    #[test]
    fn unknown_kinds_are_not_supported() {
        for data in [
            json!({ "private": "value" }),
            json!([1, true]),
            json!("text"),
            json!(42),
            json!(null),
        ] {
            let output: Output = serde_json::from_value(unknown(data.clone())).unwrap();
            assert_eq!(output, Output::Unsupported);
            assert!(!output.supported());
            assert_eq!(
                serde_json::to_value(output).unwrap(),
                json!({ "kind": "unsupported" })
            );
            let dispatch: crate::dispatch::Output = serde_json::from_value(unknown(data)).unwrap();
            assert_eq!(dispatch, crate::dispatch::Output::Unsupported);
            assert!(!dispatch.supported());
        }
    }

    #[test]
    fn nested_unknown_results_are_not_supported() {
        let completed = |output| Output::RequestOutcome {
            id: RequestId::new(),
            outcome: RequestOutcome::Completed(Box::new(Ok(output))),
        };
        assert!(!completed(completed(Output::Unsupported)).supported());
        let values = [
            json!({ "kind": "plugin_transaction", "data": [{ "kind": "browser_completed" }, unknown(json!({}))] }),
            json!({ "kind": "dispatch", "data": unknown(json!({})) }),
            json!({ "kind": "dispatch", "data": { "kind": "result", "data": { "kind": "completed", "data": { "Ok": unknown(json!({})) } } } }),
            json!({ "kind": "request_outcome", "data": { "id": RequestId::new(), "outcome": { "kind": "completed", "data": { "Ok": { "kind": "plugin_transaction", "data": [{ "kind": "dispatch", "data": unknown(json!({})) }] } } } } }),
        ];
        for value in values {
            assert!(!serde_json::from_value::<Output>(value).unwrap().supported());
        }
    }

    #[test]
    fn known_results_and_faults_remain_supported() {
        let fault = Fault::new(ErrorCode::Unavailable, "unavailable");
        for output in [
            Output::BrowserCompleted,
            Output::PluginTransaction(vec![Output::BrowserCompleted]),
            Output::Dispatch(crate::dispatch::Output::Removed),
            Output::Dispatch(crate::dispatch::Output::Result(RequestOutcome::Unknown)),
            Output::RequestOutcome {
                id: RequestId::new(),
                outcome: RequestOutcome::Completed(Box::new(Err(fault))),
            },
        ] {
            assert!(output.supported());
        }
    }

    #[test]
    fn malformed_known_kinds_remain_errors() {
        for value in [
            json!({ "kind": "file_transfer_cancelled", "data": {} }),
            json!({ "kind": "file_transfer_cancelled", "data": { "stream": 1 } }),
            json!({ "kind": "plugin_transaction", "data": {} }),
            json!({ "kind": "browser_completed", "data": "invalid" }),
            json!({ "data": {} }),
            json!({ "kind": 1, "data": {} }),
        ] {
            assert!(serde_json::from_value::<Output>(value).is_err());
        }
        for value in [
            json!({ "kind": "published", "data": {} }),
            json!({ "kind": "jobs", "data": "invalid" }),
            json!({ "kind": "removed", "data": "invalid" }),
        ] {
            assert!(serde_json::from_value::<crate::dispatch::Output>(value).is_err());
        }
    }
}
