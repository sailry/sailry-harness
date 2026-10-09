use sailry_protocol::{
    Output,
    process::{Access, Completion, Outcome, Status},
};
use serde_json::{Value, json};

const NOTICE: &str = "This command ran in the workspace sandbox: network access is disabled and writes are limited to the worktree and temporary directories. If these restrictions caused the failure, retry with access=\"unrestricted\" under the current permission policy.";

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
}
