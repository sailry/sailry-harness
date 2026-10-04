use super::*;
use sailry_protocol::plugin::{
    Context,
    desktop::{Bundle, MAX_BYTES},
};

const PNG: &[u8] = &[
    137, 80, 78, 71, 13, 10, 26, 10, 0, 0, 0, 13, 73, 72, 68, 82, 0, 0, 0, 2, 0, 0, 0, 2, 8, 6, 0,
    0, 0, 114, 182, 13, 36, 0, 0, 0, 16, 73, 68, 65, 84, 120, 156, 99, 96, 64, 6, 207, 42, 52, 22,
    0, 0, 6, 2, 2, 39, 229, 252, 56, 2, 0, 0, 0, 0, 73, 69, 78, 68, 174, 66, 96, 130,
];

fn declare(root: &Path, main: &[u8]) {
    let mut value: serde_json::Value =
        serde_json::from_slice(&fs::read(root.join("plugin.json")).unwrap()).unwrap();
    value["extensions"] = serde_json::json!({"dev.sailry.platform": {
        "api_version":"v1", "actions":["files.read"],
        "desktop":{"entry":"dev.sailry.platform/ui/main.js", "resources":["dev.sailry.platform/ui/main.js", "dev.sailry.platform/ui/locales/zh.json", "dev.sailry.platform/ui/token.png"]}
    }});
    fs::write(root.join("plugin.json"), value.to_string()).unwrap();
    fs::create_dir_all(root.join("dev.sailry.platform/ui/locales")).unwrap();
    fs::write(root.join("dev.sailry.platform/ui/main.js"), main).unwrap();
    fs::write(root.join("dev.sailry.platform/ui/token.png"), PNG).unwrap();
    fs::write(
        root.join("dev.sailry.platform/ui/locales/zh.json"),
        r#"{"title":"扩展 🙂"}"#,
    )
    .unwrap();
}

fn read(package: &Info) -> Command {
    Command::ReadPluginView {
        surface: sailry_protocol::plugin::desktop::Surface::Workspace,
        package: package.summary.reference(),
    }
}

fn bundle(output: Output) -> Bundle {
    let Output::PluginView(bundle) = output else {
        panic!("plugin view expected")
    };
    assert!(bundle.valid());
    bundle
}

#[tokio::test]
async fn reads_declared_resources() {
    for remote in [false, true] {
        let (directory, node, controller, client, worktree) = fixture(remote).await;
        let root = directory.path().join("source/package");
        declare(&root, b"export default class Example {}\n");
        let first = info(execute(&client, install(worktree, 0)).await);
        // The source directory is not the installed version.
        fs::write(
            root.join("dev.sailry.platform/ui/main.js"),
            "Changed source",
        )
        .unwrap();
        let request = client.prepare(read(&first));
        let pending = client.dispatch(request.clone()).await.unwrap();
        assert_eq!(pending.receipt.id, request.id);
        assert!(!pending.receipt.durable);
        let content = bundle(pending.completion.await.unwrap().unwrap());
        assert_eq!(content.package, first.summary.reference());
        assert_eq!(content.entry, "dev.sailry.platform/ui/main.js");
        assert_eq!(content.files.len(), 2);
        assert_eq!(content.images.len(), 1);
        use base64::{Engine as _, engine::general_purpose::STANDARD};
        assert_eq!(
            STANDARD
                .decode(&content.images["dev.sailry.platform/ui/token.png"])
                .unwrap(),
            PNG
        );
        assert_eq!(
            content.files["dev.sailry.platform/ui/main.js"],
            "export default class Example {}\n"
        );
        assert_eq!(
            content.files["dev.sailry.platform/ui/locales/zh.json"],
            r#"{"title":"扩展 🙂"}"#
        );
        assert!(!content.files.contains_key("plugin.json"));
        assert!(!content.files.contains_key("skills/analysis/SKILL.md"));
        let digest = blake3::hash(&serde_json::to_vec(&request).unwrap())
            .to_hex()
            .to_string();
        assert_eq!(
            execute(
                &client,
                Command::InspectRequest {
                    id: request.id,
                    digest
                }
            )
            .await,
            Output::RequestOutcome {
                id: request.id,
                outcome: RequestOutcome::NotAdmitted
            }
        );
        let origin = Context {
            invocation: None,
            turn: None,
            surface: Default::default(),
            package: first.summary.reference(),
            worktree: Some(worktree),
            session: None,
        };
        assert_eq!(
            client
                .execute(request.clone().with_plugin(origin))
                .await
                .unwrap_err()
                .code,
            ErrorCode::PermissionDenied
        );
        let mut wrong = request.clone();
        wrong.target = NodeId([123; 32]);
        assert_eq!(
            client.execute(wrong).await.unwrap_err().code,
            ErrorCode::WrongTarget
        );
        let mut wrong = request;
        wrong.version = 99;
        assert!(client.execute(wrong).await.is_err());
        let second = info(execute(&client, install(worktree, first.summary.revision)).await);
        assert_eq!(
            client
                .execute(client.prepare(read(&first)))
                .await
                .unwrap_err()
                .code,
            ErrorCode::RevisionConflict
        );
        assert_eq!(
            bundle(execute(&client, read(&second)).await).files["dev.sailry.platform/ui/main.js"],
            "Changed source"
        );
        let disabled = info(
            execute(
                &client,
                Command::SetPluginEnabled {
                    name: "example".into(),
                    expected_revision: second.summary.revision,
                    enabled: false,
                },
            )
            .await,
        );
        assert_eq!(
            client
                .execute(client.prepare(read(&second)))
                .await
                .unwrap_err()
                .code,
            ErrorCode::NotConfigured
        );
        execute(
            &client,
            Command::RemovePlugin {
                name: "example".into(),
                expected_revision: disabled.summary.revision,
            },
        )
        .await;
        assert_eq!(
            client
                .execute(client.prepare(read(&second)))
                .await
                .unwrap_err()
                .code,
            ErrorCode::NotFound
        );
        node.shutdown().await.unwrap();
        controller.close().await.unwrap();
    }
}

#[tokio::test]
async fn rejects_invalid_resources() {
    for remote in [false, true] {
        let (directory, node, controller, client, worktree) = fixture(remote).await;
        let root = directory.path().join("source/package");
        let mut revision = 0;
        for (bytes, missing, code) in [
            (vec![0xff], false, ErrorCode::InvalidRequest),
            (vec![b'x'; MAX_BYTES], false, ErrorCode::InvalidRequest),
            (b"entry".to_vec(), true, ErrorCode::NotFound),
            (b"entry".to_vec(), false, ErrorCode::Unavailable),
        ] {
            declare(&root, &bytes);
            if missing {
                fs::remove_file(root.join("dev.sailry.platform/ui/main.js")).unwrap();
            }
            let package = info(execute(&client, install(worktree, revision)).await);
            revision = package.summary.revision;
            assert_eq!(package.skills.len(), 1);
            assert!(package.extension.as_ref().unwrap().desktop.is_some());
            if code == ErrorCode::Unavailable {
                fs::write(
                    directory
                        .path()
                        .join("node/plugins/packages")
                        .join(&package.summary.digest)
                        .join("dev.sailry.platform/ui/main.js"),
                    "Tampered",
                )
                .unwrap();
            }
            assert_eq!(
                client
                    .execute(client.prepare(read(&package)))
                    .await
                    .unwrap_err()
                    .code,
                code
            );
        }
        node.shutdown().await.unwrap();
        controller.close().await.unwrap();
    }
}

#[tokio::test]
async fn recovers_host_versions() {
    let first = fixture(false).await;
    let second = fixture(true).await;
    declare(&first.0.path().join("source/package"), b"Host one");
    declare(&second.0.path().join("source/package"), b"Host two");
    let one = info(execute(&first.3, install(first.4, 0)).await);
    let two = info(execute(&second.3, install(second.4, 0)).await);
    assert_ne!(one.summary.digest, two.summary.digest);
    assert_eq!(
        first
            .3
            .execute(first.3.prepare(read(&two)))
            .await
            .unwrap_err()
            .code,
        ErrorCode::RevisionConflict
    );
    assert_eq!(
        second
            .3
            .execute(second.3.prepare(read(&one)))
            .await
            .unwrap_err()
            .code,
        ErrorCode::RevisionConflict
    );
    assert_eq!(
        bundle(execute(&second.3, read(&two)).await).files["dev.sailry.platform/ui/main.js"],
        "Host two"
    );
    let expected = bundle(execute(&first.3, read(&one)).await);
    first.1.shutdown().await.unwrap();
    let reopened = Node::start(first.0.path().join("node")).await.unwrap();
    let client = Client::new(reopened.local());
    assert_eq!(bundle(execute(&client, read(&one)).await), expected);
    reopened.shutdown().await.unwrap();
    second.1.shutdown().await.unwrap();
    first.2.close().await.unwrap();
    second.2.close().await.unwrap();
}

#[tokio::test]
async fn reads_disabled_settings() {
    for remote in [false, true] {
        let (directory, node, controller, client, worktree) = fixture(remote).await;
        let root = directory.path().join("source/package");
        declare(&root, b"export default class Workspace {}\n");
        let mut manifest: serde_json::Value =
            serde_json::from_slice(&fs::read(root.join("plugin.json")).unwrap()).unwrap();
        let extension = &mut manifest["extensions"]["dev.sailry.platform"];
        extension["desktop"]["resources"]
            .as_array_mut()
            .unwrap()
            .push("dev.sailry.platform/ui/settings.js".into());
        extension["settings_page"] = serde_json::json!({"navigation":{"label":"Example settings"},"entry":"dev.sailry.platform/ui/settings.js"});
        fs::write(root.join("plugin.json"), manifest.to_string()).unwrap();
        fs::write(
            root.join("dev.sailry.platform/ui/settings.js"),
            "export default class Settings {}",
        )
        .unwrap();
        let installed = info(execute(&client, install(worktree, 0)).await);
        let disabled = info(
            execute(
                &client,
                Command::SetPluginEnabled {
                    name: "example".into(),
                    expected_revision: installed.summary.revision,
                    enabled: false,
                },
            )
            .await,
        );
        assert_eq!(
            client
                .execute(client.prepare(read(&disabled)))
                .await
                .unwrap_err()
                .code,
            ErrorCode::NotConfigured
        );
        let command = Command::ReadPluginView {
            surface: plugin::desktop::Surface::Settings,
            package: disabled.summary.reference(),
        };
        let view = bundle(execute(&client, command).await);
        assert_eq!(view.entry, "dev.sailry.platform/ui/settings.js");
        assert_eq!(view.files[&view.entry], "export default class Settings {}");
        node.shutdown().await.unwrap();
        controller.close().await.unwrap();
    }
}

#[tokio::test]
async fn settings_only_package_keeps_its_surface() {
    for remote in [false, true] {
        let (_directory, node, controller, client, _) = fixture(remote).await;
        let installed = info(
            execute(
                &client,
                Command::ReadPlugin {
                    name: "computer".into(),
                },
            )
            .await,
        );
        assert!(!installed.summary.enabled);
        assert!(installed.issues.is_empty(), "{:?}", installed.issues);
        let desktop = installed
            .extension
            .as_ref()
            .unwrap()
            .desktop
            .as_ref()
            .unwrap();
        assert!(desktop.entry.is_none());
        assert!(desktop.navigation.is_none() && desktop.panel.is_none());
        assert!(desktop.renderers.is_empty());
        let view = bundle(
            execute(
                &client,
                Command::ReadPluginView {
                    surface: plugin::desktop::Surface::Settings,
                    package: installed.summary.reference(),
                },
            )
            .await,
        );
        assert_eq!(view.entry, "dev.sailry.platform/desktop/settings.js");
        assert_eq!(
            view.files[&view.entry],
            include_str!("../../../../plugins/computer/dev.sailry.platform/desktop/settings.js")
        );
        let enabled = info(
            execute(
                &client,
                Command::SetPluginEnabled {
                    name: "computer".into(),
                    expected_revision: installed.summary.revision,
                    enabled: true,
                },
            )
            .await,
        );
        assert_eq!(
            client
                .execute(client.prepare(read(&enabled)))
                .await
                .unwrap_err()
                .code,
            ErrorCode::InvalidRequest
        );
        for surface in [
            plugin::desktop::Surface::Composer,
            plugin::desktop::Surface::Project,
        ] {
            assert_eq!(
                client
                    .execute(client.prepare(Command::ReadPluginView {
                        surface,
                        package: enabled.summary.reference(),
                    }))
                    .await
                    .unwrap_err()
                    .code,
                ErrorCode::NotConfigured
            );
        }
        node.shutdown().await.unwrap();
        controller.close().await.unwrap();
    }
}
