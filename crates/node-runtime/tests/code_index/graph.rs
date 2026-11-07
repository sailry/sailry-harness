use super::*;
use sailry_protocol::code_index::{Confidence, Graph, View};
use std::time::Duration;

#[tokio::test]
async fn traces_references_on_both_transports() {
    let fixture = Fixture::empty().await;
    std::fs::create_dir(fixture.root.join("src")).unwrap();
    for (path, source) in [
        ("src/store.rs", "pub fn open() {}\n"),
        ("src/cli.rs", "pub fn main() {\n    open();\n}\n"),
    ] {
        std::fs::write(fixture.root.join(path), source).unwrap();
    }
    for client in fixture.clients() {
        let callers = client
            .graph_code(
                fixture.worktree,
                Graph {
                    name: Some("open".into()),
                    ..Graph::new(View::Callers)
                },
            )
            .await
            .unwrap();
        assert_eq!(callers.targets[0].path, "src/store.rs");
        assert_eq!(callers.callers.len(), 1, "{callers:?}");
        let caller = &callers.callers[0];
        assert_eq!(
            (caller.path.as_str(), caller.line, caller.confidence),
            ("src/cli.rs", 1, Some(Confidence::Directory))
        );
        assert_eq!(caller.scope.as_ref().unwrap().name, "main");
        let impact = client
            .graph_code(
                fixture.worktree,
                Graph {
                    paths: vec!["src/store.rs".into()],
                    ..Graph::new(View::Impact)
                },
            )
            .await
            .unwrap();
        assert_eq!(impact.impact[0].path, "src/cli.rs");
        assert_eq!(impact.impact[0].depth, 1);
        let map = client
            .graph_code(fixture.worktree, Graph::new(View::Map))
            .await
            .unwrap();
        assert_eq!(map.map[0].path, "src/store.rs");
        assert_eq!(map.index_revision, callers.index_revision);
        assert_eq!(
            client
                .graph_code(fixture.worktree, Graph::new(View::Callers))
                .await
                .unwrap_err()
                .code,
            ErrorCode::InvalidRequest
        );
    }
    fixture.close().await;
}

#[tokio::test]
async fn cancels_admitted_graph_queries() {
    let fixture = Fixture::empty().await;
    std::fs::write(fixture.root.join("main.rs"), "pub fn retained() {}\n").unwrap();
    for client in fixture.clients() {
        let (entered, release) = fixture.node.hold_code_query_fixture();
        let request = client.prepare(Command::GraphCode {
            worktree: fixture.worktree,
            query: Graph::new(View::Map),
        });
        let id = request.id;
        let admission = client.dispatch(request).await.unwrap();
        assert!(!admission.receipt.durable);
        tokio::time::timeout(Duration::from_secs(5), entered)
            .await
            .unwrap()
            .unwrap();
        client
            .execute(client.prepare(Command::CancelCodeIndexQuery { request: id }))
            .await
            .unwrap();
        assert_eq!(
            tokio::time::timeout(Duration::from_secs(5), admission.completion)
                .await
                .unwrap()
                .unwrap()
                .unwrap_err()
                .code,
            ErrorCode::Cancelled
        );
        release.cancel();
    }
    fixture.close().await;
}
