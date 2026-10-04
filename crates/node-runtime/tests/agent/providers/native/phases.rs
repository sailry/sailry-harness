use super::*;

#[tokio::test]
async fn separates_commentary_from_answers() {
    for remote in [false, true] {
        let server = crate::agent_support::Server::phases().await;
        let fixture = Fixture::new(remote, ModelApi::Responses, &server.endpoint).await;
        std::fs::write(fixture.root.join("phase.txt"), "fixture").unwrap();
        let turn = fixture.submit("Read phase.txt").await;
        let page = finished(&fixture.client, fixture.session.id, turn).await;
        assert_eq!(page.runs.last().unwrap().status, Status::Completed);
        let answer = text(&page);
        assert!(answer.contains(crate::agent_support::phases::ANSWER));
        assert!(!answer.contains(crate::agent_support::phases::COMMENTARY));
        drop(fixture.client);
        drop(fixture.controller);
        fixture.node.shutdown().await.unwrap();
    }
}
