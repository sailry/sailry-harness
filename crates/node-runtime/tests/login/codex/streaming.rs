use super::*;
use sailry_client::conversation::View;
use sailry_link::CancellationToken;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn exposes_unlabelled_sse_before_completion() {
    for remote in [false, true] {
        let release = Arc::new(tokio::sync::Notify::new());
        let gate = release.clone();
        let server = Server::start_with_request(ModelApi::Anthropic, move |request| {
            if request.path != "/responses" {
                return server::standard(request, 3600);
            }
            server::input(
                request,
                &Options::defaults(Authentication::ChatGptOAuth).unwrap(),
                ACCESS,
            );
            let first = server::delta("First Codex fragment");
            let last = server::completed(json!([server::message(
                "First Codex fragment and completion"
            )]));
            Reply::Staged {
                initial: format!("HTTP/1.1 200 OK\r\nConnection: close\r\n\r\ndata: {first}\n\n"),
                release: gate.clone(),
                remaining: format!("data: {last}\n\ndata: [DONE]\n\n"),
            }
        })
        .await;
        let fixture = Fixture::new(remote, &server).await;
        let provider = configured(&fixture, None).await;
        let session = super::super::execution::session(&fixture, &provider).await;
        let (updates, mut observed) = tokio::sync::watch::channel(View::default());
        let stop = CancellationToken::new();
        let (_requests, requests) = tokio::sync::mpsc::channel(1);
        let watcher =
            fixture
                .client
                .watch_conversation(session.id, updates, stop.clone(), requests);
        let verify = async {
            let (_, turn) = super::super::execution::submit(&fixture.client, &session).await;
            tokio::time::timeout(Duration::from_secs(10), async {
                loop {
                    if observed.borrow().snapshot.as_ref().is_some_and(|snapshot| {
                        snapshot.drafts.iter().any(|draft| {
                            draft.turn == turn.id
                                && draft.parts == vec![Part::Text("First Codex fragment".into())]
                        })
                    }) {
                        break;
                    }
                    observed.changed().await.unwrap();
                }
            })
            .await
            .expect("Codex first fragment deadline");
            {
                let observed = observed.borrow();
                let snapshot = observed.snapshot.as_ref().unwrap();
                assert!(observed.connected);
                assert_eq!(
                    snapshot
                        .page
                        .runs
                        .iter()
                        .find(|run| run.turn == turn.id)
                        .unwrap()
                        .status,
                    Status::Running
                );
                assert!(answer(&snapshot.page, turn.id).is_empty());
            }
            release.notify_one();
            let page =
                super::super::execution::finished(&fixture.client, &turn, Status::Completed).await;
            assert_eq!(
                answer(&page, turn.id),
                "First Codex fragment and completion"
            );
            assert_eq!(
                server
                    .requests
                    .lock()
                    .unwrap()
                    .iter()
                    .filter(|request| request.path == "/responses")
                    .count(),
                1
            );
            no_secrets(&page);
            stop.cancel();
        };
        let (result, ()) = tokio::join!(watcher, verify);
        result.unwrap();
        fixture.close().await;
    }
}
