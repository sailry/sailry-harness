use super::*;
#[path = "../../../support/reasoning.rs"]
pub(super) mod fixture;

#[tokio::test]
async fn preserves_only_encrypted_history() {
    for remote in [false, true] {
        let reply = Reply::Dynamic(Arc::new(|index| server::Script {
            bytes: fixture::stream(index),
            ending: server::Ending::Close,
        }));
        let server = Server::start(ModelApi::Responses, reply).await;
        let mut current = Fixture::empty(remote, ModelApi::Responses, &server.endpoint).await;
        let mut provider = current.provider.clone();
        provider.models[0].reasoning = true;
        provider.models[0].capabilities =
            Some(serde_json::from_value(serde_json::json!({"reasoning":true})).unwrap());
        provider.models[0].efforts = vec![Effort::Disabled];
        provider.models[0].default_effort = Effort::Disabled;
        let Output::Provider(provider) = execute(
            &current.client,
            Command::PutProvider {
                expected_revision: provider.revision,
                provider,
            },
        )
        .await
        else {
            panic!("provider expected")
        };
        current.provider = provider;
        let mut config = current.session.config.clone();
        config.effort = Effort::Disabled;
        let Output::Session(session) = execute(
            &current.client,
            Command::SetSessionConfig {
                session: current.session.id,
                expected_revision: current.session.revision,
                config,
            },
        )
        .await
        else {
            panic!("session expected")
        };
        current.session = session;
        let mut original = Vec::new();
        for index in 0..4 {
            let turn = current.submit("Continue this stateless conversation").await;
            let page = finished(&current.client, current.session.id, turn).await;
            assert_eq!(
                page.runs.last().unwrap().status,
                Status::Completed,
                "{:?}",
                page.runs
            );
            let requests = server.requests.lock().unwrap();
            assert_eq!(requests.len(), index + 1);
            fixture::payload(&requests[index].body, index);
            drop(requests);
            assert!(page.entries.iter().flat_map(|entry|&entry.parts).any(|part| {
                matches!(part, Part::Thinking(value) if value.contains("Legacy summary without encrypted content"))
            }));
            let events = fixture::events(current.node.profile(), current.session.id);
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
        current.controller.close().await.unwrap();
        current.node.shutdown().await.unwrap();
    }
}
