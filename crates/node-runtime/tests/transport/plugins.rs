//! Scoped plugin calls, storage reads, faults and cancellation behave alike through both
//! Client transports.
use crate::plugin_support::{execute, info, install, package};
use sailry_client::Client;
use sailry_link::{Link, NetworkScope};
use sailry_node_runtime::Node;
use sailry_protocol::{
    plugin::{Action, Context},
    *,
};
use serde_json::json;
use std::{fs, time::Duration};

const HANDLER: &str = r#"
import {getValue, setValue, completeRequest} from 'sailry/sdk';
export async function run(input) {
    await getValue(input.key);
    await completeRequest(setValue(input.key, true, '0'));
    if (input.wait) while (true) {}
    return input.key;
}
"#;

fn call(client: &Client, context: &Context, input: serde_json::Value) -> Request {
    client
        .prepare(Command::CallPlugin {
            handler: "run".into(),
            input,
        })
        .with_plugin(context.clone())
}

async fn present(client: &Client, context: &Context, key: &str) -> bool {
    let Output::PluginValue(value) = client
        .execute(
            client
                .prepare(Command::ReadPluginValue { key: key.into() })
                .with_plugin(context.clone()),
        )
        .await
        .unwrap()
    else {
        panic!("value expected")
    };
    value.present
}

#[tokio::test]
async fn matches_calls_and_cancellation() {
    let directory = tempfile::tempdir().unwrap();
    let node =
        Node::start_empty_with_plugin_catalog(directory.path().join("node"), "http://127.0.0.1:9/")
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
    let local = Client::new(node.local());
    let remote = Client::new(controller.handle().remote(address));
    let source = directory.path().join("source");
    let root = source.join("package");
    package(&root, "1.0.0");
    let worktree = crate::file_fixture::register(&local, &source).await;
    let path = root.join("plugin.json");
    let mut manifest: serde_json::Value =
        serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    let actions = [
        Action::ReadStorage,
        Action::WriteStorage,
        Action::Notify,
        Action::Dispatch,
    ];
    manifest["extensions"] = json!({"dev.sailry.platform": {
        "api_version": "v1", "actions": actions,
        "host": {
            "entry": "dev.sailry.platform/main.js",
            "resources": ["dev.sailry.platform/main.js"],
            "handlers": ["run"],
            "read_only": [],
        },
    }});
    fs::write(&path, manifest.to_string()).unwrap();
    fs::write(root.join("dev.sailry.platform/main.js"), HANDLER).unwrap();
    let installed = info(execute(&local, install(worktree, 0)).await);
    assert!(installed.issues.is_empty(), "{:?}", installed.issues);
    let context = Context {
        invocation: None,
        turn: None,
        surface: Default::default(),
        package: installed.summary.reference(),
        worktree: Some(worktree),
        session: None,
    };
    let clients = [&local, &remote];
    for (index, client) in clients.into_iter().enumerate() {
        let other = clients[1 - index];
        let key = format!("value-{index}");
        let request = call(client, &context, json!({"key": key}));
        let admission = client.dispatch(request.clone()).await.unwrap();
        assert!(admission.receipt.durable);
        let result = admission.completion.await.unwrap();
        assert_eq!(result, Ok(Output::PluginResult(json!(key))));
        assert_eq!(client.execute(request).await, result);
        // Scoped storage written through one transport is visible through the other.
        assert!(present(other, &context, &key).await);

        let key = format!("held-{index}");
        let held = call(client, &context, json!({"key": key, "wait": true}));
        let pending = client.dispatch(held.clone()).await.unwrap();
        assert!(pending.receipt.durable);
        tokio::time::timeout(Duration::from_secs(10), async {
            while !present(client, &context, &key).await {
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("plugin call start deadline");
        assert_eq!(
            client
                .execute(
                    client
                        .prepare(Command::CancelPluginCall { request: held.id })
                        .with_plugin(context.clone())
                )
                .await
                .unwrap(),
            Output::PluginCallStopping { request: held.id }
        );
        assert_eq!(
            tokio::time::timeout(Duration::from_secs(10), pending.completion)
                .await
                .unwrap()
                .unwrap()
                .unwrap_err()
                .code,
            ErrorCode::Cancelled
        );
        assert_eq!(
            client.execute(held).await.unwrap_err().code,
            ErrorCode::Cancelled
        );
    }
    // Rejections carry identical faults through both transports.
    let mut forged = context.clone();
    forged.package.digest = "0".repeat(64);
    let mut faults = Vec::new();
    for client in clients {
        let missing = client
            .prepare(Command::CallPlugin {
                handler: "missing".into(),
                input: json!({}),
            })
            .with_plugin(context.clone());
        faults.push(client.execute(missing).await.unwrap_err());
        faults.push(
            client
                .execute(call(client, &forged, json!({"key": "forged"})))
                .await
                .unwrap_err(),
        );
    }
    assert_eq!(faults[..2], faults[2..]);
    assert!(!present(&local, &context, "forged").await);
    tokio::time::timeout(Duration::from_secs(10), node.shutdown())
        .await
        .unwrap()
        .unwrap();
    controller.close().await.unwrap();
}
