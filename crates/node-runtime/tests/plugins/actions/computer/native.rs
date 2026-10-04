//! Opt-in native Cua acceptance against explicitly owned AppKit fixtures.
use super::*;

fn state(path: &str) -> Value {
    serde_json::from_slice(&fs::read(path).unwrap()).unwrap()
}

async fn command(path: &str, action: &str) {
    let revision = state(path)["command_revision"].as_u64().unwrap();
    fs::write(format!("{path}.command"), action).unwrap();
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            if state(path)["command_revision"].as_u64().unwrap() > revision {
                return;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    })
    .await
    .expect("fixture command deadline");
}

async fn fixture(remote: bool) -> (tempfile::TempDir, Node, Link, Client, Context) {
    let directory = tempfile::tempdir().unwrap();
    let node = Node::start_with_computer(
        directory.path().join("node"),
        NetworkScope::default(),
        Some(sailry_node_runtime::ComputerWorker {
            executable: std::env::var_os("SAILRY_TEST_COMPUTER_WORKER")
                .expect("explicit signed computer worker required")
                .into(),
            bundle_id: "ai.sailry.host".into(),
        }),
    )
    .await
    .unwrap();
    let controller = Link::controller(directory.path().join("controller"), NetworkScope::default())
        .await
        .unwrap();
    let address = controller
        .handle()
        .pair(node.link().invite().unwrap().ticket())
        .await
        .unwrap();
    let client = Client::new(if remote {
        controller.handle().remote(address)
    } else {
        node.local()
    });
    let root = directory.path().join("source");
    package(&root.join("package"), "1.0.0");
    execute(
        &client,
        Command::RegisterProject {
            name: "Native computer fixture".into(),
            path: root.to_str().unwrap().into(),
        },
    )
    .await;
    let Output::Snapshot(snapshot) = execute(&client, Command::Snapshot).await else {
        panic!("snapshot expected")
    };
    let worktree = snapshot.worktrees[0].id;
    let mut context = install_actions(
        &client,
        &root.join("package"),
        worktree,
        0,
        &[Action::ReadComputer, Action::ControlComputer],
    )
    .await;
    context.session = Some(scopes::session(&client, worktree).await.id);
    (directory, node, controller, client, context)
}

async fn call(client: &Client, context: &Context, name: &str, arguments: Value) -> Value {
    let Output::Computer(result) = client
        .execute(request(client, context, name, arguments))
        .await
        .unwrap()
    else {
        panic!("native MCP result expected")
    };
    assert_ne!(result["isError"], true, "{name}: {result}");
    result
}

#[tokio::test]
#[ignore = "requires two owned background AppKit fixture processes, stationary hardware pointer and SAILRY_TEST_COMPUTER_WORKER"]
async fn coordinate_input_and_receipt_recovery() {
    let output = std::env::var("SAILRY_CUA_FIXTURE_OUTPUT").expect("owned target fixture required");
    let sibling_output =
        std::env::var("SAILRY_CUA_OTHER_OUTPUT").expect("owned sibling fixture required");
    let initial = state(&output);
    let sibling_initial = state(&sibling_output);
    let pid = initial["pid"].as_u64().unwrap();
    let window = initial["window_id"].as_u64().unwrap();
    let sibling_pid = sibling_initial["pid"].as_u64().unwrap();
    assert_ne!(pid, sibling_pid, "two separate app processes required");
    let pointer = || {
        let event = objc2_core_graphics::CGEvent::new(None).unwrap();
        objc2_core_graphics::CGEvent::location(Some(&event))
    };
    for remote in [false, true] {
        let (_directory, node, controller, client, context) = fixture(remote).await;
        let foreground = state(&output)["foreground_pid"].clone();
        assert_ne!(foreground, pid, "target fixture must already be background");
        assert_ne!(
            foreground, sibling_pid,
            "sibling fixture must already be background"
        );
        let observed = call(
            &client,
            &context,
            "get_window_state",
            json!({"pid":pid,"window_id":window,"include_screenshot":true}),
        )
        .await;
        let snapshot = &observed["structuredContent"];
        assert!(snapshot["snapshot_id"].is_string());
        assert!(
            observed["content"]
                .as_array()
                .unwrap()
                .iter()
                .any(|part| part["type"] == "image")
        );
        let scale = snapshot["screenshot_scale"]
            .as_f64()
            .expect("native screenshot scale");
        let height = snapshot["screenshot_height"].as_f64().unwrap();
        let before = state(&output);
        let expected = before["pointer_clicks"].as_u64().unwrap() + 1;
        let initial_pointer = pointer();
        // Pointer Count deliberately exposes no AXPress. Its supported input
        // route is the original Cua coordinate click, not a semantic retry.
        let input = request(
            &client,
            &context,
            "click",
            json!({"target":{"kind":"window","pid":pid,"window_id":window},
            "x":290.0*scale,"y":height-60.0*scale,"delivery_mode":"background"}),
        );
        let result = client.execute(input.clone()).await.unwrap();
        let Output::Computer(raw) = &result else {
            panic!("native MCP result expected")
        };
        assert_ne!(raw["isError"], true, "{raw}");
        tokio::time::timeout(Duration::from_secs(3), async {
            loop {
                if state(&output)["pointer_clicks"] == expected {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(50)).await;
            }
        })
        .await
        .unwrap_or_else(|_| panic!("native click counter did not change: {raw}"));
        assert_eq!(client.execute(input).await.unwrap(), result);
        let current = state(&output);
        assert_eq!(
            current["pointer_clicks"], expected,
            "receipt recovery must not click again"
        );
        assert_eq!(
            current["unsupported_presses"],
            before["unsupported_presses"]
        );
        assert_eq!(current["clicks"], before["clicks"]);
        assert_eq!(current["text"], before["text"]);
        assert_eq!(pointer(), initial_pointer);
        command(&sibling_output, "refresh").await;
        let sibling = state(&sibling_output);
        assert_eq!(sibling["foreground_pid"], foreground);
        assert_eq!(current["foreground_pid"], foreground);
        assert_eq!(sibling["pointer_clicks"], sibling_initial["pointer_clicks"]);
        assert_eq!(sibling["clicks"], sibling_initial["clicks"]);
        assert_eq!(sibling["text"], sibling_initial["text"]);
        drop(client);
        controller.close().await.unwrap();
        node.shutdown().await.unwrap();
    }
}

#[tokio::test]
#[ignore = "captures only an explicitly selected owned window; requires SAILRY_CUA_TARGET_PID, SAILRY_CUA_TARGET_WINDOW and SAILRY_TEST_COMPUTER_WORKER"]
async fn captures_selected_window_with_original_parameters() {
    let pid: u32 = std::env::var("SAILRY_CUA_TARGET_PID")
        .expect("explicit target PID required")
        .parse()
        .unwrap();
    let window: u32 = std::env::var("SAILRY_CUA_TARGET_WINDOW")
        .expect("explicit target window required")
        .parse()
        .unwrap();
    assert!(pid > 0 && window > 0);
    for remote in [false, true] {
        let (_directory, node, controller, client, context) = fixture(remote).await;
        let raw = call(
            &client,
            &context,
            "get_window_state",
            json!({"pid":pid,"window_id":window,
            "include_accessibility_tree":true,"include_screenshot":false}),
        )
        .await;
        assert!(raw["structuredContent"]["snapshot_id"].is_string());
        assert!(raw["structuredContent"]["elements"].is_array());
        assert!(raw.get("screen_observation").is_none());
        assert!(raw.get("execution_node").is_none());
        drop(client);
        controller.close().await.unwrap();
        node.shutdown().await.unwrap();
    }
}
