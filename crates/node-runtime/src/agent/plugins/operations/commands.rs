use sailry_protocol::{
    ErrorCode, Output,
    process::{Access, Completion, Outcome, Status},
    tool::Failure,
};
use serde_json::{Value, json};

const NOTICE: &str = "This command ran in the workspace sandbox: network access is disabled, writes are limited to the worktree and temporary directories, and existing Git metadata and managed child worktrees are protected. macOS also blocks new .git paths. Git writes, including git init, git add and git commit, require access=\"unrestricted\" under the current permission policy. If sandbox restrictions caused the failure, inspect any partial changes before requesting unrestricted access to retry. Unrestricted access does not override operating-system permissions.";

pub(super) fn failure(output: &Output) -> Option<Failure> {
    let outcome = match output {
        Output::CommandResult(completion) => &completion.outcome,
        Output::CommandOutput(snapshot) => match &snapshot.info.status {
            Status::Finished(outcome) => outcome,
            Status::Failed(_) => return Some(Failure { code: None }),
            Status::Running | Status::Stopping => return None,
        },
        _ => return None,
    };
    match outcome {
        Outcome::Exited(_) => None,
        Outcome::Signal(_) | Outcome::TimedOut => Some(Failure { code: None }),
        Outcome::Cancelled => Some(Failure {
            code: Some(ErrorCode::Cancelled),
        }),
        Outcome::Unknown(_) => Some(Failure {
            code: Some(ErrorCode::OutcomeUnknown),
        }),
    }
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
        for (outcome, expected) in [
            (Outcome::Exited(0), None),
            (Outcome::Exited(7), None),
            (Outcome::Signal(9), Some(Failure { code: None })),
            (Outcome::TimedOut, Some(Failure { code: None })),
            (
                Outcome::Cancelled,
                Some(Failure {
                    code: Some(ErrorCode::Cancelled),
                }),
            ),
            (
                Outcome::Unknown("cleanup is unconfirmed".into()),
                Some(Failure {
                    code: Some(ErrorCode::OutcomeUnknown),
                }),
            ),
        ] {
            let output = Output::CommandResult(Completion {
                outcome,
                stdout: Capture::default(),
                stderr: Capture::default(),
                elapsed_ms: 1,
            });
            let original = output.clone();
            assert_eq!(failure(&output), expected);
            assert_eq!(output, original);
        }
    }
}
