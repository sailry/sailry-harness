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
    use crate::agent::{
        canonical::{Context, EXECUTABLE_KEY},
        controls::Controls,
        engine::{ConversationEffect, Effect, Outcome, tools::Tools},
        model::{Content, Message},
    };
    use crate::store::agent::RuntimeSession as Session;
    use std::{
        sync::{
            Mutex,
            atomic::{AtomicBool, AtomicUsize, Ordering},
        },
        time::Duration,
    };
    use tokio::sync::{Barrier, mpsc};

    /// Logs when each call starts and ends. Calls to `read` wait until two reads
    /// have started, so a pair of reads finishes only if the two overlap. Calls
    /// to `slow` never finish.
    struct Probe {
        log: Mutex<Vec<String>>,
        pair: Barrier,
        reads: AtomicUsize,
        wrote: AtomicBool,
    }
    impl Default for Probe {
        fn default() -> Self {
            Self {
                log: Mutex::default(),
                pair: Barrier::new(2),
                reads: AtomicUsize::new(0),
                wrote: AtomicBool::new(false),
            }
        }
    }
    impl Probe {
        fn position(&self, entry: &str) -> usize {
            let log = self.log.lock().unwrap();
            log.iter()
                .position(|logged| logged == entry)
                .unwrap_or_else(|| panic!("{entry} was not logged: {log:?}"))
        }
    }
    #[async_trait]
    impl ToolProvider<CallContext> for Probe {
        async fn tools(&self, _: &CallContext) -> anyhow::Result<Vec<Tool>> {
            Ok(Vec::new())
        }
        async fn call(
            &self,
            _: &CallContext,
            id: &str,
            call: CallToolRequestParams,
            _: &Emitter,
        ) -> Result<CallToolResult, ErrorData> {
            self.log.lock().unwrap().push(format!("start {id}"));
            match call.name.as_ref() {
                "read" => {
                    self.pair.wait().await;
                    self.reads.fetch_add(1, Ordering::SeqCst);
                }
                "slow" => std::future::pending::<()>().await,
                _ => self.wrote.store(true, Ordering::SeqCst),
            }
            self.log.lock().unwrap().push(format!("end {id}"));
            Ok(CallToolResult::structured(json!({ "id": id })))
        }
    }

    fn registration(
        name: &'static str,
        read_only: Option<bool>,
        concurrent: bool,
        provider: Arc<Probe>,
    ) -> Registration {
        let mut tool = Tool::new(name, name, serde_json::Map::new());
        if let Some(read_only) = read_only {
            tool = tool.with_annotations(rmcp::model::ToolAnnotations::new().read_only(read_only));
        }
        let mut registration = Registration::new(tool, provider, Presentation::default());
        registration.concurrent = concurrent;
        registration
    }

    /// A catalog whose `read` and `slow` tools are concurrent reads and whose
    /// `write` tool runs alone.
    fn catalog(probe: &Arc<Probe>) -> Catalog {
        let mut catalog = Catalog::new("session".into(), CancellationToken::new(), None);
        for entry in [
            registration("read", Some(true), true, probe.clone()),
            registration("slow", Some(true), true, probe.clone()),
            registration("write", Some(false), false, probe.clone()),
        ] {
            catalog.register(entry).unwrap();
        }
        catalog
    }

    #[test]
    fn requires_opted_in_reads() {
        let probe = Arc::new(Probe::default());
        let mut catalog = Catalog::new("session".into(), CancellationToken::new(), None);
        for entry in [
            registration("read", Some(true), true, probe.clone()),
            // MCP, browser and plugin script registrations are read-only but
            // never opt in.
            registration("reader", Some(true), false, probe.clone()),
            registration("write", Some(false), true, probe.clone()),
            registration("unmarked", None, true, probe.clone()),
        ] {
            catalog.register(entry).unwrap();
        }

        assert!(catalog.concurrent(&CallToolRequestParams::new("read")));
        for name in ["reader", "write", "unmarked", "missing"] {
            assert!(
                !catalog.concurrent(&CallToolRequestParams::new(name)),
                "{name}"
            );
        }
        catalog.serial = true;
        assert!(!catalog.concurrent(&CallToolRequestParams::new("read")));
    }

    /// Answers `(id, tool)` requests through the catalog and returns the saved
    /// results, stopping the run once `stop_after` reads have finished.
    async fn execute(
        probe: &Arc<Probe>,
        requests: &[(&'static str, &'static str)],
        stop_after: Option<usize>,
    ) -> Vec<(String, bool)> {
        let request = requests
            .iter()
            .fold(Message::assistant(), |message, (id, name)| {
                let parameters = if *name == "malformed" {
                    Err(ErrorData::invalid_params("malformed arguments", None))
                } else {
                    Ok(CallToolRequestParams::new(*name))
                };
                message.with_tool_request_with_metadata(
                    *id,
                    parameters,
                    None,
                    Some(json!({EXECUTABLE_KEY: true})),
                )
            });
        let session = Session {
            id: "session".into(),
            conversation: vec![Message::user().with_text("use tools"), request],
            context: Context::default(),
            records: Vec::new(),
            tool_names: Default::default(),
            capability: None,
            reviewed: false,
            state: Default::default(),
            thinking: None,
            invocation: None,
        };
        let operation = Tools::new(
            Arc::new(catalog(probe)),
            Arc::new(Controls::default()),
            sailry_protocol::TurnId::new(),
        );
        let stop = CancellationToken::new();
        let (sender, _receiver) = mpsc::unbounded_channel();
        let emit = Emitter::new(sender, stop.clone());
        let run =
            tokio::spawn(
                async move { operation.run(&session, &session.conversation, &emit).await },
            );
        if let Some(reads) = stop_after {
            while probe.reads.load(Ordering::SeqCst) < reads {
                tokio::task::yield_now().await;
            }
            stop.cancel();
        }
        let outcome = tokio::time::timeout(Duration::from_secs(5), run)
            .await
            .expect("the tool step should finish")
            .unwrap()
            .unwrap();
        let Outcome::Applied(transition) = outcome else {
            panic!("tool requests must produce durable effects");
        };
        let [Effect::Conversation(ConversationEffect::AppendMessage(message))] =
            transition.effects.as_slice()
        else {
            panic!("results must be saved in one message");
        };
        message
            .content
            .iter()
            .map(|content| {
                let Content::ToolResult(response) = content else {
                    panic!("tool result expected");
                };
                let finished = response
                    .result
                    .as_ref()
                    .is_ok_and(|result| result.is_error != Some(true));
                (response.id.clone(), finished)
            })
            .collect()
    }

    #[tokio::test]
    async fn keeps_canonical_order_for_mixed_batches() {
        let probe = Arc::new(Probe::default());
        let results = execute(
            &probe,
            &[
                ("read-1", "read"),
                ("read-2", "read"),
                ("write", "write"),
                ("read-3", "read"),
                ("read-4", "read"),
            ],
            None,
        )
        .await;

        let ids = results
            .iter()
            .map(|(id, _)| id.as_str())
            .collect::<Vec<_>>();
        assert_eq!(ids, ["read-1", "read-2", "write", "read-3", "read-4"]);
        assert!(results.iter().all(|(_, finished)| *finished));
        let write = probe.position("start write");
        assert!(write > probe.position("end read-1"));
        assert!(write > probe.position("end read-2"));
        assert_eq!(probe.position("end write"), write + 1);
        assert!(probe.position("start read-3") > write + 1);
    }

    #[tokio::test]
    async fn malformed_call_preserves_read_batch() {
        let probe = Arc::new(Probe::default());
        let results = execute(
            &probe,
            &[
                ("read-1", "read"),
                ("invalid", "malformed"),
                ("read-2", "read"),
            ],
            None,
        )
        .await;
        assert_eq!(
            results,
            [
                ("read-1".to_owned(), true),
                ("invalid".to_owned(), false),
                ("read-2".to_owned(), true),
            ]
        );
        assert_eq!(probe.reads.load(Ordering::SeqCst), 2);
        assert!(!probe.wrote.load(Ordering::SeqCst));
        assert_eq!(probe.log.lock().unwrap().len(), 4);
    }

    #[tokio::test]
    async fn stop_never_runs_a_later_write() {
        let probe = Arc::new(Probe::default());
        let results = execute(
            &probe,
            &[
                ("slow", "slow"),
                ("read-1", "read"),
                ("read-2", "read"),
                ("write", "write"),
            ],
            Some(2),
        )
        .await;

        assert_eq!(
            results,
            [
                ("slow".to_owned(), false),
                ("read-1".to_owned(), true),
                ("read-2".to_owned(), true),
                ("write".to_owned(), false),
            ]
        );
        assert!(!probe.wrote.load(Ordering::SeqCst));
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
        classify(&result, Some(&Failure::tool(Some(ErrorCode::NotFound)))),
        (ToolResult::Error, Some(ErrorCode::NotFound))
    );
    assert_eq!(
        classify(&result, Some(&Failure::runtime(None))),
        (ToolResult::Error, None)
    );
    assert_eq!(
        classify(
            &result,
            Some(&Failure::runtime(Some(ErrorCode::PermissionDenied)))
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
