use super::*;
use plugin::{Context, desktop::Surface};
use serde_json::json;

#[tokio::test]
async fn callbacks_require_declared_permission_authority() {
    for remote in [false, true] {
        let (directory, node, controller, client, worktree) = support::empty(remote).await;
        let root = directory.path().join("source/package");
        let path = root.join("plugin.json");
        let mut manifest: serde_json::Value =
            serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        manifest["extensions"] = json!({"dev.sailry.platform":{
            "api_version":"v1","scope":"host","actions":["storage.read"],
            "host":{"entry":"dev.sailry.platform/main.js","resources":["dev.sailry.platform/main.js"],
                "handlers":["check","request","unrelated"],"read_only":["check","unrelated"]},
            "permissions":[{"kind":"accessibility","subject":"fixture.helper","check":"check","request":"request","actions":[]}]
        }});
        for subject in ["granted.helper", "unavailable.helper"] {
            let mut declaration =
                manifest["extensions"]["dev.sailry.platform"]["permissions"][0].clone();
            declaration["subject"] = json!(subject);
            manifest["extensions"]["dev.sailry.platform"]["permissions"]
                .as_array_mut()
                .unwrap()
                .push(declaration);
        }
        fs::write(path, manifest.to_string()).unwrap();
        fs::write(
            root.join("dev.sailry.platform/main.js"),
            r#"
import {getValue} from 'sailry/sdk';
const result={grants:[{kind:'accessibility',authorized:false}],can_request:true};
export async function check(input) {
    return {grants:[{kind:'accessibility',authorized:input.subject==='granted.helper'}],
        can_request:input.subject!=='unavailable.helper'};
}
export async function request(input) {
    if(input.subject!=='fixture.helper') throw new Error('unexpected request');
    try { await getValue('unrelated'); throw new Error('unexpected authority'); }
    catch(error) { if(error.code!=='permission_denied') throw error; }
    return result;
}
export async function unrelated() { return result; }
"#,
        )
        .unwrap();
        let package = info(execute(&client, install(worktree, 0)).await);
        assert!(package.issues.is_empty(), "{:?}", package.issues);
        let entries = sailry_client::permissions::directory(node.id(), [&package]);
        assert_eq!(entries.len(), 3);
        let status = client.permission_status(&entries[0], false).await.unwrap();
        assert_eq!(status.grants[0].authorized, Some(false));
        assert_eq!(
            client.permission_status(&entries[0], true).await.unwrap(),
            status
        );
        assert_eq!(
            client
                .permission_status(&entries[1], true)
                .await
                .unwrap()
                .grants[0]
                .authorized,
            Some(true)
        );
        assert!(
            !client
                .permission_status(&entries[2], true)
                .await
                .unwrap()
                .can_request
        );
        let mut context = Context {
            invocation: None,
            turn: None,
            surface: Surface::Permissions,
            package: package.summary.reference(),
            worktree: None,
            session: None,
        };
        for command in [
            Command::ReadPluginCallback {
                handler: "unrelated".into(),
                input: json!({}),
            },
            Command::ReadPluginValue {
                key: "unrelated".into(),
            },
        ] {
            assert_eq!(
                client
                    .execute(client.prepare(command).with_plugin(context.clone()))
                    .await
                    .unwrap_err()
                    .code,
                ErrorCode::PermissionDenied
            );
        }
        context.surface = Surface::Settings;
        assert_eq!(
            client
                .execute(
                    client
                        .prepare(Command::ReadPluginCallback {
                            handler: "check".into(),
                            input: json!({})
                        })
                        .with_plugin(context)
                )
                .await
                .unwrap_err()
                .code,
            ErrorCode::PermissionDenied
        );
        execute(
            &client,
            Command::SetPluginEnabled {
                name: package.summary.name.clone(),
                expected_revision: package.summary.revision,
                enabled: false,
            },
        )
        .await;
        assert_eq!(
            client
                .permission_status(&entries[0], false)
                .await
                .unwrap_err()
                .code,
            ErrorCode::PermissionDenied
        );
        controller.close().await.unwrap();
        node.shutdown().await.unwrap();
    }
}
