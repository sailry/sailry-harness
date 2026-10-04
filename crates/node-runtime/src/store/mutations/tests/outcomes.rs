use super::*;

#[tokio::test]
async fn observes_blocked_admission() {
    let (fixture, mut gate) = blocked().await;
    let network = Link::bind(
        &fixture.identity,
        NetworkScope::default(),
        fixture.store.ingress.clone(),
        fixture.store.ingress.clone(),
    )
    .await
    .unwrap();
    let controller = Link::controller(
        fixture.directory.path().join("controller"),
        NetworkScope::default(),
    )
    .await
    .unwrap();
    let address = controller
        .handle()
        .pair(network.handle().invite().unwrap().ticket())
        .await
        .unwrap();
    let remote = Client::new(controller.handle().remote(address));
    for client in [&fixture.client, &remote] {
        let request = fixture.write("blocked", "content");
        let admission = client.dispatch(request.clone()).await.unwrap();
        gate.entered.recv().await.unwrap();
        tokio::time::timeout(Duration::from_secs(2), async {
            assert_eq!(
                client.outcome(&request).await.unwrap(),
                RequestOutcome::Admitted
            );
            let query = client.prepare(Command::InspectRequest {
                id: request.id,
                digest: blake3::hash(&serde_json::to_vec(&request).unwrap())
                    .to_hex()
                    .to_string(),
            });
            let read = client.dispatch(query).await.unwrap();
            assert!(!read.receipt.durable);
            assert_eq!(
                read.completion.await.unwrap().unwrap(),
                Output::RequestOutcome {
                    id: request.id,
                    outcome: RequestOutcome::Admitted
                },
            );
        })
        .await
        .expect("outcome reads must not wait for filesystem execution");
        gate.release.send(()).unwrap();
        let result = admission.completion.await.unwrap();
        assert_eq!(
            client.outcome(&request).await.unwrap(),
            RequestOutcome::Completed(Box::new(result))
        );
    }
    controller.close().await.unwrap();
    network.close().await.unwrap();
    fixture.store.shutdown().await.unwrap();
}
