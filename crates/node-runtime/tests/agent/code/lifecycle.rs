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
        let mut page = fixture.finish(turn).await;
        page.entries.retain(|entry| entry.turn == turn);
        let output = results(&page);
        assert_eq!(output.len(), 3);
        assert!(
            output[0]
                .as_array()
                .unwrap()
                .iter()
                .flat_map(|language| language["servers"].as_array().unwrap())
                .any(|candidate| candidate["server"] == "rust-analyzer"
                    && candidate["status"] == "stopped")
        );
        assert_eq!(output[1], &json!([]));
        assert_eq!(output[2], &json!([]));
        assert!(!fixture.root.join("pid").exists());
        server.replace_tools(vec![(
            plugin_tool("code", "code_start"),
            serde_json::to_value(&startup).unwrap(),
        )]);
        let turn = fixture.submit().await;
        let mut page = fixture.finish(turn).await;
        page.entries.retain(|entry| entry.turn == turn);
        let output = results(&page);
        assert_eq!(output.len(), 1);
        assert_eq!(output[0]["status"], "ready");
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
        let mut page = fixture.finish(turn).await;
        page.entries.retain(|entry| entry.turn == turn);
        let output = results(&page);
        assert_eq!(output.len(), 1);
        assert_eq!(output[0]["status"], "stopped");
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
        let mut page = fixture.finish(turn).await;
        page.entries.retain(|entry| entry.turn == turn);
        assert_eq!(results(&page).len(), 1);
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
