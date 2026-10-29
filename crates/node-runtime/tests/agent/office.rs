use super::*;
use serde_json::json;

#[tokio::test]
#[cfg(unix)]
#[ignore = "requires SAILRY_TEST_PYTHON_RUNTIME and network access for the locked Office wheels"]
async fn scripts_use_node_environment() {
    let runtime = std::path::PathBuf::from(
        std::env::var_os("SAILRY_TEST_PYTHON_RUNTIME").expect("bundled test runtime required"),
    );
    assert!(runtime.is_absolute());
    assert!(runtime.is_dir());
    for remote in [false, true] {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().join("project");
        std::fs::create_dir(&root).unwrap();
        std::fs::write(
            root.join("verify.py"),
            concat!(
                include_str!("office/verify.py"),
                "\n",
                include_str!("office/templates.py"),
            ),
        )
        .unwrap();
        let node = Node::start(directory.path().join("node")).await.unwrap();
        node.set_python_runtime_fixture(&runtime).unwrap();
        let controller =
            Link::controller(directory.path().join("controller"), NetworkScope::default())
                .await
                .unwrap();
        let address = controller
            .handle()
            .pair(node.link().invite().unwrap().ticket())
            .await
            .unwrap();
        let client = Client::new(if remote {
            controller.handle().remote(address)
        } else {
            node.local()
        });
        let server = Server::tools(vec![]).await;
        let session = approvals::prepare(&client, &server, &root).await;
        let Output::Plugin(package) = client
            .execute(client.prepare(Command::ReadPlugin {
                name: "office".into(),
            }))
            .await
            .unwrap()
        else {
            panic!("plugin expected")
        };
        let installed = node
            .profile()
            .join("plugins/packages")
            .join(&package.summary.digest);
        let loader =
            Server::turn_tools(vec![("load_skill".into(), json!({"skill":"office:word"}))]).await;
        let Output::Providers(mut providers) = client
            .execute(client.prepare(Command::ListProviders))
            .await
            .unwrap()
        else {
            panic!("providers expected")
        };
        let mut provider = providers.remove(0);
        let revision = provider.revision;
        provider.endpoint = loader.endpoint.clone();
        client
            .execute(client.prepare(Command::PutProvider {
                provider,
                expected_revision: revision,
            }))
            .await
            .unwrap();
        let mut config = session.config.clone();
        config.permission = Permission::Full;
        let Output::Session(session) = client
            .execute(client.prepare(Command::SetSessionConfig {
                session: session.id,
                expected_revision: session.revision,
                config,
            }))
            .await
            .unwrap()
        else {
            panic!("session expected")
        };
        let Output::QueuedTurn(preparing) = client
            .execute(client.prepare(Command::SubmitTurn {
                session: session.id,
                expected_revision: session.revision,
                message: "Prepare the managed Office environment".into(),
            }))
            .await
            .unwrap()
        else {
            panic!("turn expected");
        };
        // This explicit real-service acceptance includes cold wheel downloads.
        let ready =
            finished_within(&client, session.id, preparing.id, Duration::from_secs(120)).await;
        assert_eq!(ready.runs.last().unwrap().status, Status::Completed);
        let python = results(&client, &ready)
            .await
            .into_iter()
            .find_map(|result| {
                result
                    .get("python")
                    .and_then(|value| value.as_str())
                    .map(str::to_owned)
            })
            .expect("managed Python returned");
        assert!(
            std::path::Path::new(&python).starts_with(node.profile().join("python/environments"))
        );
        let command = format!(
            "\"$SAILRY_PYTHON\" verify.py '{}'",
            installed.to_str().unwrap().replace('\'', "'\"'\"'")
        );
        let mut calls: Vec<_> = ["word", "excel", "powerpoint", "pdf"]
            .into_iter()
            .map(|name| {
                (
                    "load_skill".into(),
                    json!({"skill":format!("office:{name}")}),
                )
            })
            .collect();
        calls.push((
            plugin_tool("commands", "run_command"),
            json!({"access":"workspace", "command":command}),
        ));
        let author = Server::turn_tools(calls).await;
        let Output::Providers(mut providers) = client
            .execute(client.prepare(Command::ListProviders))
            .await
            .unwrap()
        else {
            panic!("providers expected");
        };
        let mut provider = providers.remove(0);
        let revision = provider.revision;
        provider.endpoint = author.endpoint.clone();
        client
            .execute(client.prepare(Command::PutProvider {
                provider,
                expected_revision: revision,
            }))
            .await
            .unwrap();
        let Output::QueuedTurn(turn) = client
            .execute(client.prepare(Command::SubmitTurn {
                session: session.id,
                expected_revision: session.revision,
                message: "Create styled Office documents using managed Python".into(),
            }))
            .await
            .unwrap()
        else {
            panic!("turn expected")
        };
        // This acceptance invokes real authoring libraries and all four installed scripts.
        let page = finished_within(&client, session.id, turn.id, Duration::from_secs(90)).await;
        assert_eq!(page.runs.last().unwrap().status, Status::Completed);
        let requests = author.requests.lock().unwrap().clone();
        let names: Vec<_> = requests[0]["tools"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|tool| tool["function"]["name"].as_str())
            .collect();
        for required in ["load_skill", "run_command"] {
            assert!(names.contains(&required));
        }
        // Office reading stays discoverable through tool search.
        for removed in [
            "write_office".to_owned(),
            "edit_office".to_owned(),
            "read_office".to_owned(),
            "get_office_runtime".to_owned(),
            plugin_tool("files", "get_office_runtime"),
        ] {
            assert!(!names.contains(&removed.as_str()));
        }
        let results: Vec<_> = tool_details(&client, &page)
            .await
            .into_iter()
            .map(|(name, detail)| (name, detail.result.unwrap()))
            .collect();
        assert_eq!(results.len(), 5, "Office tool results: {results:?}");
        for (_, result) in &results {
            assert!(result.get("error").is_none(), "{result}");
        }
        for ((name, result), skill) in
            results[..4]
                .iter()
                .zip(["word", "excel", "powerpoint", "pdf"])
        {
            assert_eq!(name.as_str(), "load_skill");
            assert_eq!(result["skill"], format!("office:{skill}"));
            assert_eq!(result["python"], python);
            assert_eq!(
                result["directory"],
                installed.join("skills").join(skill).to_str().unwrap()
            );
        }
        assert_eq!(results[4].0, plugin_tool("commands", "run_command"));
        let Output::CommandResult(completion) =
            serde_json::from_value(results[4].1.clone()).unwrap()
        else {
            panic!("command result expected")
        };
        assert_eq!(
            completion.outcome,
            sailry_protocol::process::Outcome::Exited(0),
            "{}",
            completion.stderr.text
        );
        assert!(completion.stdout.text.contains("Verified DOCX"));
        assert!(
            completion
                .stdout
                .text
                .contains("Verified installed Office scripts/templates for all four formats")
        );
        for path in [
            "styled.docx",
            "styled.xlsx",
            "cached.xlsx",
            "styled.pptx",
            "filled.pdf",
        ] {
            assert!(root.join("output/office-check").join(path).is_file());
            let Output::OfficeContent(content) = client
                .execute(client.prepare(Command::ReadOffice {
                    worktree: session.worktree,
                    options: sailry_protocol::office::Read {
                        source: sailry_protocol::office::Source::Path(format!(
                            "output/office-check/{path}"
                        )),
                        offset: 0,
                    },
                }))
                .await
                .unwrap()
            else {
                panic!("content expected")
            };
            assert!(!content.sections.is_empty());
        }
        controller.close().await.unwrap();
        node.shutdown().await.unwrap();
    }
}

#[path = "office/lifecycle.rs"]
mod lifecycle;
