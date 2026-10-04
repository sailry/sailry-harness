//! Exercise the production Node binary preview route in isolated data.
use sailry_client::Client;
use sailry_link::CancellationToken;
use sailry_node_runtime::Node;
use sailry_protocol::{Command, Output};

pub(super) fn documents(output: &std::path::Path) -> Vec<(String, Vec<u8>, String)> {
    let runtime = tokio::runtime::Runtime::new().unwrap();
    runtime.block_on(async {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().join("project");
        std::fs::create_dir(&root).unwrap();
        let node = Node::start(directory.path().join("node")).await.unwrap();
        let client = Client::new(node.local());
        client
            .execute(client.prepare(Command::RegisterProject {
                name: "Preview fixture".into(),
                path: root.to_str().unwrap().into(),
            }))
            .await
            .unwrap();
        let Output::Snapshot(snapshot) = client
            .execute(client.prepare(Command::Snapshot))
            .await
            .unwrap()
        else {
            panic!("snapshot expected")
        };
        let worktree = snapshot.worktrees[0].id;
        let cases: Vec<serde_json::Value> = serde_json::from_str(include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../crates/node-runtime/src/office/fixtures.json"
        )))
        .unwrap();
        let mut documents = Vec::new();
        for case in cases {
            let path = case["path"].as_str().unwrap().to_owned();
            std::fs::copy(
                std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                    .join("../../crates/node-runtime/src/office/fixtures")
                    .join(&path),
                root.join(&path),
            )
            .unwrap();
            let Output::OfficePreview(preview) = client
                .execute(client.prepare(Command::PreviewOffice {
                    worktree,
                    path: path.clone(),
                }))
                .await
                .unwrap()
            else {
                panic!("preview expected")
            };
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
            std::fs::copy(root.join(&path), output.join(&path)).unwrap();
            std::fs::write(output.join(format!("preview-{path}.pdf")), &bytes).unwrap();
            documents.push((path, bytes, case["text"].as_str().unwrap().to_owned()));
        }
        node.shutdown().await.unwrap();
        documents
    })
}
