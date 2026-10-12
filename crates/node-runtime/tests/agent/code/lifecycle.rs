use super::*;

async fn servers(fixture: &Fixture) -> Vec<code::Server> {
    let Output::CodeServers(servers) = fixture
        .client
        .execute(fixture.client.prepare(Command::ListCodeServers {
            worktree: fixture.session.worktree,
        }))
        .await
        .unwrap()
    else {
        panic!("language instances expected")
    };
    servers
}

#[tokio::test]
async fn separates_inventory_from_scoped_lifecycle() {
    for remote in [false, true] {
        let server = Server::scripted_tools(Vec::new()).await;
        let mut fixture = Fixture::new(remote, &server).await;
        fixture.config(WorkMode::Code, Permission::Full).await;
        let startup = fixture
            .read(
                Query::Hover {
                    path: "main.rs".into(),
                    line: 1,
                    symbol: "symbol".into(),
                },
                None,
            )
            .await
            .startup
            .unwrap();
        server.replace_tools(vec![
            (plugin_tool("code", "code_catalog"), json!({})),
            (
                plugin_tool("code", "code_packages"),
                json!({"installed":true}),
            ),
            (plugin_tool("code", "code_servers"), json!({})),
        ]);
        let turn = fixture.submit().await;
        let page = fixture.finish(turn).await;
        let output = results(&page);
        let catalog = output
            .iter()
            .find(|result| result["kind"] == "code_catalog")
            .unwrap();
        assert!(
            catalog["data"]
                .as_array()
                .unwrap()
                .iter()
                .flat_map(|language| language["servers"].as_array().unwrap())
                .any(|candidate| candidate["server"] == "rust-analyzer"
                    && candidate["status"] == "stopped")
        );
        assert!(output.iter().any(|result| result["kind"] == "code_installations" && result["data"] == json!([])));
        assert!(
            output
                .iter()
                .any(|result| result["kind"] == "code_servers" && result["data"] == json!([]))
        );
        assert!(!fixture.root.join("pid").exists());
        server.replace_tools(vec![(
            plugin_tool("code", "code_start"),
            serde_json::to_value(&startup).unwrap(),
        )]);
        let turn = fixture.submit().await;
        let page = fixture.finish(turn).await;
        assert!(
            results(&page)
                .iter()
                .any(|result| result["kind"] == "code_server")
        );
        assert!(
            page.approvals
                .iter()
                .filter(|approval| approval.turn == turn)
                .all(|approval| approval.source == ApprovalSource::Full)
        );
        assert_eq!(servers(&fixture).await[0].startup, startup);
        let before = fixture
            .client
            .execute(fixture.client.prepare(Command::ReadCodeSettings))
            .await
            .unwrap();
        server.replace_tools(vec![(
            plugin_tool("code", "code_stop"),
            serde_json::to_value(&startup).unwrap(),
        )]);
        let turn = fixture.submit().await;
        let page = fixture.finish(turn).await;
        assert!(
            results(&page)
                .iter()
                .any(|result| result["kind"] == "code_server"
                    && result["data"]["status"] == "stopped")
        );
        assert!(servers(&fixture).await.is_empty());
        assert_eq!(
            fixture
                .client
                .execute(fixture.client.prepare(Command::ReadCodeSettings))
                .await
                .unwrap(),
            before
        );
        let mut forged = startup.clone();
        forged.worktree = WorktreeId::new();
        server.replace_tools(vec![(
            plugin_tool("code", "code_start"),
            serde_json::to_value(forged).unwrap(),
        )]);
        let turn = fixture.submit().await;
        let page = fixture.finish(turn).await;
        assert!(
            results(&page)
                .iter()
                .any(|result| result.to_string().contains("outside the captured worktree"))
        );
        assert!(servers(&fixture).await.is_empty());
        fixture.node.shutdown().await.unwrap();
        fixture.controller.close().await.unwrap();
    }
}
