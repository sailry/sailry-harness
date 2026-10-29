use super::*;
use sailry_link::CancellationToken;
use serde_json::json;
use std::io::Write;

fn requires_newer(error: Fault) {
    assert_eq!(error.code, ErrorCode::NotConfigured);
    assert_eq!(
        error.details,
        Some(FaultDetails::PluginRequiresNewer {
            required: "99.0.0".into()
        })
    );
}

fn manifest(required: &str) -> serde_json::Value {
    json!({
        "$schema":"https://agent-plugins.org/schemas/1.0.0/plugin.schema.json",
        "name":"example", "version":"1.0.0",
        "extensions":{"dev.sailry.platform":{"api_version":"v1", "min_sailry_version":required}}
    })
}

#[tokio::test]
async fn rejects_worktree_and_uploaded_packages() {
    for remote in [false, true] {
        let (directory, node, controller, client, worktree) = support::empty(remote).await;
        let bytes = serde_json::to_vec(&manifest("99.0.0")).unwrap();
        let current = semver::Version::parse(sailry_link::PRODUCT_VERSION).unwrap();
        let next = format!("{}.{}.{}", current.major, current.minor, current.patch + 1);
        for minimum in [format!("{next}-alpha.1"), next, "99.0.0".into()] {
            fs::write(
                directory.path().join("source/package/plugin.json"),
                manifest(&minimum).to_string(),
            )
            .unwrap();
            let error = client
                .execute(client.prepare(install(worktree, 0)))
                .await
                .unwrap_err();
            assert_eq!(error.code, ErrorCode::NotConfigured);
            assert_eq!(
                error.details,
                Some(FaultDetails::PluginRequiresNewer { required: minimum })
            );
        }
        for source in [
            plugin::UploadSource::Directory,
            plugin::UploadSource::Archive,
        ] {
            let mut zip = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
            zip.start_file("plugin.json", zip::write::SimpleFileOptions::default())
                .unwrap();
            zip.write_all(&bytes).unwrap();
            let archive = zip.finish().unwrap().into_inner();
            let Output::PluginUpload(upload) = execute(
                &client,
                Command::UploadPlugin(plugin::UploadSpec {
                    size: archive.len() as u64,
                    revision: blake3::hash(&archive).to_hex().to_string(),
                }),
            )
            .await
            else {
                panic!("upload expected");
            };
            client
                .upload(&upload, &mut &archive[..], CancellationToken::new(), |_| {})
                .await
                .unwrap();
            let inspected = info(
                execute(
                    &client,
                    Command::InspectPluginUpload {
                        stream: upload.stream,
                    },
                )
                .await,
            );
            assert_eq!(inspected.summary.compatibility.required(), Some("99.0.0"));
            requires_newer(
                client
                    .execute(client.prepare(Command::InstallPluginUpload {
                        stream: upload.stream,
                        source,
                        name: "example".into(),
                        expected_revision: 0,
                    }))
                    .await
                    .unwrap_err(),
            );
        }
        let Output::Plugins(installed) = execute(&client, Command::ListPlugins).await else {
            panic!("inventory expected");
        };
        assert!(installed.is_empty());
        controller.close().await.unwrap();
        node.shutdown().await.unwrap();
    }
}

#[tokio::test]
async fn retains_unavailable_installations_and_captures() {
    for remote in [false, true] {
        let (directory, node, controller, client, worktree) = support::empty(remote).await;
        fs::write(
            directory.path().join("source/package/plugin.json"),
            manifest("0.1.0-alpha.4").to_string(),
        )
        .unwrap();
        let mut installed = info(execute(&client, install(worktree, 0)).await);
        let reference = installed.summary.reference();
        controller.close().await.unwrap();
        node.shutdown().await.unwrap();
        installed.extension.as_mut().unwrap().min_sailry_version = Some("99.0.0".into());
        let captured = serde_json::to_vec(&installed).unwrap();
        let profile = directory.path().join("node");
        let path = profile.join("storage/node.turso");
        let (handle, db) = database_fixture::open(&path, false).await;
        for table in ["plugins", "plugin_packages"] {
            db.execute(
                &format!("UPDATE {table} SET body=?1 WHERE name='example'"),
                [captured.clone()],
            )
            .await
            .unwrap();
        }
        database_fixture::close(handle, db).await;
        let node = Node::start_empty_with_plugin_catalog(&profile, "http://127.0.0.1:9/")
            .await
            .unwrap();
        let controller =
            Link::controller(directory.path().join("controller"), NetworkScope::default())
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
        let current = info(
            execute(
                &client,
                Command::ReadPlugin {
                    name: "example".into(),
                },
            )
            .await,
        );
        assert!(current.summary.enabled);
        assert!(!current.summary.available());
        assert_eq!(current.summary.reference(), reference);
        assert_eq!(current.summary.compatibility.required(), Some("99.0.0"));
        for command in [
            Command::SetPluginEnabled {
                name: "example".into(),
                expected_revision: current.summary.revision,
                enabled: true,
            },
            Command::ReadPluginView {
                package: reference.clone(),
                surface: plugin::desktop::Surface::Workspace,
            },
            Command::ReadPluginVersion {
                package: reference.clone(),
            },
        ] {
            requires_newer(client.execute(client.prepare(command)).await.unwrap_err());
        }
        let Output::Snapshot(snapshot) = execute(&client, Command::Snapshot).await else {
            panic!("snapshot expected");
        };
        assert_eq!(snapshot.plugins, [current.summary]);
        controller.close().await.unwrap();
        node.shutdown().await.unwrap();
        let (handle, db) = database_fixture::open(&path, true).await;
        let mut rows = db
            .query("SELECT body FROM plugin_packages WHERE name='example'", ())
            .await
            .unwrap();
        let stored: Vec<u8> = rows.next().await.unwrap().unwrap().get(0).unwrap();
        assert_eq!(stored, captured);
        drop(rows);
        database_fixture::close(handle, db).await;
    }
}
