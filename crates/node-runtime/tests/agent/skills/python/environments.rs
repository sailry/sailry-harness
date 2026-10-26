use super::*;
use std::path::PathBuf;

const IMPORTS: &str = "import encodings, json, pathlib, sqlite3, ssl, sys, zlib; assert sqlite3.connect(':memory:').execute('select 1').fetchone() == (1,); print(json.dumps({'environment':sys.prefix,'base':sys.base_prefix,'stdlib':encodings.__file__,'version':list(sys.version_info[:3])}))";

const VERIFY: &str = r#"
import encodings
import errno
import json
import os
from pathlib import Path
import sqlite3
import ssl
import sys
import zlib

environment = Path(sys.prefix).resolve()
base = Path(sys.base_prefix).resolve()
stdlib = Path(encodings.__file__).resolve()
assert environment != base
assert stdlib.is_relative_to(base)
assert sys.version_info[:3] == (3, 12, 13)
assert sqlite3.connect(':memory:').execute('select 1').fetchone() == (1,)
for path in [stdlib, environment / 'pyvenv.cfg']:
    try:
        descriptor = os.open(path, os.O_WRONLY)
    except OSError as error:
        assert error.errno in (errno.EACCES, errno.EPERM, errno.EROFS), error
    else:
        os.close(descriptor)
        raise AssertionError(f'managed Python must be read-only: {path}')
print(json.dumps({'environment':str(environment),'base':str(base),'stdlib':str(stdlib),'readonly':2}))
"#;

fn home(configuration: &str) -> &str {
    configuration
        .lines()
        .find_map(|line| line.strip_prefix("home = "))
        .expect("venv base home expected")
}

async fn run(stale: bool) {
    let runtime = PathBuf::from(
        std::env::var_os("SAILRY_TEST_PYTHON_RUNTIME")
            .expect("SAILRY_TEST_PYTHON_RUNTIME is required"),
    )
    .canonicalize()
    .unwrap();
    for remote in [false, true] {
        let idle = Server::start(false).await;
        let (directory, node, controller, client, session) = fixture(remote, &idle).await;
        node.set_python_runtime_fixture(&runtime).unwrap();
        let root = directory.path().join("project");
        // An existing empty requirements file selects a real managed venv;
        // a skill without that file uses the bundled base interpreter instead.
        fs::write(root.join("package/requirements.txt"), "").unwrap();
        let installed = install(&client, session.worktree, 1).await;
        let environments: Vec<_> = fs::read_dir(node.profile().join("python/environments"))
            .unwrap()
            .map(|entry| entry.unwrap().path().canonicalize().unwrap())
            .collect();
        assert_eq!(environments.len(), 1);
        let environment = &environments[0];
        let python = environment.join("bin/python");
        assert!(environment.join(".sailry-ready.json").is_file());
        assert_eq!(
            fs::read_to_string(environment.join(".sailry-requirements.txt")).unwrap(),
            ""
        );
        let configuration = environment.join("pyvenv.cfg");
        let original = fs::read_to_string(&configuration).unwrap();
        assert_eq!(Path::new(home(&original)), runtime.join("bin"));
        let output = std::process::Command::new(&python)
            .args(["-I", "-B", "-c", IMPORTS])
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let record: Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(record["environment"], environment.to_str().unwrap());
        assert_eq!(record["base"], runtime.to_str().unwrap());
        assert_eq!(record["version"], json!([3, 12, 13]));
        if stale {
            let missing = directory.path().join("missing-runtime/bin");
            let changed = original
                .lines()
                .map(|line| {
                    if line.starts_with("home = ") {
                        format!("home = {}", missing.display())
                    } else {
                        line.to_owned()
                    }
                })
                .collect::<Vec<_>>()
                .join("\n");
            // Change ordinary environment data without altering its seal. The
            // manager must repair it before binding, rather than granting the home.
            fs::write(&configuration, changed).unwrap();
            let output = std::process::Command::new(&python)
                .args(["-I", "-B", "-c", IMPORTS])
                .output()
                .unwrap();
            assert!(!output.status.success());
            assert!(
                String::from_utf8_lossy(&output.stderr)
                    .contains("Could not find platform independent libraries <prefix>")
            );
        }
        fs::write(root.join("verify.py"), VERIFY).unwrap();
        let command = format!("\"{}\" -I -B verify.py", python.display());
        let author = Server::turn_tools(vec![
            ("load_skill".into(), json!({"skill":"example:analysis"})),
            (
                plugin_tool("commands", "run_command"),
                json!({"command":command, "access":"workspace"}),
            ),
        ])
        .await;
        let Output::Snapshot(snapshot) = execute(&client, Command::Snapshot).await else {
            panic!("snapshot expected");
        };
        let mut provider = snapshot.providers[0].clone();
        let revision = provider.revision;
        provider.endpoint = author.endpoint.clone();
        execute(
            &client,
            Command::PutProvider {
                provider,
                expected_revision: revision,
            },
        )
        .await;
        let Output::QueuedTurn(turn) = execute(
            &client,
            Command::SubmitTurn {
                session: session.id,
                expected_revision: session.revision,
                message: "Run standard library imports with the loaded skill interpreter".into(),
            },
        )
        .await
        else {
            panic!("turn expected");
        };
        let page = finished(&client, session.id, turn.id).await;
        assert_eq!(page.runs[0].status, Status::Completed, "{:?}", page.runs);
        let loaded = results(&client, &page).await;
        assert_eq!(loaded[0]["python"], python.to_str().unwrap());
        assert_eq!(
            turn.plugins
                .iter()
                .find(|package| package.name == "example")
                .expect("captured skill package expected")
                .digest,
            installed.summary.digest,
        );
        let commands = process::results(&client, &page).await;
        assert_eq!(commands.len(), 1);
        assert_eq!(
            commands[0].outcome,
            sailry_protocol::process::Outcome::Exited(0),
            "{}",
            commands[0].stderr.text
        );
        assert!(
            commands[0].stderr.text.is_empty(),
            "{}",
            commands[0].stderr.text
        );
        let record: Value = serde_json::from_str(&commands[0].stdout.text).unwrap();
        let calls = tool_calls(&client, &page).await;
        let (_, detail) = calls
            .iter()
            .find(|(name, _)| *name == plugin_tool("commands", "run_command"))
            .unwrap();
        assert_eq!(detail.arguments["command"], command);
        let input = &detail.resolved.as_ref().unwrap().input;
        let summary = &input.summary.as_ref().unwrap().text;
        assert!(summary.contains("python"));
        assert!(!summary.contains(python.to_str().unwrap()));
        assert_eq!(input.content.as_ref().unwrap().text, command);
        assert_eq!(record["environment"], environment.to_str().unwrap());
        assert_eq!(record["base"], runtime.to_str().unwrap());
        assert_eq!(record["readonly"], 2);
        assert!(Path::new(record["stdlib"].as_str().unwrap()).starts_with(&runtime));
        assert_eq!(
            Path::new(home(&fs::read_to_string(&configuration).unwrap())),
            runtime.join("bin")
        );
        drop(client);
        controller.close().await.unwrap();
        node.shutdown().await.unwrap();
    }
}

#[tokio::test]
#[ignore = "requires SAILRY_TEST_PYTHON_RUNTIME pointing at verified bundled Python"]
async fn imports_from_managed_environment() {
    run(false).await;
}

#[tokio::test]
#[ignore = "requires SAILRY_TEST_PYTHON_RUNTIME pointing at verified bundled Python"]
async fn repairs_stale_home_before_binding() {
    run(true).await;
}
