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
            network: None,
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
    let records = rejected.records();
    assert_eq!(records.len(), 2);
    assert_eq!(records[0]["fields"]["phase"], "begin");
    assert_eq!(records[1]["fields"]["phase"], "rejected");
    assert_eq!(records[1]["fields"]["communication_outcome"], "succeeded");
    assert_eq!(records[1]["fields"]["execution_outcome"], "rejected");
    assert_eq!(records[1]["fields"]["code"], "not_found");
    assert!(records[1]["fields"]["end_to_end_us"].is_u64());

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
    let records = completed.records();
    assert_eq!(records.len(), 2);
    let fields = &records[1]["fields"];
    assert_eq!(fields["phase"], "completed");
    assert_eq!(fields["communication_outcome"], "succeeded");
    assert_eq!(fields["execution_outcome"], "failed");
    assert_eq!(fields["code"], "permission_denied");
    assert!(fields["end_to_end_us"].is_u64());
    assert!(
        !serde_json::to_string(&records)
            .unwrap()
            .contains("PRIVATE_")
    );

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
    assert_eq!(records.len(), 2);
    assert_eq!(records[0]["fields"]["phase"], "begin");
    let record = &records[1];
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
    assert_eq!(fields["communication_outcome"], "interrupted");
    assert_eq!(fields["execution_outcome"], "unknown");
    assert_eq!(fields["recovery_decision"], "same_request");
    assert!(fields["duration_us"].is_u64());
    assert!(
        !serde_json::to_string(&records)
            .unwrap()
            .contains("PRIVATE_")
    );

    fixture.controller.close().await.unwrap();
}

#[tokio::test]
async fn reports_successful_round_trips() {
    let fixture = Fixture::new().await;
    let captured = Capture::default();
    let request = installation(fixture.target);
    let request_id = request.id;
    let admission = fixture
        .remote()
        .dispatch(request)
        .with_subscriber(captured.dispatch())
        .await
        .unwrap();
    let before = captured.records();
    assert_eq!(before.len(), 1);
    assert_eq!(before[0]["fields"]["phase"], "begin");
    assert!(before[0]["fields"].get("end_to_end_us").is_none());
    let output = Output::SshOutcome(ssh::Outcome::HostInstalled {
        node: fixture.target,
    });
    fixture
        .handler
        .completions
        .lock()
        .unwrap()
        .pop()
        .unwrap()
        .send(Ok(output.clone()))
        .unwrap();
    assert_eq!(admission.completion.await.unwrap().unwrap(), output);
    let records = captured.records();
    assert_eq!(records.len(), 2);
    for record in &records {
        assert_eq!(record["fields"]["node"], fixture.target.to_string());
        assert_eq!(record["fields"]["node"].as_str().unwrap().len(), 64);
        assert_eq!(record["fields"]["request"], request_id.to_string());
    }
    let fields = &records[1]["fields"];
    assert_eq!(fields["phase"], "completed");
    assert_eq!(fields["communication_outcome"], "succeeded");
    assert_eq!(fields["execution_outcome"], "succeeded");
    assert_eq!(fields["durable"], true);
    assert!(fields["end_to_end_us"].is_u64());
    assert!(
        fields.get("duration_us").is_none(),
        "Node duration is a separate observation"
    );
    assert!(fields.get("code").is_none());
    assert!(
        !serde_json::to_string(&records)
            .unwrap()
            .contains("PRIVATE_")
    );
    fixture.close().await;
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
    tokio::time::pause();
    tokio::time::advance(Duration::from_secs(11)).await;
    tokio::task::yield_now().await;
    tokio::time::resume();
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
    let captured = Capture::default();
    let admission = fixture
        .remote()
        .dispatch(installation(fixture.target))
        .with_subscriber(captured.dispatch())
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
    let records = captured.records();
    assert_eq!(records.len(), 2);
    let fields = &records[1]["fields"];
    assert_eq!(fields["phase"], "interrupted");
    assert_eq!(fields["execution_outcome"], "unknown");
    assert_eq!(fields["recovery_decision"], "same_request");
    assert_eq!(fields["code"], "outcome_unknown");
    fixture.execution.close().await.unwrap();
}

#[tokio::test]
async fn dropping_completion_does_not_cancel_execution() {
    let fixture = Fixture::new().await;
    let captured = Capture::default();
    let admission = fixture
        .remote()
        .dispatch(installation(fixture.target))
        .with_subscriber(captured.dispatch())
        .await
        .unwrap();
    drop(admission);
    fixture
        .handler
        .completions
        .lock()
        .unwrap()
        .pop()
        .unwrap()
        .send(Ok(Output::SshOutcome(ssh::Outcome::HostInstalled {
            node: fixture.target,
        })))
        .unwrap();
    tokio::time::timeout(Duration::from_secs(2), async {
        while captured.records().len() < 2 {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    let records = captured.records();
    assert_eq!(records.len(), 2);
    assert_eq!(records[1]["fields"]["phase"], "completed");
    assert_eq!(records[1]["fields"]["execution_outcome"], "succeeded");
    fixture.close().await;
}

#[tokio::test]
async fn trust_changes_bypass_completion_capacity() {
    let fixture = Fixture::new().await;
    let directory = fixture._directory.path().join("second-execution");
    std::fs::create_dir(&directory).unwrap();
    let identity = Identity::open(&directory).unwrap();
    let other = identity.id();
    let execution = Link::bind(
        &identity,
        NetworkScope::default(),
        fixture.handler.clone(),
        Arc::new(Peers::default()),
    )
    .await
    .unwrap();
    execution
        .handle()
        .set_trust(fixture.caller, true)
        .await
        .unwrap();
    fixture
        .controller
        .handle()
        .set_trust(other, true)
        .await
        .unwrap();
    let mut completions = Vec::new();
    // The observer quota belongs to the controller. Two peers avoid testing
    // QUIC's batched stream-credit return after each connection's Inspect.
    for (target, address) in [
        (fixture.target, fixture.execution.handle().address()),
        (other, execution.handle().address()),
    ] {
        let remote = fixture.controller.handle().remote(address);
        for _ in 0..32 {
            let admission = remote.dispatch(installation(target)).await.unwrap();
            assert!(admission.receipt.durable);
            completions.push(admission.completion);
        }
    }
    // Every receipt has arrived, but the service has not released one business
    // completion. All 64 existing observer slots are still occupied.
    assert_eq!(fixture.handler.completions.lock().unwrap().len(), 64);
    let gate = Arc::new(TrustGate {
        entered: tokio::sync::Semaphore::new(0),
        release: tokio::sync::Semaphore::new(0),
    });
    *fixture.peers.gate.lock().unwrap() = Some(gate.clone());
    let handle = fixture.controller.handle();
    let target = fixture.target;
    let mut changes = handle.peer_changes();
    let control = tokio::spawn(async move { handle.set_trust(target, false).await });
    notified(&gate.entered).await;
    control.abort();
    assert!(control.await.unwrap_err().is_cancelled());
    gate.release.add_permits(1);
    tokio::time::timeout(Duration::from_secs(2), changes.changed())
        .await
        .unwrap()
        .unwrap();
    assert!(
        !fixture
            .peers
            .values
            .lock()
            .unwrap()
            .contains(&fixture.target)
    );
    *fixture.peers.gate.lock().unwrap() = None;
    fixture
        .controller
        .handle()
        .set_trust(other, false)
        .await
        .unwrap();
    for completion in completions {
        let result = tokio::time::timeout(Duration::from_secs(2), completion)
            .await
            .unwrap()
            .unwrap();
        assert!(matches!(
            result,
            Err(Fault {
                code: ErrorCode::OutcomeUnknown,
                ..
            })
        ));
    }
    assert_eq!(
        fixture.handler.completions.lock().unwrap().len(),
        64,
        "revocation must not replay admitted work"
    );
    execution.close().await.unwrap();
    fixture.close().await;
}
