use super::*;

#[tokio::test]
async fn completes_tools() {
    for api in APIS {
        for remote in [false, true] {
            let server = Server::start(api, Reply::Tool).await;
            let fixture = Fixture::new(remote, api, &server.endpoint).await;
            std::fs::write(fixture.root.join("native.txt"), "工具内容 🙂").unwrap();
            let turn = fixture.submit("Read native.txt").await;
            let page = finished(&fixture.client, fixture.session.id, turn).await;
            assert_eq!(
                page.runs.last().unwrap().status,
                Status::Completed,
                "{api:?}: {:?}",
                page.runs
            );
            assert!(!serde_json::to_string(&page).unwrap().contains(server::KEY));
            let requests = server.requests.lock().unwrap().clone();
            assert_eq!(requests.len(), 2, "{api:?}");
            assert!(text(&page).contains(server::ANSWER));
            assert!(requests[1].body.to_string().contains("工具内容 🙂"));
            assert!(
                page.entries
                    .iter()
                    .flat_map(|entry| &entry.parts)
                    .any(|part| matches!(part, Part::ToolResult { .. }))
            );
            fixture.controller.close().await.unwrap();
            fixture.node.shutdown().await.unwrap();
        }
    }
}

#[tokio::test]
async fn bounds_transient_failure_retries() {
    for api in APIS {
        for remote in [false, true] {
            let server = Server::start(api, Reply::Failure).await;
            let fixture = Fixture::new(remote, api, &server.endpoint).await;
            let turn = fixture.submit("Failure fixture").await;
            let page = finished(&fixture.client, fixture.session.id, turn).await;
            assert_eq!(
                page.runs.last().unwrap().status,
                Status::Failed,
                "{api:?}: {:?}",
                page.runs
            );
            assert!(!serde_json::to_string(&page).unwrap().contains(server::KEY));
            let attempts = if api == ModelApi::Bedrock { 1 } else { 6 };
            assert_eq!(server.requests.lock().unwrap().len(), attempts, "{api:?}");
            fixture.controller.close().await.unwrap();
            fixture.node.shutdown().await.unwrap();
        }
    }
}

#[tokio::test]
async fn rejects_redirects() {
    for api in APIS {
        for remote in [false, true] {
            let target = Server::start(api, Reply::Text).await;
            let server = Server::start(api, Reply::Redirect(target.endpoint.clone())).await;
            let fixture = Fixture::new(remote, api, &server.endpoint).await;
            let turn = fixture.submit("Redirect fixture").await;
            let page = finished(&fixture.client, fixture.session.id, turn).await;
            assert_eq!(
                page.runs.last().unwrap().status,
                Status::Failed,
                "{api:?}: {:?}",
                page.runs
            );
            assert!(!serde_json::to_string(&page).unwrap().contains(server::KEY));
            assert_eq!(server.requests.lock().unwrap().len(), 1, "{api:?}");
            assert!(target.requests.lock().unwrap().is_empty());
            fixture.controller.close().await.unwrap();
            fixture.node.shutdown().await.unwrap();
        }
    }
}
