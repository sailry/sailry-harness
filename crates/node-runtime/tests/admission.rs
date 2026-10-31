//! Admission is durable before derived publication, on both Client transports.
use crate::file_fixture::Fixture;
use sailry_protocol::{Command, Event, Output, Update};
use std::time::Duration;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn acknowledges_before_publication_and_survives_caller_drop() {
    let fixture = Fixture::empty().await;
    for (transport, client) in fixture.clients().into_iter().enumerate() {
        for abandon in [false, true] {
            let root = fixture
                .root
                .join(format!("admission-{transport}-{abandon}"));
            std::fs::create_dir(&root).unwrap();
            let request = client.prepare(Command::RegisterProject {
                name: "Receipt fixture".into(),
                path: root.to_string_lossy().into_owned(),
            });
            let mut updates = fixture.clients()[0].subscribe().await.unwrap();
            assert!(matches!(updates.next().await.unwrap(), Update::Snapshot(_)));
            let (entered, release) = fixture.node.hold_publication_fixture().await.unwrap();
            let mut admission =
                tokio::time::timeout(Duration::from_secs(10), client.dispatch(request.clone()))
                    .await
                    .expect("receipt must not wait for publication")
                    .unwrap();
            tokio::time::timeout(Duration::from_secs(10), entered.notified())
                .await
                .expect("publication gate must be entered");
            assert!(admission.receipt.durable);
            assert_eq!(admission.receipt.id, request.id);
            assert!(matches!(
                admission.completion.try_recv(),
                Err(tokio::sync::oneshot::error::TryRecvError::Empty)
            ));
            // Replay observes the committed result even while original publication is held.
            let replay =
                tokio::time::timeout(Duration::from_secs(10), client.execute(request.clone()))
                    .await
                    .expect("replay must use the durable result")
                    .unwrap();
            let Output::Project(project) = &replay else {
                panic!("project expected")
            };
            if abandon {
                drop(admission);
                release.send(()).unwrap();
            } else {
                release.send(()).unwrap();
                assert_eq!(admission.completion.await.unwrap().unwrap(), replay);
            }
            tokio::time::timeout(Duration::from_secs(10), async {
                loop {
                    if let Update::Event(envelope) = updates.next().await.unwrap()
                        && let Event::ProjectRegistered(published) = envelope.event
                        && published.id == project.id
                    {
                        break;
                    }
                }
            })
            .await
            .expect("caller drop must not cancel publication");
            assert_eq!(client.execute(request).await.unwrap(), replay);
        }
    }
    fixture.close().await;
}
