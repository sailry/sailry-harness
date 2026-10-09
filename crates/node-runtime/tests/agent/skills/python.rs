use super::*;

#[tokio::test]
#[cfg(unix)]
#[ignore = "requires SAILRY_TEST_PYTHON_RUNTIME pointing at verified bundled Python"]
async fn runs_in_the_managed_session_scope() {
    let runtime = std::path::PathBuf::from(
        std::env::var_os("SAILRY_TEST_PYTHON_RUNTIME")
            .expect("SAILRY_TEST_PYTHON_RUNTIME is required"),
    );
    for remote in [false, true] {
        let idle = Server::start(false).await;
        let (directory, node, controller, client, session) = fixture(remote, &idle).await;
        node.set_python_runtime_fixture(&runtime).unwrap();
        let Output::Plugin(installed) = execute(
            &client,
            Command::ReadPlugin {
                name: "example".into(),
            },
        )
        .await
        else {
            panic!("plugin expected");
        };
        let captured = node
            .profile()
            .join("plugins/packages")
            .join(&installed.summary.digest)
            .join("skills/analysis");
        // Package resources stay immutable. The script is authored in the task
        // worktree and receives only the loaded skill's exact read permission.
        let root = directory.path().join("project");
        let script = root.join("verify.py");
        fs::write(&script, "import json, os, pathlib, sys\nassert sys.version_info[:3] == (3,12,13)\nsession = pathlib.Path(os.environ['SAILRY_SESSION_DIR'])\nsession.joinpath('result.json').write_text(json.dumps({'python':sys.executable,'path':os.environ.get('PATH'),'index':os.environ.get('PIP_INDEX_URL'),'session':str(session),'version':list(sys.version_info[:3])}))\nprint(session)\n").unwrap();
        let author = Server::turn_tools(vec![
            ("load_skill".into(), json!({"skill":"example:analysis"})),
            (
                plugin_tool("commands", "run_command"),
                json!({"command":"\"$SAILRY_PYTHON\" -I verify.py", "access":"workspace"}),
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
                message: "Run the script with bundled Python in its session directory".into(),
            },
        )
        .await
        else {
            panic!("turn expected");
        };
        let page = finished(&client, session.id, turn.id).await;
        assert_eq!(page.runs[0].status, Status::Completed);
        let loaded = results(&page);
        assert_eq!(loaded[0]["directory"], captured.to_str().unwrap());
        let python = loaded[0]["python"]
            .as_str()
            .expect("managed interpreter returned");
        assert!(std::path::Path::new(python).starts_with(runtime.canonicalize().unwrap()));
        let commands = process::results(&page);
        assert_eq!(commands.len(), 1);
        assert_eq!(
            commands[0].outcome,
            sailry_protocol::process::Outcome::Exited(0),
            "{}",
            commands[0].stderr.text
        );
        let session_directory = node
            .profile()
            .join("workspaces/sessions")
            .join(session.id.to_string());
        assert_eq!(
            commands[0].stdout.text.trim(),
            session_directory.to_str().unwrap()
        );
        let record: Value =
            serde_json::from_slice(&fs::read(session_directory.join("result.json")).unwrap())
                .unwrap();
        assert_eq!(record["version"], json!([3, 12, 13]));
        assert_eq!(record["python"], python);
        let path = record["path"].as_str().unwrap();
        assert!(!std::env::split_paths(path).any(|entry| entry.starts_with(&runtime)));

        let Output::PythonSettings(mut settings) =
            execute(&client, Command::ReadPythonSettings).await
        else {
            panic!("Python settings expected");
        };
        settings.index = Some("https://pypi.org/simple/".into());
        let previous = client.prepare(Command::SavePythonSettings(settings));
        let saved = client.execute(previous.clone()).await.unwrap();
        let Output::PythonSettings(mut settings) = saved.clone() else {
            panic!("Python settings expected");
        };
        let index = "https://pypi.tuna.tsinghua.edu.cn/simple/";
        settings.index = Some(index.into());
        let current = execute(&client, Command::SavePythonSettings(settings)).await;
        assert_eq!(client.execute(previous).await.unwrap(), saved);
        assert_eq!(execute(&client, Command::ReadPythonSettings).await, current);
        let Output::QueuedTurn(next) = execute(
            &client,
            Command::SubmitTurn {
                session: session.id,
                expected_revision: session.revision,
                message: "Read the current managed Python command environment".into(),
            },
        )
        .await
        else {
            panic!("turn expected");
        };
        let page = finished(&client, session.id, next.id).await;
        assert_eq!(page.runs.last().unwrap().status, Status::Completed);
        let commands = process::results(&page);
        assert_eq!(commands.len(), 2);
        assert_eq!(
            commands[1].outcome,
            sailry_protocol::process::Outcome::Exited(0),
            "{}",
            commands[1].stderr.text
        );
        let record: Value =
            serde_json::from_slice(&fs::read(session_directory.join("result.json")).unwrap())
                .unwrap();
        assert_eq!(record["index"], index);
        assert_eq!(record["python"], python);
        assert_eq!(record["version"], json!([3, 12, 13]));
        let request = client.prepare(Command::RemoveSession {
            session: session.id,
            expected_revision: session.revision,
        });
        let Output::SessionsRemoved(removed) = client.execute(request.clone()).await.unwrap()
        else {
            panic!("session removal expected");
        };
        assert!(removed.contains(&session.id));
        assert!(!session_directory.exists());
        assert!(script.is_file());
        assert!(captured.join("SKILL.md").is_file());
        assert_eq!(
            client.execute(request).await.unwrap(),
            Output::SessionsRemoved(removed)
        );
        drop(client);
        controller.close().await.unwrap();
        node.shutdown().await.unwrap();
    }
}
