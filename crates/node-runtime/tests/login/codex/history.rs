use super::*;
use std::sync::{
    Mutex,
    atomic::{AtomicUsize, Ordering},
};
#[path = "../../support/reasoning.rs"]
mod fixture;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn preserves_only_encrypted_history() {
    for remote in [false, true] {
        let count = Arc::new(AtomicUsize::new(0));
        let generation = count.clone();
        let requests = Arc::new(Mutex::new(Vec::new()));
        let bodies = requests.clone();
        let server = Server::start_with_request(ModelApi::Anthropic, move |request| {
            if request.path != "/responses" {
                return server::standard(request, 3600);
            }
            if request.method == "GET" {
                return Reply::Raw("HTTP/1.1 426 Upgrade Required\r\nContent-Length: 0\r\nConnection: close\r\n\r\n".into());
            }
            let body = server::input(
                request,
                &Options::defaults(Authentication::ChatGptOAuth).unwrap(),
                ACCESS,
            );
            let index = generation.fetch_add(1, Ordering::SeqCst);
            fixture::payload(&body, index);
            bodies.lock().unwrap().push(body);
            server::stream(vec![server::completed(fixture::output(index))], true)
        })
        .await;
        let fixture = Fixture::packages(remote, &server, &["files"]).await;
        let mut provider = configured(&fixture, None).await;
        provider.models[0].reasoning = false;
        provider.models[0].capabilities = None;
        provider.models[0].efforts.clear();
        provider.models[0].default_effort = Effort::Default;
        let Output::Provider(provider) = fixture
            .client
            .execute(fixture.client.prepare(Command::PutProvider {
                expected_revision: provider.revision,
                provider,
            }))
            .await
            .unwrap()
        else {
            panic!("provider expected")
        };
        let session = super::super::execution::session(&fixture, &provider).await;
        let mut original = Vec::new();
        for index in 0..4 {
            let (_, turn) = super::super::execution::submit(&fixture.client, &session).await;
            let page =
                super::super::execution::finished(&fixture.client, &turn, Status::Completed).await;
            assert!(answer(&page, turn.id).contains(&format!("Stateless answer {index}")));
            assert_eq!(requests.lock().unwrap().len(), index + 1);
            let events = fixture::events(fixture.node.profile(), session.id);
            if index == 0 {
                assert!(
                    events
                        .iter()
                        .any(|body| String::from_utf8_lossy(body).contains("stateless-0"))
                );
                original = events;
            } else {
                assert_eq!(&events[..original.len()], &original);
            }
        }
        fixture.close().await;
    }
}
