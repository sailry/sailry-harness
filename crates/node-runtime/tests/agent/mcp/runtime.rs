use super::*;
use plugin::mcp::RuntimeState;

pub(super) async fn state(
    fixture: &process::Fixture,
    expected: Option<RuntimeState>,
) -> plugin::Info {
    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            let Output::Plugin(info) = execute(
                &fixture.client,
                Command::ReadPlugin {
                    name: "example".into(),
                },
            )
            .await
            else {
                panic!("plugin expected");
            };
            if info
                .mcp
                .first()
                .and_then(|server| server.runtime.as_ref())
                .map(|runtime| runtime.state)
                == expected
            {
                return info;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("MCP runtime publication deadline")
}

#[tokio::test]
async fn publishes_warm_reconnect_and_disable() {
    let model = Server::turn_tools(vec![]).await;
    let fixture = process::Fixture::packages(&model, &[]).await;
    package(
        &fixture.root,
        json!({"native":peer::config("mcp::peer::stdio_peer","normal")}),
        "runtime",
    );
    let installed = install(&fixture, 0).await;
    let current = state(&fixture, Some(RuntimeState::Connected)).await;
    assert_eq!(current.summary, installed.summary);
    assert_eq!(current.mcp[0].runtime.as_ref().unwrap().tools, 4);
    let data = fixture.node.profile().join("plugins/data/example");
    assert_eq!(connections::requests(&data, "initialize"), 1);
    let mut events = fixture.client.subscribe().await.unwrap();
    let _ = events.next().await.unwrap();
    let Output::PluginMcpChecked(checked) = execute(
        &fixture.client,
        Command::CheckPluginMcp {
            package: current.summary.reference(),
            server: "native".into(),
        },
    )
    .await
    else {
        panic!("MCP check expected");
    };
    assert_eq!(checked.tools.len(), 4);
    let refreshed = state(&fixture, Some(RuntimeState::Connected)).await;
    assert_eq!(refreshed.summary, installed.summary);
    assert_eq!(connections::requests(&data, "initialize"), 2);
    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            if let Update::Event(envelope) = events.next().await.unwrap()
                && matches!(&envelope.event, Event::PluginChanged(summary) if *summary == installed.summary)
            {
                break;
            }
        }
    }).await.expect("runtime metadata event deadline");
    execute(
        &fixture.client,
        Command::SetPluginMcpAllowed {
            package: refreshed.summary.reference(),
            server: "native".into(),
            allowed: false,
        },
    )
    .await;
    let disabled = state(&fixture, None).await;
    assert!(!disabled.mcp[0].allowed);
    assert!(model.requests.lock().unwrap().is_empty());
    fixture.node.shutdown().await.unwrap();
    fixture.controller.close().await.unwrap();
}

#[tokio::test]
async fn publishes_public_connection_failure() {
    let model = Server::turn_tools(vec![]).await;
    let fixture = process::Fixture::packages(&model, &[]).await;
    let mut config = peer::config("mcp::peer::stdio_peer", "bad_handshake");
    config["env"]["PRIVATE_TOKEN"] = json!("private-runtime-env-value");
    package(&fixture.root, json!({"native":config}), "failure");
    let manifest_path = fixture.root.join("package/plugin.json");
    let mut manifest: Value = serde_json::from_slice(&fs::read(&manifest_path).unwrap()).unwrap();
    manifest["extensions"] = json!({"dev.sailry.platform":{
        "api_version":"v1",
        "instructions":"Use the example service for fixture records"
    }});
    fs::write(manifest_path, manifest.to_string()).unwrap();
    install(&fixture, 0).await;
    let info = state(&fixture, Some(RuntimeState::Failed)).await;
    let runtime = info.mcp[0].runtime.as_ref().unwrap();
    assert_eq!(runtime.tools, 0);
    assert_eq!(
        runtime.fault.as_ref().unwrap().message,
        "MCP server is unavailable"
    );
    let public = serde_json::to_string(runtime).unwrap();
    assert!(!public.contains("private-runtime-env-value"));
    assert!(!public.contains("fixture rejects initialization"));
    let (_, turn) = submit(&fixture, false).await;
    let page = finished(&fixture.client, fixture.session.id, turn.id).await;
    assert_eq!(page.runs.last().unwrap().status, Status::Completed);
    let messages = model.requests.lock().unwrap()[0]["messages"].clone();
    let system = messages
        .as_array()
        .unwrap()
        .iter()
        .filter(|message| matches!(message["role"].as_str(), Some("system" | "developer")))
        .map(Value::to_string)
        .collect::<Vec<_>>()
        .join("\n");
    assert!(
        system.contains("Use the example service for fixture records"),
        "{system}"
    );
    fixture.node.shutdown().await.unwrap();
    fixture.controller.close().await.unwrap();
}
