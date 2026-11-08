use sailry_protocol::{
    ErrorCode, Output,
    process::{Access, Completion, Outcome, Status},
    tool::Failure,
};
use serde_json::{Value, json};

const NOTICE: &str = "This command ran in the workspace sandbox: network access is disabled, writes are limited to the worktree and temporary directories, and existing Git metadata and managed child worktrees are protected. macOS also blocks new .git paths. Git writes, including git init, git add and git commit, require access=\"unrestricted\" under the current permission policy. If sandbox restrictions caused the failure, inspect any partial changes before requesting unrestricted access to retry. Unrestricted access does not override operating-system permissions.";

const PROMOTED: &str = "The user sent a new message while this command was running. The command was moved to the background and is still running; use the command read and stop tools with this id to check it. Its completion is reported automatically.";

/// Explains a foreground command that moved to the background for guidance.
pub(super) fn promoted(output: &Output, value: &mut Value, foreground: bool) {
    if foreground && matches!(output, Output::CommandStarted(_)) {
        value["data"]["notice"] = json!(PROMOTED);
    }
}

/// Freeze model preview metadata at this tool return, independent of later reads.
pub(super) async fn retain(
    commands: &crate::process::background::Commands,
    session: sailry_protocol::SessionId,
    id: sailry_protocol::RequestId,
    output: &Output,
    value: &mut Value,
) {
    if !matches!(output, Output::CommandResult(_) | Output::CommandOutput(_)) {
        return;
    }
    let Ok(retained) = commands.retained(session, id) else {
        return;
    };
    let background = match output {
        Output::CommandOutput(snapshot) => Some(snapshot.info.clone()),
        _ => None,
    };
    let projected = tokio::task::spawn_blocking(move || {
        let mut preview = retained.preview(id).unwrap_or_else(|| json!({"id":id}));
        // Background captures contain the recent tail; only these need a small
        // explicit head. Foreground captures already own the original prefix.
        if let Some(info) = background {
            for (name, stream) in [
                ("stdout", sailry_protocol::process::OutputStream::Stdout),
                ("stderr", sailry_protocol::process::OutputStream::Stderr),
            ] {
                if preview[name].is_object()
                    && let Ok(head) = retained.page(info.clone(), stream, 0, 8 * 1024)
                {
                    if !head.invalid_utf8 && preview[name]["transcoded"] != true {
                        preview[name]["head_bytes"] = json!(head.text.len());
                    }
                    preview[name]["head_invalid_utf8"] = json!(head.invalid_utf8);
                    preview[name]["head"] = json!(head.text);
                }
            }
        }
        preview
    })
    .await;
    value["sailry_command"] = projected.unwrap_or_else(|_| json!({"id":id}));
}

pub(super) fn failure(output: &Output) -> Option<crate::agent::observation::Failure> {
    let outcome = match output {
        Output::CommandResult(completion) => &completion.outcome,
        Output::CommandOutput(snapshot) => match &snapshot.info.status {
            Status::Finished(outcome) => outcome,
            Status::Failed(message) => {
                return Some(crate::agent::observation::Failure {
                    failure: Failure::runtime(None),
                    message: Some(message.clone()),
                });
            }
            Status::Running | Status::Stopping => return None,
        },
        _ => return None,
    };
    let (failure, message) = match outcome {
        Outcome::Exited(0) => return None,
        Outcome::Exited(code) => (
            Failure::tool(None),
            Some(format!("Command exited with code {code}")),
        ),
        Outcome::Signal(signal) => (
            Failure::runtime(None),
            Some(format!("Command terminated by signal {signal}")),
        ),
        Outcome::TimedOut => (Failure::runtime(None), Some("Command timed out".into())),
        Outcome::Cancelled => (Failure::runtime(Some(ErrorCode::Cancelled)), None),
        Outcome::Unknown(_) => (Failure::runtime(Some(ErrorCode::OutcomeUnknown)), None),
    };
    Some(crate::agent::observation::Failure { failure, message })
}

pub(super) fn annotate(output: &Output, value: &mut Value, workspace: bool) {
    let (failed, stdout, stderr) = match output {
        Output::CommandResult(Completion {
            outcome,
            stdout,
            stderr,
            ..
        }) if workspace => (outcome != &Outcome::Exited(0), stdout, stderr),
        Output::CommandOutput(snapshot) if snapshot.info.access == Access::Workspace => (
            matches!(
                &snapshot.info.status,
                Status::Finished(outcome) if outcome != &Outcome::Exited(0)
            ) || matches!(&snapshot.info.status, Status::Failed(_)),
            &snapshot.stdout,
            &snapshot.stderr,
        ),
        _ => return,
    };
    if failed && (restricted(&stdout.text) || restricted(&stderr.text)) {
        value["data"]["sandbox_notice"] = json!(NOTICE);
    }
}

fn restricted(text: &str) -> bool {
    let text = text.to_ascii_lowercase();
    [
        "operation not permitted",
        "permission denied",
        "sandbox",
        "dyld:",
        "dyld[",
        "could not resolve host",
        "network is unreachable",
    ]
    .iter()
    .any(|pattern| text.contains(pattern))
}

#[cfg(test)]
mod tests {
    use super::*;
    use sailry_protocol::process::Capture;

    fn output(code: i32, text: &str) -> Output {
        Output::CommandResult(Completion {
            outcome: Outcome::Exited(code),
            stdout: Capture::default(),
            stderr: Capture {
                text: text.into(),
                ..Default::default()
            },
            elapsed_ms: 1,
        })
    }

    #[tokio::test]
    async fn persists_only_bounded_command_preview() {
        use sailry_protocol::{RequestId, SessionId, TurnId, process::OutputStream};
        let profile = tempfile::tempdir().unwrap();
        let commands = crate::process::background::Commands::default();
        let session = SessionId::new();
        let id = RequestId::new();
        let retained = commands.retain(
            id,
            session,
            TurnId::new(),
            &crate::process::Launch {
                profile: Some(profile.path().to_owned()),
                root: profile.path().to_owned(),
                command: "cargo test".into(),
                access: Access::Workspace,
                pty: false,
                cwd: String::new(),
                timeout_ms: 1_000,
                inputs: None,
                scratch: None,
                python: None,
            },
        );
        let raw = format!(
            "{}failures:\n{}last failure\n",
            "test passed ... ok\n".repeat(100),
            "original failure output\n".repeat(50_000)
        );
        retained.append(OutputStream::Stderr, raw.as_bytes());
        let output = output(1, "canonical capture");
        let mut value = serde_json::to_value(&output).unwrap();
        retain(&commands, session, id, &output, &mut value).await;
        let metadata = &value["sailry_command"];
        assert!(metadata.to_string().len() < 36 * 1024);
        let preview = &metadata["stderr"];
        assert!(preview.get("head").is_none());
        assert!(
            preview["tail"]
                .as_str()
                .unwrap()
                .ends_with("last failure\n")
        );
        assert!(!preview.to_string().contains("Omitted 100 passed tests"));
        assert_eq!(preview["total_bytes"], raw.len());
        assert_eq!(value["data"]["stderr"]["text"], "canonical capture");
    }

    #[test]
    fn explains_restricted_failures_without_changing_output() {
        for text in [
            "git: Operation not permitted",
            "dyld[123]: Library not loaded",
            "curl: Could not resolve host",
        ] {
            let output = output(1, text);
            let mut value = serde_json::to_value(&output).unwrap();
            annotate(&output, &mut value, true);
            assert_eq!(value["data"]["sandbox_notice"], NOTICE);
            assert!(NOTICE.contains("Git writes, including git init, git add and git commit"));
            assert!(NOTICE.contains("macOS also blocks new .git paths"));
            assert_eq!(value["data"]["stderr"]["text"], text);
            assert_eq!(serde_json::from_value::<Output>(value).unwrap(), output);
        }
    }

    #[test]
    fn excludes_success_and_unrelated_failures() {
        for (code, text, workspace) in [
            (0, "sandbox documentation", true),
            (1, "unknown repository", true),
            (1, "Operation not permitted", false),
        ] {
            let output = output(code, text);
            let mut value = serde_json::to_value(&output).unwrap();
            let original = value.clone();
            annotate(&output, &mut value, workspace);
            assert_eq!(value, original);
        }
    }

    #[test]
    fn distinguishes_operational_outcomes() {
        for (outcome, expected, message) in [
            (Outcome::Exited(0), None, None),
            (
                Outcome::Exited(7),
                Some(Failure::tool(None)),
                Some("Command exited with code 7"),
            ),
            (
                Outcome::Signal(9),
                Some(Failure::runtime(None)),
                Some("Command terminated by signal 9"),
            ),
            (
                Outcome::TimedOut,
                Some(Failure::runtime(None)),
                Some("Command timed out"),
            ),
            (
                Outcome::Cancelled,
                Some(Failure::runtime(Some(ErrorCode::Cancelled))),
                None,
            ),
            (
                Outcome::Unknown("cleanup is unconfirmed".into()),
                Some(Failure::runtime(Some(ErrorCode::OutcomeUnknown))),
                None,
            ),
        ] {
            let output = Output::CommandResult(Completion {
                outcome,
                stdout: Capture::default(),
                stderr: Capture::default(),
                elapsed_ms: 1,
            });
            let original = output.clone();
            let observed = failure(&output);
            assert_eq!(
                observed.as_ref().map(|value| &value.failure),
                expected.as_ref()
            );
            assert_eq!(
                observed.as_ref().and_then(|value| value.message.as_deref()),
                message
            );
            assert_eq!(output, original);
        }
    }

    #[test]
    fn distinguishes_background_service_failures() {
        use sailry_protocol::{
            RequestId, SessionId, TurnId,
            process::{Info, Snapshot},
        };
        for (status, expected, message) in [
            (Status::Running, None, None),
            (Status::Stopping, None, None),
            (
                Status::Finished(Outcome::Exited(7)),
                Some(Failure::tool(None)),
                Some("Command exited with code 7"),
            ),
            (
                Status::Failed("process service failed".into()),
                Some(Failure::runtime(None)),
                Some("process service failed"),
            ),
        ] {
            let output = Output::CommandOutput(Snapshot {
                info: Info {
                    id: RequestId::new(),
                    session: SessionId::new(),
                    turn: TurnId::new(),
                    command: "fixture".into(),
                    access: Access::Unrestricted,
                    pty: false,
                    cwd: String::new(),
                    started_ms: 0,
                    status,
                    services: vec![],
                },
                stdout: Capture::default(),
                stderr: Capture::default(),
            });
            let original = output.clone();
            let observed = failure(&output);
            assert_eq!(
                observed.as_ref().map(|value| &value.failure),
                expected.as_ref()
            );
            assert_eq!(
                observed.as_ref().and_then(|value| value.message.as_deref()),
                message
            );
            assert_eq!(output, original);
        }
    }
}
