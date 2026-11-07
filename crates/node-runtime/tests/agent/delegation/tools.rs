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
