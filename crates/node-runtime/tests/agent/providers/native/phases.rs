use super::*;
use sailry_client::conversation::View;
use sailry_link::CancellationToken;

#[tokio::test]
async fn separates_commentary_from_answers() {
    for remote in [false, true] {
        let server = crate::agent_support::Server::phases().await;
        let fixture =
            Fixture::packages(remote, ModelApi::Responses, &server.endpoint, &["files"]).await;
        std::fs::write(fixture.root.join("phase.txt"), "fixture").unwrap();
        let turn = fixture.submit("Read phase.txt").await;
        let page = finished(&fixture.client, fixture.session.id, turn).await;
        assert_eq!(page.runs.last().unwrap().status, Status::Completed);
        let answer = text(&page);
        assert!(answer.contains(crate::agent_support::phases::ANSWER));
        assert!(answer.contains(crate::agent_support::phases::COMMENTARY));
        let parts = page
            .entries
            .iter()
            .filter(|entry| entry.turn == turn && entry.author != "user")
            .flat_map(|entry| &entry.parts)
            .collect::<Vec<_>>();
        let commentary = parts.iter().position(|part| matches!(part,Part::Text(text) if text == crate::agent_support::phases::COMMENTARY)).unwrap();
        let call = parts
            .iter()
            .position(|part| matches!(part, Part::ToolCall { .. }))
            .unwrap();
        assert!(commentary < call);
        let boundary = sailry_client::conversation::reply::boundary(
            page.entries
                .iter()
                .filter(|entry| entry.turn == turn && entry.author != "user")
                .flat_map(|entry| {
                    entry.parts.iter().enumerate().map(|(index, part)| {
                        sailry_client::conversation::reply::is_work(
                            part,
                            entry.commentary.contains_key(&index),
                        )
                    })
                }),
        );
        let final_text = parts[boundary..]
            .iter()
            .filter_map(|part| match part {
                Part::Text(text) => Some(text.as_str()),
                _ => None,
            })
            .collect::<String>();
        assert_eq!(final_text, crate::agent_support::phases::ANSWER);
        drop(fixture.client);
        drop(fixture.controller);
        fixture.node.shutdown().await.unwrap();
    }
}

#[tokio::test]
async fn streams_commentary_before_tools() {
    for remote in [false, true] {
        let server = crate::agent_support::Server::phases().await;
        let fixture =
            Fixture::packages(remote, ModelApi::Responses, &server.endpoint, &["files"]).await;
        std::fs::write(fixture.root.join("phase.txt"), "fixture").unwrap();
        let (updates, mut observed) = tokio::sync::watch::channel(View::default());
        let stop = CancellationToken::new();
        let (_requests, requests) = tokio::sync::mpsc::channel(1);
        let watcher =
            fixture
                .client
                .watch_conversation(fixture.session.id, updates, stop.clone(), requests);
        let verify = async {
            let turn = fixture.submit("Read phase.txt").await;
            tokio::time::timeout(Duration::from_secs(10),async {
                loop {
                    if observed.borrow().snapshot.as_ref().is_some_and(|snapshot| snapshot.drafts.iter().any(|draft| {
                        draft.turn == turn && draft.parts.iter().any(|part| matches!(part,Part::Text(text) if text == crate::agent_support::phases::COMMENTARY))
                    })) { break; }
                    observed.changed().await.unwrap();
                }
            }).await.expect("commentary streaming deadline");
            let snapshot = observed.borrow().snapshot.clone().unwrap();
            assert!(!snapshot.page.entries.iter().any(|entry| {
                entry.turn == turn
                    && entry
                        .parts
                        .iter()
                        .any(|part| matches!(part, Part::ToolResult { .. }))
            }));
            finished(&fixture.client, fixture.session.id, turn).await;
            stop.cancel();
        };
        let (result, ()) = tokio::join!(watcher, verify);
        result.unwrap();
        fixture.controller.close().await.unwrap();
        fixture.node.shutdown().await.unwrap();
    }
}
