use super::*;

fn shell() -> tokio::process::Command {
    let mut command = tokio::process::Command::new("/bin/sh");
    command.arg("-s");
    command
}

#[tokio::test]
async fn bounds_output_and_preserves_failures() {
    let stop = CancellationToken::new();
    assert_eq!(
        run(shell(), "printf ready", &stop, Duration::from_secs(5))
            .await
            .unwrap(),
        "ready"
    );
    let error = run(
        shell(),
        "echo 'systemd is unavailable' >&2; exit 1",
        &stop,
        Duration::from_secs(5),
    )
    .await
    .unwrap_err();
    assert_eq!(error.code, ErrorCode::Unavailable);
    assert!(error.message.contains("systemd is unavailable"));
    for script in ["kill -TERM $$", "printf '\\377'"] {
        assert_eq!(
            run(shell(), script, &stop, Duration::from_secs(5))
                .await
                .unwrap_err()
                .code,
            ErrorCode::OutcomeUnknown
        );
    }
    assert_eq!(
        run(shell(), "exit 73", &stop, Duration::from_secs(5))
            .await
            .unwrap_err()
            .code,
        ErrorCode::OutcomeUnknown
    );
    assert_eq!(
        run(
            shell(),
            "head -c 131073 /dev/zero",
            &stop,
            Duration::from_secs(5)
        )
        .await
        .unwrap_err()
        .code,
        ErrorCode::OutcomeUnknown
    );
}

#[tokio::test]
async fn preserves_argument_line_endings() {
    let resource = crate::host::deployment::normalize("set -eu\r\n");
    let argument = "first\r\nsecond";
    let script = format!("{resource}printf '%s' '{argument}'\n");
    assert_eq!(
        run(
            shell(),
            &script,
            &CancellationToken::new(),
            Duration::from_secs(5)
        )
        .await
        .unwrap(),
        argument
    );
}

#[tokio::test]
async fn stops_cancelled_and_timed_out_commands() {
    let stop = CancellationToken::new();
    stop.cancel();
    assert_eq!(
        run(shell(), "exit 0", &stop, Duration::from_secs(5))
            .await
            .unwrap_err()
            .code,
        ErrorCode::Cancelled
    );
    assert_eq!(
        run(
            shell(),
            "sleep 30",
            &CancellationToken::new(),
            Duration::from_millis(100)
        )
        .await
        .unwrap_err()
        .code,
        ErrorCode::OutcomeUnknown
    );
    let stop = CancellationToken::new();
    let cancel = stop.clone();
    let operation = run(shell(), "sleep 30", &stop, Duration::from_secs(5));
    let cancel = async {
        tokio::task::yield_now().await;
        cancel.cancel();
    };
    let (result, ()) = tokio::join!(operation, cancel);
    assert_eq!(result.unwrap_err().code, ErrorCode::Cancelled);
}
