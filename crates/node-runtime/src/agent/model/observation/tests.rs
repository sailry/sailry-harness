use super::*;
use futures::{StreamExt, stream};
use sailry_protocol::conversation::observation::CategoryKind as Kind;
use serde_json::json;

fn tokens(payload: &Payload, kind: Kind) -> Option<u64> {
    payload
        .snapshot()
        .0
        .iter()
        .find(|row| row.kind == kind)
        .unwrap()
        .tokens
}

fn input(text: &str) -> String {
    json!({"model_config":{"request_headers":{"authorization":"PRIVATE_AUTH"}},
        "input":{"instructions":text,"input":[{"role":"user","content":"task"}]}})
    .to_string()
}

#[tokio::test]
async fn isolates_interleaved_request_handles() {
    let first = Sources::default().capture("", Instant::now());
    let second = Sources::default().capture("", Instant::now());
    let (mut first_handle, mut second_handle) = tokio::join!(
        first.scope(async {
            let handle = Logger.start().unwrap();
            tokio::task::yield_now().await;
            handle
        }),
        second.scope(async {
            tokio::task::yield_now().await;
            Logger.start().unwrap()
        }),
    );
    // Handles retain their own request even while a different request is current.
    second
        .scope(async { first_handle.write(&input("first")).unwrap() })
        .await;
    first
        .scope(async { second_handle.write(&input("second payload")).unwrap() })
        .await;
    assert_eq!(tokens(&first, Kind::System), Some(2));
    assert_eq!(tokens(&second, Kind::System), Some(4));
    let before = first.snapshot();
    first_handle
        .write(&input("later response text must be ignored"))
        .unwrap();
    first_handle.write("not JSON; PRIVATE_RESPONSE").unwrap();
    assert_eq!(first.snapshot(), before);
    let recorded = serde_json::to_string(&before.0).unwrap();
    for secret in ["PRIVATE_AUTH", "PRIVATE_RESPONSE", "first"] {
        assert!(!recorded.contains(secret));
    }
}

#[tokio::test]
async fn covers_creation_and_lazy_polling() {
    install();
    let sources = Sources::default();
    sources.replace(vec![Source {
        kind: Kind::Skills,
        text: "skill source".into(),
    }]);
    let payload = sources.capture("skill source", Instant::now());
    sources.replace(vec![Source {
        kind: Kind::Skills,
        text: "new source".into(),
    }]);
    let mut replies = payload
        .scope(async {
            request_log::start_log(json!({}), json!({"instructions":"skill source"})).unwrap();
            assert_eq!(tokens(&payload, Kind::Skills), Some(3));
            stream::once(async {
                request_log::start_log(
                    json!({}),
                    json!({"instructions":"skill source", "previous_response_id":"previous"}),
                )
                .unwrap();
            })
            .boxed()
        })
        .await;
    payload.scope(replies.next()).await;
    assert_eq!(tokens(&payload, Kind::Skills), Some(3));
    assert_eq!(tokens(&payload, Kind::Injected), Some(0));
    assert!(!payload.snapshot().1);
    assert!(
        sources
            .capture("new source", Instant::now())
            .snapshot()
            .0
            .iter()
            .all(|row| row.tokens.is_none())
    );
}

#[tokio::test]
async fn cancellation_leaves_no_current_request() {
    let payload = Sources::default().capture("", Instant::now());
    let captured = payload.clone();
    let (sender, receiver) = tokio::sync::oneshot::channel();
    let task = tokio::spawn(async move {
        captured
            .scope(async {
                sender.send(Logger.start().unwrap()).ok().unwrap();
                std::future::pending::<()>().await;
            })
            .await;
    });
    let mut held = receiver.await.unwrap();
    task.abort();
    assert!(task.await.unwrap_err().is_cancelled());
    let next = Sources::default().capture("", Instant::now());
    next.scope(async { held.write(&input("cancelled input")).unwrap() })
        .await;
    assert_eq!(tokens(&payload, Kind::System), Some(4));
    assert!(next.snapshot().0.iter().all(|row| row.tokens.is_none()));
    assert!(CURRENT.try_with(|_| ()).is_err());
    Logger.start().unwrap().write(&input("unscoped")).unwrap();
    assert!(next.snapshot().0.iter().all(|row| row.tokens.is_none()));
}

#[tokio::test]
async fn isolates_http_receipts_and_clears_retries() {
    let started = Instant::now();
    let first = Sources::default().capture("", started);
    let second = Sources::default().capture("", Instant::now());
    first.scope(async { headers(200) }).await;
    let receipt = first.snapshot().2.unwrap();
    assert_eq!(receipt.status, 200);
    assert!(u128::from(receipt.headers_us) <= started.elapsed().as_micros());
    second.scope(async { headers(429) }).await;
    assert_eq!(second.snapshot().2.unwrap().status, 429);
    assert_eq!(first.snapshot().2, Some(receipt));
    first.scope(async { prepared(Some("{}")) }).await;
    assert!(first.snapshot().2.is_none());
    headers(204);
    assert!(first.snapshot().2.is_none());
    assert_eq!(second.snapshot().2.unwrap().status, 429);
}

#[tokio::test]
async fn classifies_only_prepared_plugin_body() {
    let sources = Sources::default();
    sources.replace(Vec::new());
    let payload = sources.capture("adapted", Instant::now());
    payload
        .scope(async {
            prepared(Some(
                &json!({"messages":[{"role":"system","content":"adapted"},
            {"role":"user","content":"sent"},{"role":"tool","content":"result"}],
            "tools":[{"name":"lookup"}]})
                .to_string(),
            ));
        })
        .await;
    assert_eq!(tokens(&payload, Kind::System), Some(2));
    assert_eq!(tokens(&payload, Kind::User), Some(1));
    assert_eq!(tokens(&payload, Kind::Results), Some(2));
    assert_eq!(tokens(&payload, Kind::Skills), Some(0));
    assert!(payload.snapshot().1);
    payload
        .scope(async {
            prepared(Some(
                "{\"messages\":[{\"role\":\"system\",\"content\":\"rewritten\"}]}",
            ))
        })
        .await;
    assert_eq!(tokens(&payload, Kind::System), Some(3));
    assert_eq!(tokens(&payload, Kind::Skills), None);
    assert_eq!(tokens(&payload, Kind::Injected), None);
    for body in [
        None,
        Some("not JSON"),
        Some("{}"),
        Some("{\"private\":\"value\"}"),
        Some("{\"input\":{\"prompt\":\"opaque\"}}"),
        Some("{\"system\":\"adapted\",\"messages\":{\"prompt\":\"opaque\"}}"),
        Some("{\"contents\":\"opaque\"}"),
    ] {
        payload.scope(async { prepared(body) }).await;
        assert!(payload.snapshot().0.iter().all(|row| row.tokens.is_none()));
        assert!(!payload.snapshot().1);
    }
}

#[tokio::test]
async fn marks_both_continuation_layouts() {
    let sources = Sources::default();
    sources.replace(Vec::new());
    let payload = sources.capture("fixed summary prompt", Instant::now());
    for key in ["previous_response_id", "previous_interaction_id"] {
        let mut body = json!({"instructions":"fixed summary prompt","input":[]});
        body[key] = json!("previous");
        payload
            .scope(async { prepared(Some(&body.to_string())) })
            .await;
        assert!(!payload.snapshot().1);
        assert_eq!(tokens(&payload, Kind::Skills), Some(0));
        assert_eq!(tokens(&payload, Kind::Injected), Some(0));
        body[key] = serde_json::Value::Null;
        payload
            .scope(async { prepared(Some(&body.to_string())) })
            .await;
        assert!(payload.snapshot().1);
    }
}

#[test]
fn preserves_an_existing_logger() {
    // Use a fresh process: the GDK slot is process-wide and cannot be reset.
    const CHILD: &str = "SAILRY_LOGGER_OWNERSHIP_TEST";
    if std::env::var_os(CHILD).is_none() {
        let name = std::thread::current().name().unwrap().to_owned();
        let result = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", &name, "--nocapture"])
            .env(CHILD, "1")
            .output()
            .unwrap();
        assert!(
            result.status.success(),
            "{}",
            String::from_utf8_lossy(&result.stderr)
        );
        return;
    }
    struct Existing;
    struct Noop;
    impl RequestLogger for Existing {
        fn start(&self) -> Result<Box<dyn RequestLogHandle>, LogError> {
            Ok(Box::new(Noop))
        }
    }
    impl RequestLogHandle for Noop {
        fn write(&mut self, _: &str) -> Result<(), LogError> {
            Ok(())
        }
    }
    request_log::install_logger(Existing).unwrap();
    install();
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    runtime.block_on(async {
        let payload = Sources::default().capture("", Instant::now());
        payload
            .scope(async {
                request_log::start_log(json!({}), json!({"instructions":"owned elsewhere"}))
                    .unwrap();
            })
            .await;
        assert!(payload.snapshot().0.iter().all(|row| row.tokens.is_none()));
    });
}
