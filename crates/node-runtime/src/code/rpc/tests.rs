#![cfg(unix)]
use super::*;
use crate::code::tests::Fixture;
#[tokio::test]
async fn preserves_initial_exit_reason() {
    let fixture = Fixture::new().await;
    fixture.settings(json!({"initial_crash":true}));
    let plan = fixture.plan();
    let rpc = Rpc::spawn(&plan, &fixture.environment, CancellationToken::new()).unwrap();
    let error = rpc
        .request(
            "initialize",
            json!({"rootUri":url::Url::from_directory_path(&fixture.root).unwrap()}),
            plan.timeout,
            &CancellationToken::new(),
        )
        .await
        .unwrap_err();
    assert_eq!(error.code, ErrorCode::Unavailable);
    assert!(
        error.message.starts_with("language server exited:")
            || error.message == "language server closed its output",
        "{error:?}"
    );
    assert_eq!(
        rpc.state.lock().await.failure.as_deref(),
        Some(error.message.as_str())
    );
    let stopped = rpc
        .request(
            "initialize",
            json!({}),
            plan.timeout,
            &CancellationToken::new(),
        )
        .await
        .unwrap_err();
    assert_eq!(stopped, error);
    rpc.shutdown().await;
}

#[tokio::test]
async fn cancels_requests_and_bounds_stderr() {
    let fixture = Fixture::new().await;
    fixture.settings(json!({"hang":true,"stderr":70000}));
    let plan = fixture.plan();
    let rpc = Rpc::spawn(&plan, &fixture.environment, CancellationToken::new()).unwrap();
    rpc.request(
        "initialize",
        json!({"rootUri":url::Url::from_directory_path(&fixture.root).unwrap()}),
        plan.timeout,
        &CancellationToken::new(),
    )
    .await
    .unwrap();
    let stop = CancellationToken::new();
    let request = rpc.request(
        "textDocument/hover",
        json!({}),
        Duration::from_secs(20),
        &stop,
    );
    tokio::pin!(request);
    tokio::select! {_ = tokio::time::sleep(Duration::from_millis(30))=>stop.cancel(),value=&mut request=>panic!("unexpected response {value:?}")}
    assert_eq!(request.await.unwrap_err().code, ErrorCode::Cancelled);
    tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            if fixture
                .messages()
                .iter()
                .any(|message| message["method"] == "$/cancelRequest")
            {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    assert!(rpc.state.lock().await.stderr.len() <= 64 * 1024);
    assert!(!rpc.state.lock().await.stderr.is_empty());
    rpc.shutdown().await;
}
#[tokio::test]
async fn times_out_without_retaining_a_pending_request() {
    let fixture = Fixture::new().await;
    fixture.settings(json!({"hang":true}));
    let plan = fixture.plan();
    let rpc = Rpc::spawn(&plan, &fixture.environment, CancellationToken::new()).unwrap();
    rpc.request(
        "initialize",
        json!({"rootUri":url::Url::from_directory_path(&fixture.root).unwrap()}),
        plan.timeout,
        &CancellationToken::new(),
    )
    .await
    .unwrap();
    assert_eq!(
        rpc.request(
            "textDocument/hover",
            json!({}),
            Duration::from_millis(30),
            &CancellationToken::new()
        )
        .await
        .unwrap_err()
        .code,
        ErrorCode::Unavailable
    );
    fixture.settings(json!({}));
    assert!(
        rpc.request(
            "textDocument/hover",
            json!({}),
            Duration::from_secs(2),
            &CancellationToken::new()
        )
        .await
        .is_ok()
    );
    rpc.shutdown().await;
}

#[tokio::test]
async fn kills_descendants_when_shutdown_stalls() {
    let fixture = Fixture::new().await;
    fixture.settings(json!({"child":true,"hang_shutdown":true}));
    let plan = fixture.plan();
    let rpc = Rpc::spawn(&plan, &fixture.environment, CancellationToken::new()).unwrap();
    rpc.request(
        "initialize",
        json!({"rootUri":url::Url::from_directory_path(&fixture.root).unwrap()}),
        plan.timeout,
        &CancellationToken::new(),
    )
    .await
    .unwrap();
    let pid = std::fs::read_to_string(fixture.root.join("child_pid")).unwrap();
    assert!(running(&pid));
    tokio::time::timeout(Duration::from_secs(4), rpc.shutdown())
        .await
        .unwrap();
    tokio::time::timeout(Duration::from_secs(2), async {
        while running(&pid) {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    assert_eq!(rpc.state.lock().await.status, Status::Stopped);
}

fn running(pid: &str) -> bool {
    let output = std::process::Command::new("ps")
        .args(["-p", pid, "-o", "stat="])
        .output()
        .unwrap();
    let state = String::from_utf8(output.stdout).unwrap();
    let state = state.trim();
    !state.is_empty() && !state.starts_with('Z')
}

#[tokio::test]
async fn refuses_relocated_dependency_before_spawning() {
    let fixture = Fixture::new().await;
    let dependency = fixture
        ._directory
        .path()
        .canonicalize()
        .unwrap()
        .join("package");
    std::fs::create_dir(&dependency).unwrap();
    let mut plan = fixture.plan();
    plan.dependency = dependency.clone();
    std::fs::remove_dir(&dependency).unwrap();
    std::os::unix::fs::symlink(fixture._directory.path(), &dependency).unwrap();
    let error = match Rpc::spawn(&plan, &fixture.environment, CancellationToken::new()) {
        Ok(_) => panic!("relocated dependency must fail before process creation"),
        Err(error) => error,
    };
    assert_eq!(error.code, ErrorCode::RevisionConflict);
    assert!(!fixture.root.join("pid").exists());
}
