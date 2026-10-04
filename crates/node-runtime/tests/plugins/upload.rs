use super::*;
use sailry_link::CancellationToken;
use std::io::{Cursor, Write};

fn archive(version: &str, wrapped: bool) -> Vec<u8> {
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let prefix = if wrapped { "example/" } else { "" };
    for (path, bytes) in [
        ("plugin.json", serde_json::json!({
            "$schema":"https://agent-plugins.org/schemas/1.0.0/plugin.schema.json",
            "name":"example", "version":version, "description":"Uploaded package"
        }).to_string()),
        ("skills/analysis/SKILL.md", "---\nname: analysis\ndescription: Inspect changes\n---\nAnalyze changes\n".into()),
        ("mcp.json", serde_json::json!({
            "$schema":"https://agent-plugins.org/schemas/1.0.0/mcp.schema.json",
            "mcpServers":{"isolated":{"type":"stdio", "command":"sailry_missing_fixture_executable"}}
        }).to_string()),
    ] {
        zip.start_file(format!("{prefix}{path}"), zip::write::SimpleFileOptions::default()).unwrap();
        zip.write_all(bytes.as_bytes()).unwrap();
    }
    zip.finish().unwrap().into_inner()
}

async fn stage(client: &Client, bytes: &[u8]) -> plugin::Upload {
    let Output::PluginUpload(upload) = execute(
        client,
        Command::UploadPlugin(plugin::UploadSpec {
            size: bytes.len() as u64,
            revision: blake3::hash(bytes).to_hex().to_string(),
        }),
    )
    .await
    else {
        panic!("plugin upload expected");
    };
    client
        .upload_plugin(&upload, &mut &bytes[..], CancellationToken::new(), |_| {})
        .await
        .unwrap();
    upload
}

#[tokio::test]
async fn installs_without_projects() {
    for remote in [false, true] {
        let directory = tempfile::tempdir().unwrap();
        let node = Node::start(directory.path().join("node")).await.unwrap();
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
        let packages = node.profile().join("plugins/packages");
        let published = || {
            fs::read_dir(&packages)
                .unwrap()
                .map(|entry| entry.unwrap().file_name())
                .collect::<std::collections::BTreeSet<_>>()
        };
        let before = published();
        let bytes = archive("1.0.0", remote);
        let upload = stage(&client, &bytes).await;
        let inspect = || Command::InspectPluginUpload {
            stream: upload.stream,
        };
        let preview = info(execute(&client, inspect()).await);
        assert_eq!(preview.summary.name, "example");
        assert_eq!(preview.origin, None);
        assert_eq!(preview.summary.revision, 0);
        assert_eq!(preview.skills.len(), 1);
        assert_eq!(preview.mcp.len(), 1);
        assert_eq!(preview, info(execute(&client, inspect()).await));
        assert!(inventory(&client).await.is_empty());
        assert_eq!(
            published(),
            before,
            "inspection must not publish package files"
        );
        assert!(!packages.join(&preview.summary.digest).exists());
        let Output::Snapshot(snapshot) = execute(&client, Command::Snapshot).await else {
            panic!("snapshot expected");
        };
        assert!(snapshot.projects.is_empty() && snapshot.worktrees.is_empty());
        let request = client.prepare(Command::InstallPluginUpload {
            stream: upload.stream,
            source: plugin::UploadSource::Directory,
            name: preview.summary.name.clone(),
            expected_revision: 0,
        });
        let pending = client.dispatch(request.clone()).await.unwrap();
        assert!(pending.receipt.durable);
        drop(pending);
        let installed = tokio::time::timeout(Duration::from_secs(10), async {
            loop {
                if let RequestOutcome::Completed(result) = client.outcome(&request).await.unwrap() {
                    break info(result.unwrap());
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .unwrap();
        assert_eq!(installed.summary.digest, preview.summary.digest);
        assert_eq!(installed.summary.revision, 1);
        assert_eq!(installed.origin, Some(plugin::Origin::Directory));
        let version = info(
            execute(
                &client,
                Command::ReadPluginVersion {
                    package: installed.summary.reference(),
                },
            )
            .await,
        );
        assert_eq!(version.origin, None);
        assert_eq!(
            info(client.execute(request.clone()).await.unwrap()),
            installed
        );

        let update = stage(&client, &archive("2.0.0", !remote)).await;
        execute(
            &client,
            Command::InspectPluginUpload {
                stream: update.stream,
            },
        )
        .await;
        let conflict = client
            .execute(client.prepare(Command::InstallPluginUpload {
                stream: update.stream,
                source: plugin::UploadSource::Archive,
                name: "example".into(),
                expected_revision: 0,
            }))
            .await
            .unwrap_err();
        assert_eq!(conflict.code, ErrorCode::RevisionConflict);
        let updated = info(
            execute(
                &client,
                Command::InstallPluginUpload {
                    stream: update.stream,
                    source: plugin::UploadSource::Archive,
                    name: "example".into(),
                    expected_revision: 1,
                },
            )
            .await,
        );
        assert_eq!(updated.summary.version.as_deref(), Some("2.0.0"));
        assert_eq!(updated.summary.revision, 2);
        assert_eq!(updated.origin, Some(plugin::Origin::Archive));
        assert_eq!(
            info(client.execute(request.clone()).await.unwrap()),
            installed
        );
        node.shutdown().await.unwrap();
        let recovered = Node::start(directory.path().join("node")).await.unwrap();
        let local = Client::new(recovered.local());
        assert_eq!(inventory(&local).await, vec![updated.summary.clone()]);
        assert_eq!(
            info(
                execute(
                    &local,
                    Command::ReadPlugin {
                        name: "example".into()
                    }
                )
                .await
            ),
            updated
        );
        let reconnected = Client::new(if remote {
            controller.handle().remote(recovered.link().address())
        } else {
            recovered.local()
        });
        assert_eq!(info(reconnected.execute(request).await.unwrap()), installed);
        recovered.shutdown().await.unwrap();
        controller.close().await.unwrap();
    }
}

#[tokio::test]
async fn confines_uploaded_resources() {
    let directory = tempfile::tempdir().unwrap();
    let node = Node::start(directory.path().join("node")).await.unwrap();
    let controller = Link::controller(directory.path().join("controller"), NetworkScope::default())
        .await
        .unwrap();
    let address = controller
        .handle()
        .pair(node.link().invite().unwrap().ticket())
        .await
        .unwrap();
    let remote = Client::new(controller.handle().remote(address));
    let local = Client::new(node.local());
    for (owner, other) in [(&local, &remote), (&remote, &local)] {
        let upload = stage(owner, &archive("1.0.0", false)).await;
        assert!(other.open(upload.stream).await.is_err());
        for command in [
            Command::InspectPluginUpload {
                stream: upload.stream,
            },
            Command::CancelFileTransfer {
                stream: upload.stream,
            },
        ] {
            assert_eq!(
                other
                    .execute(other.prepare(command))
                    .await
                    .unwrap_err()
                    .code,
                ErrorCode::NotFound
            );
        }
        execute(
            owner,
            Command::InspectPluginUpload {
                stream: upload.stream,
            },
        )
        .await;
        execute(
            owner,
            Command::CancelFileTransfer {
                stream: upload.stream,
            },
        )
        .await;
        assert_eq!(
            owner
                .execute(owner.prepare(Command::InstallPluginUpload {
                    stream: upload.stream,
                    source: plugin::UploadSource::Archive,
                    name: "example".into(),
                    expected_revision: 0
                }))
                .await
                .unwrap_err()
                .code,
            ErrorCode::NotFound
        );
        let invalid = stage(owner, b"not an archive").await;
        assert_eq!(
            owner
                .execute(owner.prepare(Command::InspectPluginUpload {
                    stream: invalid.stream
                }))
                .await
                .unwrap_err()
                .code,
            ErrorCode::InvalidRequest
        );
        assert!(inventory(owner).await.is_empty());
        assert!(
            owner
                .execute(owner.prepare(Command::UploadPlugin(plugin::UploadSpec {
                    size: plugin::MAX_PACKAGE_BYTES + 1,
                    revision: blake3::hash(b"").to_hex().to_string()
                })))
                .await
                .is_err()
        );
    }
    node.shutdown().await.unwrap();
    controller.close().await.unwrap();
}
