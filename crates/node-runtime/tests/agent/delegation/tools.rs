use super::{fixture::Fixture, *};

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn reads_git_on_worker_stack() {
    let parent = Server::parallel(vec![(
        "spawn_agent".into(),
        json!({"role":"review","task":"Inspect Git status twice and read source.txt"}),
    )])
    .await;
    let child = Server::tools(vec![
        (plugin_tool("git", "git_status"), json!({})),
        (plugin_tool("git", "git_status"), json!({})),
        (
            plugin_tool("files", "read_file"),
            json!({"path":"source.txt"}),
        ),
    ])
    .await;
    let fixture = Fixture::start(parent, child).await;
    git2::Repository::init(fixture.root()).unwrap();
    std::fs::write(fixture.root().join("source.txt"), "Delegated inspection\n").unwrap();
    let turn = fixture.push("Delegate the repository inspection").await;
    let page = finished(&fixture.client, fixture.session.id, turn.id).await;
    assert_eq!(page.runs[0].status, Status::Completed, "{page:?}");
    let child = children(&fixture.client, fixture.session.id, 1)
        .await
        .pop()
        .unwrap();
    let page = history(&fixture.client, child.id).await;
    assert_eq!(page.runs[0].status, Status::Completed, "{page:?}");
    assert!(page.approvals.is_empty(), "{page:?}");
    let output = results(&fixture.client, &page).await;
    assert_eq!(output.len(), 3, "{output:?}");
    for status in &output[..2] {
        assert_eq!(status["kind"], "git_status", "{status}");
        assert_eq!(status["data"]["kind"], "unborn", "{status}");
        assert!(
            status["data"]["entries"]
                .as_array()
                .unwrap()
                .iter()
                .any(|entry| entry["path"] == "source.txt"),
            "{status}"
        );
    }
    assert_eq!(
        output[2]["data"]["lines"][0]["text"],
        "Delegated inspection\n"
    );
    fixture.close().await;
}

#[cfg(unix)]
#[tokio::test]
async fn shares_mcp_with_the_parent() {
    let tool = crate::mcp::peer::package_alias("shared-mcp", "native", "read");
    let parent = Server::turn_tools(vec![
        (
            "tool_search".into(),
            json!({"query":tool.clone(),"limit":1}),
        ),
        (tool.clone(), json!({})),
        (
            "spawn_agent".into(),
            json!({"role":"review","task":"Read the shared MCP fixture"}),
        ),
    ])
    .await;
    let child = Server::tools(vec![(tool.clone(), json!({}))]).await;
    let mut fixture = Fixture::start(parent, child).await;
    let path = fixture.root().join("shared-mcp");
    std::fs::create_dir(&path).unwrap();
    std::fs::write(
        path.join("plugin.json"),
        json!({"$schema":"https://agent-plugins.org/schemas/1.0.0/plugin.schema.json",
            "name":"shared-mcp","version":"0.1.0",
            "extensions":{"dev.sailry.platform":{"api_version":"v1",
                "tools":[{"server":"native","name":"read"}]}}})
        .to_string(),
    )
    .unwrap();
    std::fs::write(
        path.join("mcp.json"),
        json!({"$schema":"https://agent-plugins.org/schemas/1.0.0/mcp.schema.json",
            "mcpServers":{"native":crate::mcp::peer::config("mcp::peer::stdio_peer", "normal")}})
        .to_string(),
    )
    .unwrap();
    std::fs::write(path.join("version.txt"), "shared").unwrap();
    fixture
        .client
        .execute(fixture.client.prepare(Command::InstallPlugin {
            worktree: fixture.session.worktree,
            path: "shared-mcp".into(),
            name: "shared-mcp".into(),
            expected_revision: 0,
            expected_digest: None,
        }))
        .await
        .unwrap();
    let mut role = fixture.role.clone();
    role.tools = Some(vec![plugin::conversation::Tool::Package {
        package: "shared-mcp".into(),
        name: "read".into(),
    }]);
    let Output::Role(role) = fixture
        .client
        .execute(fixture.client.prepare(Command::PutRole {
            expected_revision: role.revision,
            role,
        }))
        .await
        .unwrap()
    else {
        panic!("role expected")
    };
    let Output::Session(session) = fixture
        .client
        .execute(fixture.client.prepare(Command::SetSessionRoles {
            session: fixture.session.id,
            expected_revision: fixture.session.revision,
            roles: vec![role.reference()],
        }))
        .await
        .unwrap()
    else {
        panic!("session expected")
    };
    fixture.session = session;
    let turn = fixture.push("Read, then delegate another read").await;
    let page = finished(&fixture.client, fixture.session.id, turn.id).await;
    assert_eq!(page.runs[0].status, Status::Completed, "{page:?}");
    let parent_details = tool_details(&fixture.client, &page).await;
    let parent_result = &parent_details
        .iter()
        .find(|(name, _)| name == &tool)
        .expect("parent MCP result expected")
        .1
        .result;
    assert_eq!(
        parent_result.as_ref().unwrap()["structuredContent"]["version"],
        "shared",
        "{parent_details:?}"
    );
    let child = children(&fixture.client, fixture.session.id, 1)
        .await
        .remove(0);
    let page = history(&fixture.client, child.id).await;
    assert_eq!(page.runs[0].status, Status::Completed, "{page:?}");
    let child_details = tool_details(&fixture.client, &page).await;
    let child_result = &child_details
        .iter()
        .find(|(name, _)| name == &tool)
        .expect("child MCP result expected")
        .1
        .result;
    assert_eq!(
        child_result.as_ref().unwrap()["structuredContent"]["version"],
        "shared",
        "{child_details:?}"
    );
    let data = fixture.node.profile().join("plugins/data/shared-mcp");
    assert_eq!(std::fs::read(data.join("launches")).unwrap(), b"x");
    let requests = std::fs::read_to_string(data.join("requests.jsonl")).unwrap();
    let requests: Vec<Value> = requests
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    assert_eq!(
        requests
            .iter()
            .filter(|request| request["method"] == "initialize")
            .count(),
        1
    );
    assert_eq!(
        requests
            .iter()
            .filter(|request| request["method"] == "tools/list")
            .count(),
        2
    );
    assert_eq!(
        requests
            .iter()
            .filter(|request| request["method"] == "tools/call")
            .count(),
        2
    );
    fixture.close().await;
}
