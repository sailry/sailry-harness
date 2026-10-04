//! Opt-in native SDK checks, separate from mock transport acceptance.
use super::*;
use crate::computer::driver::Runtime;

fn worker() -> Worker {
    Worker {
        executable: std::env::var_os("SAILRY_TEST_COMPUTER_WORKER")
            .expect("isolated computer worker binary required")
            .into(),
        bundle_id: "ai.sailry.host".into(),
    }
}

#[tokio::test]
#[ignore = "requires graphical macOS and an isolated SAILRY_TEST_COMPUTER_WORKER; no input or screenshots"]
async fn retains_authority_until_release() {
    let desktop = Desktop::default();
    desktop.configure_worker(Some(worker())).await;
    let session = SessionId::new();
    let first = CancellationToken::new();
    let state = desktop
        .execute(
            session,
            "get_agent_cursor_state",
            json!({}),
            &first,
            async { Ok(()) },
        )
        .await
        .unwrap();
    assert_ne!(state["isError"], true, "{state}");
    let initial = desktop
        .execute(session, "get_session", json!({}), &first, async { Ok(()) })
        .await
        .unwrap();
    assert_ne!(initial["isError"], true, "{initial}");
    let label = initial["structuredContent"]["session"].as_str().unwrap();
    first.cancel();
    let next = CancellationToken::new();
    let resumed = desktop
        .execute(session, "get_session", json!({}), &next, async { Ok(()) })
        .await
        .unwrap();
    assert_ne!(resumed["isError"], true, "{resumed}");
    assert_eq!(resumed["structuredContent"]["session"], label);
    desktop.finish(session).await;
    let ended = desktop
        .execute(session, "get_session", json!({}), &next, async { Ok(()) })
        .await
        .unwrap();
    assert_eq!(ended["isError"], true, "{ended}");
    assert_eq!(
        ended["structuredContent"]["code"], "session_not_started",
        "{ended}"
    );
    let fresh = desktop
        .execute(session, "get_agent_cursor_state", json!({}), &next, async {
            Ok(())
        })
        .await
        .unwrap();
    assert_ne!(fresh["isError"], true, "{fresh}");
    let current = desktop
        .execute(session, "get_session", json!({}), &next, async { Ok(()) })
        .await
        .unwrap();
    assert_ne!(current["isError"], true, "{current}");
    assert_ne!(current["structuredContent"]["session"], label);
    desktop.shutdown().await;
}

#[tokio::test]
#[ignore = "requires graphical macOS and an isolated SAILRY_TEST_COMPUTER_WORKER"]
async fn bound_calls_reject_session_substitution() {
    let mut runtime = Runtime::default();
    runtime.worker = Some(worker());
    let session = SessionId::new();
    let apps = runtime.call(session, "list_apps", json!({})).await.unwrap();
    assert_ne!(apps["isError"], true, "{apps}");
    for label in ["notes-create-hello", "", "implicit"] {
        let result = runtime
            .call(session, "list_apps", json!({"session":label}))
            .await
            .unwrap();
        assert_eq!(result["isError"], true, "{result}");
        assert_eq!(
            result["structuredContent"]["refusal"]["code"], "permission_denied",
            "{result}"
        );
        assert_eq!(
            result["structuredContent"]["refusal"]["message"],
            "public session substitution does not match the bound authorization context",
            "{result}"
        );
    }
    let result = runtime
        .call(session, "get_agent_cursor_state", json!({}))
        .await
        .unwrap();
    assert_ne!(result["isError"], true, "{result}");
    runtime.finish(session).await;
    let fresh = runtime.call(session, "list_apps", json!({})).await.unwrap();
    assert_ne!(fresh["isError"], true, "{fresh}");
    runtime.shutdown().await;
}

#[tokio::test]
#[ignore = "requires graphical macOS and an isolated SAILRY_TEST_COMPUTER_WORKER"]
async fn bound_sessions_keep_default_cursor() {
    let event = objc2_core_graphics::CGEvent::new(None).unwrap();
    let pointer = objc2_core_graphics::CGEvent::location(Some(&event));
    let mut runtime = Runtime::default();
    runtime.worker = Some(worker());
    let first = SessionId::new();
    let second = SessionId::new();
    let mut foreground = None;
    for (session, x) in [(first, 320), (second, 520)] {
        let apps = runtime.call(session, "list_apps", json!({})).await.unwrap();
        assert_ne!(apps["isError"], true, "{apps}");
        let active = apps["structuredContent"]["apps"]
            .as_array()
            .unwrap()
            .iter()
            .find(|app| app["active"] == true)
            .map(|app| app["pid"].clone());
        match &foreground {
            None => foreground = Some(active),
            Some(expected) => assert_eq!(&active, expected),
        }
        let result = runtime
            .call(
                session,
                "move_cursor",
                json!({"scope":"window","x":x,"y":280}),
            )
            .await
            .unwrap();
        assert_ne!(result["isError"], true, "{result}");
        let state = runtime
            .call(session, "get_agent_cursor_state", json!({}))
            .await
            .unwrap();
        assert_ne!(state["isError"], true, "{state}");
        assert_eq!(
            state["structuredContent"]["theme"]["id"], "cua.default",
            "{state}"
        );
        assert_eq!(
            state["structuredContent"]["position"],
            json!({"x":x as f64,"y":280.0}),
            "{state}"
        );
        assert_eq!(state["structuredContent"]["enabled"], true, "{state}");
        let apps = runtime.call(session, "list_apps", json!({})).await.unwrap();
        let active = apps["structuredContent"]["apps"]
            .as_array()
            .unwrap()
            .iter()
            .find(|app| app["active"] == true)
            .map(|app| app["pid"].clone());
        assert_eq!(Some(active), foreground);
    }
    runtime.finish(first).await;
    let peer = runtime
        .call(second, "get_agent_cursor_state", json!({}))
        .await
        .unwrap();
    assert_eq!(peer["structuredContent"]["position"]["x"], 520.0, "{peer}");
    let revived_session = runtime.call(first, "get_session", json!({})).await.unwrap();
    assert_eq!(revived_session["isError"], true, "{revived_session}");
    assert_eq!(
        revived_session["structuredContent"]["code"], "session_not_started",
        "{revived_session}"
    );
    let revived = runtime
        .call(first, "get_agent_cursor_state", json!({}))
        .await
        .unwrap();
    assert_ne!(revived["isError"], true, "{revived}");
    assert!(
        revived["structuredContent"]["position"].is_null(),
        "{revived}"
    );
    let active = runtime.call(first, "get_session", json!({})).await.unwrap();
    assert_ne!(active["isError"], true, "{active}");
    let event = objc2_core_graphics::CGEvent::new(None).unwrap();
    assert_eq!(
        objc2_core_graphics::CGEvent::location(Some(&event)),
        pointer
    );
    runtime.shutdown().await;
}
