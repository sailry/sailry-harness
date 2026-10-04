use super::*;

fn pid(data: &Path) -> i32 {
    fs::read_to_string(data.join("pid"))
        .unwrap()
        .parse()
        .unwrap()
}

#[tokio::test]
async fn binds_reused_inputs() {
    use question::{Answer, Response};
    use std::sync::atomic::Ordering;
    for remote in [false, true] {
        for sse in [false, true] {
            for task in [false, true] {
                let peer = if task {
                    input::Server::task(
                        json!({"mode":"url","url":"https://example.invalid/continue"}),
                        sse,
                    )
                    .await
                } else {
                    input::Server::url("https://example.invalid/continue", sse).await
                };
                let model = Server::turn_tools(vec![(alias("input", "read"), json!({}))]).await;
                let fixture = process::Fixture::new(remote, &model).await;
                peer.package(&fixture.root.join("package"));
                install(&fixture, 0).await;

                for response in [Response::Answer(Answer::Opened), Response::Decline] {
                    let (_, turn) = submit(&fixture, false).await;
                    let (_, question) =
                        questions::pending(&fixture.client, fixture.session.id).await;
                    assert_eq!(question.turn, turn.id);
                    execute(
                        &fixture.client,
                        Command::ResolveQuestion {
                            session: fixture.session.id,
                            question: question.id,
                            response,
                        },
                    )
                    .await;
                    let page = finished(&fixture.client, fixture.session.id, turn.id).await;
                    assert_eq!(page.runs.last().unwrap().status, Status::Completed);
                    fixture
                        .controller
                        .handle()
                        .disconnect(fixture.node.id())
                        .await;
                }
                assert_eq!(peer.initializations.load(Ordering::SeqCst), 1);
                assert_eq!(peer.calls.load(Ordering::SeqCst), 2);
                assert_eq!(model.requests.lock().unwrap().len(), 4);
                fixture.node.shutdown().await.unwrap();
                fixture.controller.close().await.unwrap();
            }
        }
    }
}

#[tokio::test]
async fn replaces_changed_connections() {
    for remote in [false, true] {
        let model = Server::turn_tools(vec![(alias("native", "read"), json!({}))]).await;
        let fixture = process::Fixture::new(remote, &model).await;
        let servers = json!({"native":peer::config("mcp::peer::stdio_peer", "normal")});
        package(&fixture.root, servers.clone(), "first");
        install(&fixture, 0).await;

        let data = fixture.node.profile().join("plugins/data/example");
        let mut original = None;
        for _ in 0..2 {
            let (_, turn) = submit(&fixture, false).await;
            let page = finished(&fixture.client, fixture.session.id, turn.id).await;
            assert_eq!(page.runs.last().unwrap().status, Status::Completed);
            assert_eq!(results(&page).last().unwrap()["output"]["version"], "first");
            assert_eq!(fs::read_to_string(data.join("launches")).unwrap(), "x");
            assert_eq!(*original.get_or_insert(pid(&data)), pid(&data));
            fixture
                .controller
                .handle()
                .disconnect(fixture.node.id())
                .await;
        }
        package(&fixture.root, servers, "second");
        install(&fixture, 1).await;

        let (_, turn) = submit(&fixture, false).await;
        let page = finished(&fixture.client, fixture.session.id, turn.id).await;
        assert_eq!(
            results(&page).last().unwrap()["output"]["version"],
            "second"
        );
        assert_eq!(fs::read_to_string(data.join("launches")).unwrap(), "xx");
        assert_ne!(unsafe { libc::kill(original.unwrap(), 0) }, 0);
        fixture.node.shutdown().await.unwrap();
        reaped(&data);
        fixture.controller.close().await.unwrap();
    }
}

#[tokio::test]
async fn closes_idle_children() {
    let model = Server::tools(vec![(alias("native", "read"), json!({}))]).await;
    let mut fixture = process::Fixture::new(false, &model).await;
    package(
        &fixture.root,
        json!({"native":peer::config("mcp::peer::stdio_peer", "normal")}),
        "pooled",
    );
    install(&fixture, 0).await;

    let data = fixture.node.profile().join("plugins/data/example");
    let mut children = Vec::new();
    for _ in 0..9 {
        let Output::Session(session) = execute(
            &fixture.client,
            Command::CreateSession {
                project: fixture.session.project,
                worktree: Some(fixture.session.worktree),
                config: Some(fixture.session.config.clone()),
            },
        )
        .await
        else {
            panic!("session expected")
        };
        fixture.session = session;
        let (_, turn) = submit(&fixture, false).await;
        let page = finished(&fixture.client, fixture.session.id, turn.id).await;
        assert_eq!(page.runs.last().unwrap().status, Status::Completed);
        children.push(pid(&data));
    }
    assert_ne!(unsafe { libc::kill(children[0], 0) }, 0);
    assert!(
        children[1..]
            .iter()
            .all(|pid| unsafe { libc::kill(*pid, 0) } == 0)
    );
    fixture.node.shutdown().await.unwrap();
    assert!(
        children
            .iter()
            .all(|pid| unsafe { libc::kill(*pid, 0) } != 0)
    );
    fixture.controller.close().await.unwrap();
}
