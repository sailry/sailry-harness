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
async fn reports_failures_once_without_payloads() {
    let fixture = Fixture::new().await;

    let failed = Capture::default();
    let request = Request::new(
        NodeId([7; 32]),
        Command::RegisterProject {
            name: "PRIVATE_PROJECT".into(),
            path: "PRIVATE_PATH".into(),
        },
    );
    let request_id = request.id;
    let result = fixture
        .remote()
        .dispatch(request)
        .with_subscriber(failed.dispatch())
        .await;
    let Err(error) = result else {
        panic!("wrong-target request must fail")
    };
    assert_eq!(error.code, ErrorCode::WrongTarget);
    let records = failed.records();
    assert_eq!(records.len(), 1);
    let record = &records[0];
    let fields = &record["fields"];
    assert_eq!(record["target"], "sailry");
    assert_eq!(record["level"], "ERROR");
    assert_eq!(fields["area"], "link");
    assert_eq!(fields["operation"], "remote_command");
    assert_eq!(fields["phase"], "failed");
    assert_eq!(fields["stage"], "target");
    assert_eq!(fields["reason"], "target_mismatch");
    assert_eq!(fields["request"], request_id.to_string());
    assert_eq!(fields["code"], "wrong_target");
    assert!(fields["duration_us"].is_u64());
    assert!(fields.get("durable").is_none());
    assert!(
        !serde_json::to_string(&records)
            .unwrap()
            .contains("PRIVATE_")
    );

    let rejected = Capture::default();
    let result = fixture
        .remote()
        .dispatch(Request::new(fixture.target, Command::ListPlugins))
        .with_subscriber(rejected.dispatch())
        .await;
    let Err(error) = result else {
        panic!("rejected request must fail")
    };
    assert_eq!(error.code, ErrorCode::NotFound);
    assert!(rejected.records().is_empty());

    let completed = Capture::default();
    let admission = fixture
        .remote()
        .dispatch(installation(fixture.target))
        .with_subscriber(completed.dispatch())
        .await
        .unwrap();
    fixture
        .handler
        .completions
        .lock()
        .unwrap()
        .pop()
        .unwrap()
        .send(Err(Fault::new(
            ErrorCode::PermissionDenied,
            "PRIVATE_NODE_FAILURE",
        )))
        .unwrap();
    assert_eq!(
        admission.completion.await.unwrap().unwrap_err().code,
        ErrorCode::PermissionDenied
    );
    assert!(completed.records().is_empty());

    let interrupted = Capture::default();
    let admission = fixture
        .remote()
        .dispatch(installation(fixture.target))
        .with_subscriber(interrupted.dispatch())
        .await
        .unwrap();
    let request_id = admission.receipt.id;
    fixture.execution.close().await.unwrap();
    let error = tokio::time::timeout(Duration::from_secs(2), admission.completion)
        .await
        .unwrap()
        .unwrap()
        .unwrap_err();
    assert_eq!(error.code, ErrorCode::OutcomeUnknown);
    let records = interrupted.records();
    assert_eq!(records.len(), 1);
    let record = &records[0];
    let fields = &record["fields"];
    assert_eq!(record["target"], "sailry");
    assert_eq!(record["level"], "WARN");
    assert_eq!(fields["area"], "link");
    assert_eq!(fields["operation"], "remote_command");
    assert_eq!(fields["phase"], "interrupted");
    assert_eq!(fields["stage"], "completion");
    assert_eq!(fields["reason"], "completion_missing");
    assert_eq!(fields["request"], request_id.to_string());
    assert_eq!(fields["code"], "outcome_unknown");
    assert_eq!(fields["durable"], true);
    assert!(fields["duration_us"].is_u64());
    assert!(
        !serde_json::to_string(&records)
            .unwrap()
            .contains("PRIVATE_")
    );

    fixture.controller.close().await.unwrap();
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
