use super::*;
use serde_json::json;

#[tokio::test]
#[cfg(unix)]
#[ignore = "requires SAILRY_TEST_OFFICE_PYTHON pointing to an external test environment"]
async fn scripts_use_node_environment() {
    let python = std::path::PathBuf::from(
        std::env::var_os("SAILRY_TEST_OFFICE_PYTHON").expect("external test interpreter required"),
    );
    assert!(python.is_absolute());
    assert!(python.is_file());
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
        let command = format!(
            "'{}' verify.py '{}'",
            python.to_str().unwrap().replace('\'', "'\"'\"'"),
            installed.to_str().unwrap().replace('\'', "'\"'\"'")
        );
        let author = Server::turn_tools(vec![
            ("load_skill".into(), json!({"skill":"office:word"})),
            (
                plugin_tool("commands", "run_command"),
                json!({"command":command}),
            ),
        ])
        .await;
        let Output::Providers(mut providers) = client
            .execute(client.prepare(Command::ListProviders))
            .await
            .unwrap()
        else {
            panic!("providers expected")
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
        let Output::QueuedTurn(turn) = client
            .execute(client.prepare(Command::SubmitTurn {
                session: session.id,
                expected_revision: session.revision,
                message: "Create styled Office documents using the project environment".into(),
            }))
            .await
            .unwrap()
        else {
            panic!("turn expected")
        };
        let page = finished(&client, session.id, turn.id).await;
        assert_eq!(page.runs.last().unwrap().status, Status::Completed);
        let requests = author.requests.lock().unwrap().clone();
        let names: Vec<_> = requests[0]["tools"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|tool| tool["function"]["name"].as_str())
            .collect();
        for required in [
            "load_skill",
            plugin_tool("files", "read_office").as_str(),
            plugin_tool("files", "export_pdf").as_str(),
        ] {
            assert!(names.contains(&required));
        }
        assert!(names.contains(&plugin_tool("commands", "run_command").as_str()));
        for removed in [
            "write_office".to_owned(),
            "edit_office".to_owned(),
            plugin_tool("files", "get_office_runtime"),
        ] {
            assert!(!names.contains(&removed.as_str()));
        }
        let results: Vec<_> = page
            .entries
            .iter()
            .flat_map(|e| &e.parts)
            .filter_map(|part| match part {
                Part::ToolResult { result, .. } => Some(result),
                _ => None,
            })
            .collect();
        assert_eq!(results.len(), 2);
        for result in &results {
            assert!(result.get("error").is_none(), "{result}");
        }
        let Output::CommandResult(completion) = serde_json::from_value(results[1].clone()).unwrap()
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
                        path: format!("output/office-check/{path}"),
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
