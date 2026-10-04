use sailry_client::Client;
use sailry_link::{CancellationToken, Link, NetworkScope};
use sailry_node_runtime::Node;
use sailry_protocol::{office::*, *};

#[tokio::test]
async fn reads_and_previews_locally_and_remotely() {
    let fixture = tempfile::tempdir().unwrap();
    let root = fixture.path().join("project");
    std::fs::create_dir(&root).unwrap();
    let node = Node::start(fixture.path().join("node")).await.unwrap();
    let local = Client::new(node.local());
    local
        .execute(local.prepare(Command::RegisterProject {
            name: "Office fixture".into(),
            path: root.to_str().unwrap().into(),
        }))
        .await
        .unwrap();
    let Output::Snapshot(snapshot) = local
        .execute(local.prepare(Command::Snapshot))
        .await
        .unwrap()
    else {
        panic!("snapshot expected")
    };
    let worktree = snapshot.worktrees[0].id;
    let controller = Link::controller(fixture.path().join("controller"), NetworkScope::default())
        .await
        .unwrap();
    let address = controller
        .handle()
        .pair(node.link().invite().unwrap().ticket())
        .await
        .unwrap();
    let remote = Client::new(controller.handle().remote(address));
    let cases: Vec<serde_json::Value> =
        serde_json::from_str(include_str!("../src/office/fixtures.json")).unwrap();
    local
        .execute(local.prepare(Command::SetPluginEnabled {
            name: "office".into(),
            expected_revision: 1,
            enabled: false,
        }))
        .await
        .unwrap();
    for (index, (client, other)) in [(&local, &remote), (&remote, &local)]
        .into_iter()
        .enumerate()
    {
        if index == 1 {
            client
                .execute(client.prepare(Command::RemovePlugin {
                    name: "office".into(),
                    expected_revision: 2,
                }))
                .await
                .unwrap();
        }
        for case in &cases {
            let name = case["path"].as_str().unwrap();
            let path = format!("{index}-{name}");
            let source = std::fs::read(
                std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                    .join("src/office/fixtures")
                    .join(name),
            )
            .unwrap();
            std::fs::write(root.join(&path), &source).unwrap();
            let Output::OfficeContent(content) = client
                .execute(client.prepare(Command::ReadOffice {
                    worktree,
                    options: Read {
                        path: path.clone(),
                        offset: 0,
                    },
                }))
                .await
                .unwrap()
            else {
                panic!("content expected")
            };
            assert_eq!(content.revision, blake3::hash(&source).to_hex().as_str());
            assert!(
                content
                    .sections
                    .iter()
                    .any(|s| s.text.contains(case["text"].as_str().unwrap()))
            );
            let admission = client
                .dispatch(client.prepare(Command::PreviewOffice {
                    worktree,
                    path: path.clone(),
                }))
                .await
                .unwrap();
            assert!(!admission.receipt.durable);
            let Output::OfficePreview(preview) = admission.completion.await.unwrap().unwrap()
            else {
                panic!("preview expected")
            };
            assert_eq!(preview.source_revision, content.revision);
            assert!(matches!(
                other.open(preview.download.stream).await,
                Err(Fault {
                    code: ErrorCode::NotFound,
                    ..
                })
            ));
            let mut bytes = Vec::new();
            client
                .download(
                    &preview.download,
                    &mut bytes,
                    CancellationToken::new(),
                    |_| {},
                )
                .await
                .unwrap();
            let pdf = lopdf::Document::load_mem(&bytes).unwrap();
            assert_eq!(pdf.get_pages().len(), 2, "{path}");
            let text = pdf.extract_text(&[1, 2]).unwrap();
            assert!(
                text.contains(case["text"].as_str().unwrap()),
                "{path}: {text}"
            );

            assert_eq!(std::fs::read(root.join(&path)).unwrap(), source);

            if !path.ends_with(".pdf") {
                let export = client.prepare(Command::ExportPdf {
                    worktree,
                    options: Export {
                        source: path.clone(),
                        path: format!("exports/{path}.pdf"),
                        expected_revision: None,
                    },
                });
                let result = client.execute(export.clone()).await.unwrap();
                assert_eq!(client.execute(export).await.unwrap(), result);
                assert!(
                    std::fs::read(root.join(format!("exports/{path}.pdf")))
                        .unwrap()
                        .starts_with(b"%PDF-")
                );
            }
        }
    }
    node.shutdown().await.unwrap();
    controller.close().await.unwrap();
}
