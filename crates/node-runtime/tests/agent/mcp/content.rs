use super::*;
use sailry_protocol::tool::{Content, Presentation};

#[tokio::test]
async fn restores() {
    for remote in [false, true] {
        let model = Server::tools(vec![(
            peer::package_alias("tool-content", "reports", "report"),
            json!({}),
        )])
        .await;
        let fixture = process::Fixture::packages(remote, &model, &[]).await;
        let package = fixture.root.join("package");
        fs::create_dir(&package).unwrap();
        for (name, text) in [
            (
                "plugin.json",
                include_str!("../../../../../plugins/examples/tool-content/plugin.json"),
            ),
            (
                "content.json",
                include_str!("../../../../../plugins/examples/tool-content/content.json"),
            ),
        ] {
            fs::write(package.join(name), text).unwrap();
        }
        fs::write(
            package.join("mcp.json"),
            json!({"$schema":"https://agent-plugins.org/schemas/1.0.0/mcp.schema.json",
                "mcpServers":{"reports":peer::config("mcp::modern::stdio_peer", "content")}})
            .to_string(),
        )
        .unwrap();
        let Output::Plugin(info) = execute(
            &fixture.client,
            Command::InstallPlugin {
                worktree: fixture.session.worktree,
                path: "package".into(),
                name: "tool-content".into(),
                expected_revision: 0,
                expected_digest: None,
            },
        )
        .await
        else {
            panic!("plugin expected")
        };
        assert!(info.issues.is_empty(), "{:?}", info.issues);
        assert_eq!(
            info.extension.as_ref().unwrap().tools[0].presentation,
            Presentation::Content
        );
        let (_, turn) = submit(&fixture, true).await;
        execute(
            &fixture.client,
            Command::RemovePlugin {
                name: "tool-content".into(),
                expected_revision: info.summary.revision,
            },
        )
        .await;
        execute(&fixture.client, Command::StartQueuedTurn { turn: turn.id }).await;
        let page = finished(&fixture.client, fixture.session.id, turn.id).await;
        assert_eq!(page.runs[0].status, Status::Completed, "{:?}", page.runs);
        assert!(page.approvals.is_empty());
        let alias = peer::package_alias("tool-content", "reports", "report");
        assert!(
            model.requests.lock().unwrap()[0]["tools"]
                .as_array()
                .unwrap()
                .iter()
                .any(|tool| tool["function"]["name"] == alias),
            "captured MCP tool was not registered: {page:?}"
        );
        modern::assert_lifecycle(&fixture.node.profile().join("plugins/data/tool-content"));
        let expected: Content = serde_json::from_str(include_str!(
            "../../../../../plugins/examples/tool-content/content.json"
        ))
        .unwrap();
        let view = super::super::progress::observe(&fixture.client, fixture.session.id).await;
        assert_eq!(view.calls.len(), 1);
        let call = &view.calls[0];
        assert_eq!(call.presentation, Presentation::Content);
        assert_eq!(call.content.as_ref(), Some(&expected));
        assert_eq!(call.result(&page).unwrap()["output"]["sample"], true);
        assert_eq!(call.images(&page).len(), 1);
        let image = call.images(&page)[0].clone();
        let requests = model.requests.lock().unwrap().len();
        assert_eq!(requests, 2);
        let path = fixture.node.profile().to_owned();
        fixture.node.shutdown().await.unwrap();
        let node = Node::start_empty(&path).await.unwrap();
        let client = Client::new(if remote {
            fixture.controller.handle().remote(node.link().address())
        } else {
            node.local()
        });
        assert_eq!(history(&client, fixture.session.id).await, page);
        let restored = super::super::progress::observe(&client, fixture.session.id).await;
        assert_eq!(restored.calls[0].content.as_ref(), Some(&expected));
        let Output::AttachmentDownload(download) = execute(
            &client,
            Command::DownloadImage {
                session: fixture.session.id,
                image: image.clone(),
            },
        )
        .await
        else {
            panic!("image download expected")
        };
        let mut bytes = Vec::new();
        client
            .download_attachment(
                &download,
                &mut bytes,
                sailry_link::CancellationToken::new(),
                |_| {},
            )
            .await
            .unwrap();
        assert!(bytes.starts_with(b"\x89PNG\r\n\x1a\n"));
        assert_eq!(
            blake3::hash(&bytes).to_hex().as_str(),
            image.attachment.spec.revision
        );
        assert_eq!(model.requests.lock().unwrap().len(), requests);
        node.shutdown().await.unwrap();
        fixture.controller.close().await.unwrap();
    }
}
