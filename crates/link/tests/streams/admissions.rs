use super::*;

async fn notified(signal: &tokio::sync::Semaphore) {
    tokio::time::timeout(Duration::from_secs(2), signal.acquire())
        .await
        .expect("command lifecycle deadline")
        .unwrap()
        .forget();
}

#[tokio::test]
async fn cancels_transient_preparation() {
    let fixture = Fixture::new().await;
    for _ in 0..12 {
        let remote = fixture.remote();
        let request = Request::new(fixture.target, Command::InspectHost);
        let job = tokio::spawn(async move { remote.dispatch(request).await });
        notified(&fixture.handler.preparing).await;
        job.abort();
        let _ = job.await;
        notified(&fixture.handler.cancelled).await;
    }
    fixture.close().await;
}

#[tokio::test]
async fn retains_unobserved_preparation() {
    let fixture = Fixture::new().await;
    let remote = fixture.remote();
    let request = Request::new(
        fixture.target,
        Command::RegisterProject {
            name: "fixture".into(),
            path: "fixture".into(),
        },
    );
    let job = tokio::spawn(async move { remote.dispatch(request).await });
    notified(&fixture.handler.preparing).await;
    job.abort();
    let _ = job.await;
    assert!(
        tokio::time::timeout(
            Duration::from_millis(100),
            fixture.handler.cancelled.acquire()
        )
        .await
        .is_err()
    );
    fixture.handler.release.add_permits(1);
    notified(&fixture.handler.cancelled).await;
    fixture.close().await;
}

fn installation(target: NodeId) -> Request {
    Request::new(
        target,
        Command::InstallHost {
            profile: SshId::new(),
            expected_revision: 1,
        },
    )
}

#[tokio::test]
async fn completion_outlives_the_transport_deadline() {
    let fixture = Fixture::new().await;
    let local: Arc<dyn Transport> = Arc::new(Local::new(
        fixture.target,
        fixture.caller,
        fixture.handler.clone(),
    ));
    let mut admissions = Vec::new();
    for transport in [local, fixture.remote()] {
        let request = installation(fixture.target);
        let admission = transport.dispatch(request.clone()).await.unwrap();
        assert_eq!(admission.receipt.id, request.id);
        assert!(admission.receipt.durable);
        admissions.push(admission);
    }
    // The transport's I/O deadline is ten seconds. Both receipts are already safe.
    tokio::time::sleep(Duration::from_secs(11)).await;
    for admission in &mut admissions {
        assert!(matches!(
            admission.completion.try_recv(),
            Err(tokio::sync::oneshot::error::TryRecvError::Empty)
        ));
    }
    let completions = std::mem::take(&mut *fixture.handler.completions.lock().unwrap());
    assert_eq!(completions.len(), 2);
    let output = Output::SshOutcome(ssh::Outcome::HostInstalled {
        node: fixture.target,
    });
    for sender in completions {
        sender.send(Ok(output.clone())).unwrap();
    }
    for admission in admissions {
        assert_eq!(admission.completion.await.unwrap().unwrap(), output);
    }
    fixture.close().await;
}

#[tokio::test]
async fn shutdown_interrupts_completion_without_replay() {
    let fixture = Fixture::new().await;
    let admission = fixture
        .remote()
        .dispatch(installation(fixture.target))
        .await
        .unwrap();
    fixture.controller.close().await.unwrap();
    let response = tokio::time::timeout(Duration::from_secs(2), admission.completion)
        .await
        .unwrap();
    // Link shutdown can drop the observer before it publishes its fault. Client
    // also maps that closed completion channel to an uncertain outcome.
    let result = response
        .map_err(|_| ErrorCode::OutcomeUnknown)
        .and_then(|result| result.map_err(|fault| fault.code));
    assert_eq!(result.unwrap_err(), ErrorCode::OutcomeUnknown);
    assert_eq!(fixture.handler.completions.lock().unwrap().len(), 1);
    fixture.execution.close().await.unwrap();
}
