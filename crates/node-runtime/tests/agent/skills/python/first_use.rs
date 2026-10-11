use super::*;
use sailry_protocol::tool::{Alias, AliasKind, InputFormat};
use std::path::PathBuf;

fn assert_alias(page: &Page, name: &str, command: &str, python: &Path) {
    let (arguments, display) = page
        .entries
        .iter()
        .flat_map(|entry| &entry.parts)
        .find_map(|part| {
            if let Part::ToolCall {
                name: called,
                arguments,
                display,
                ..
            } = part
                && called == name
            {
                Some((arguments, display.as_ref().unwrap()))
            } else {
                None
            }
        })
        .expect("command call expected");
    assert_eq!(arguments["command"], command);
    let input = display.input.as_ref().unwrap();
    assert_eq!(
        input.summary.as_ref().unwrap().format,
        Some(InputFormat::Command)
    );
    assert_eq!(
        input.content.as_ref().unwrap().read(arguments),
        Some(command)
    );
    assert!(display.aliases.contains(&Alias {
        path: python.to_str().unwrap().into(),
        kind: AliasKind::Python,
    }));
}

#[tokio::test]
#[ignore = "requires SAILRY_TEST_PYTHON_RUNTIME pointing at verified bundled Python"]
async fn captures_bundled_alias_before_execution() {
    let runtime = PathBuf::from(
        std::env::var_os("SAILRY_TEST_PYTHON_RUNTIME")
            .expect("SAILRY_TEST_PYTHON_RUNTIME is required"),
    )
    .canonicalize()
    .unwrap();
    let python = runtime.join("bin/python3.12").canonicalize().unwrap();
    let name = plugin_tool("commands", "run_command");
    let command = format!(
        "'{}' -I -B -c 'from pathlib import Path; Path(\"executed.txt\").write_text(\"executed\"); print(\"base-ready\")'",
        python.to_str().unwrap().replace('\'', "'\\''")
    );
    for remote in [false, true] {
        let server = Server::turn_tools(vec![(
            name.clone(),
            json!({"command":command, "access":"unrestricted"}),
        )])
        .await;
        let fixture = process::Fixture::new(remote, &server).await;
        enabled_only(&fixture.client, &["commands"]).await;
        fixture.node.set_python_runtime_fixture(&runtime).unwrap();
        assert!(!fixture.node.profile().join("python").exists());
        let turn = approvals::submit(&fixture.client, fixture.session.id).await;
        let (pending, approval) = approvals::pending(&fixture.client, fixture.session.id).await;
        assert_eq!(approval.source, ApprovalSource::User);
        assert_eq!(approval.state, ApprovalState::Pending);
        assert_alias(&pending, &name, &command, &python);
        assert!(!fixture.root.join("executed.txt").exists());
        assert!(!fixture.node.profile().join("python").exists());
        assert!(process::results(&pending).is_empty());
        process::decide(&fixture.client, &approval, Decision::Approve).await;
        let page = finished(&fixture.client, fixture.session.id, turn).await;
        assert_eq!(page.runs.last().unwrap().status, Status::Completed);
        assert_alias(&page, &name, &command, &python);
        let results = process::results(&page);
        assert_eq!(results.len(), 1);
        assert_eq!(
            results[0].outcome,
            sailry_protocol::process::Outcome::Exited(0)
        );
        assert_eq!(results[0].stdout.text, "base-ready\n");
        assert_eq!(
            fs::read_to_string(fixture.root.join("executed.txt")).unwrap(),
            "executed"
        );
        drop(fixture.client);
        fixture.controller.close().await.unwrap();
        fixture.node.shutdown().await.unwrap();
    }
}
