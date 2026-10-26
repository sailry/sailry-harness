#![cfg(unix)]
use super::*;
use git2::{Repository, Signature};
use serde_json::json;

#[tokio::test]
async fn explores_history_without_shell_approval() {
    for remote in [false, true] {
        for mode in [WorkMode::Code, WorkMode::Plan] {
            let show = plugin_tool("git", "git_show");
            let blame = plugin_tool("git", "git_blame");
            let server = Server::tools(vec![
                (
                    show.clone(),
                    json!({"path":"-quoted [资料].txt","offset":1,"limit":1}),
                ),
                (
                    blame.clone(),
                    json!({"path":"-quoted [资料].txt","offset":0,"limit":2}),
                ),
            ])
            .await;
            let mut fixture = process::Fixture::packages(remote, &server, &["git"]).await;
            let Output::Plugin(info) = fixture
                .client
                .execute(
                    fixture
                        .client
                        .prepare(Command::ReadPlugin { name: "git".into() }),
                )
                .await
                .unwrap()
            else {
                panic!("Git plugin expected")
            };
            let Output::Plugin(info) = fixture
                .client
                .execute(fixture.client.prepare(Command::InstallBundledPlugin {
                    name: "git".into(),
                    expected_revision: info.summary.revision,
                }))
                .await
                .unwrap()
            else {
                panic!("installed Git plugin expected")
            };
            assert!(info.issues.is_empty(), "{:?}", info.issues);
            if !info.summary.enabled {
                fixture
                    .client
                    .execute(fixture.client.prepare(Command::SetPluginEnabled {
                        name: "git".into(),
                        expected_revision: info.summary.revision,
                        enabled: true,
                    }))
                    .await
                    .unwrap();
            }
            let repository = Repository::init(&fixture.root).unwrap();
            std::fs::write(
                fixture.root.join("-quoted [资料].txt"),
                "First\nSecond\nThird\n",
            )
            .unwrap();
            let mut index = repository.index().unwrap();
            index
                .add_path(std::path::Path::new("-quoted [资料].txt"))
                .unwrap();
            index.write().unwrap();
            let tree = repository.find_tree(index.write_tree().unwrap()).unwrap();
            let author = Signature::now("Fixture", "fixture@example.invalid").unwrap();
            let commit = repository
                .commit(Some("HEAD"), &author, &author, "Fixture", &tree, &[])
                .unwrap();
            std::fs::write(
                fixture.root.join("-quoted [资料].txt"),
                "Uncommitted content\n",
            )
            .unwrap();
            let before = std::fs::read(repository.path().join("index")).unwrap();
            let mut config = fixture.session.config.clone();
            config.permission = Permission::Ask;
            config.mode = mode;
            let Output::Session(session) = fixture
                .client
                .execute(fixture.client.prepare(Command::SetSessionConfig {
                    session: fixture.session.id,
                    expected_revision: fixture.session.revision,
                    config,
                }))
                .await
                .unwrap()
            else {
                panic!("session expected")
            };
            fixture.session = session;
            let Output::QueuedTurn(turn) = fixture
                .client
                .execute(fixture.client.prepare(Command::SubmitTurn {
                    session: fixture.session.id,
                    expected_revision: fixture.session.revision,
                    message: "Inspect committed contents and line authorship".into(),
                }))
                .await
                .unwrap()
            else {
                panic!("turn expected")
            };
            let page = tokio::time::timeout(Duration::from_secs(10), async {
                loop {
                    let page = history(&fixture.client, fixture.session.id).await;
                    assert!(
                        page.approvals.is_empty(),
                        "read-only Git requested approval: {page:?}"
                    );
                    assert!(
                        !page
                            .runs
                            .iter()
                            .any(|run| { run.turn == turn.id && run.status == Status::Failed }),
                        "{page:?}"
                    );
                    if !page.questions.is_empty()
                        || page
                            .runs
                            .iter()
                            .any(|run| run.turn == turn.id && run.status == Status::Completed)
                    {
                        break page;
                    }
                    tokio::time::sleep(Duration::from_millis(10)).await;
                }
            })
            .await
            .unwrap();
            let output: Vec<_> = results(&fixture.client, &page).await;
            assert_eq!(output.len(), 2, "{page:?}");
            assert_eq!(output[0]["kind"], "git_file");
            assert_eq!(output[0]["data"]["text"], "2: Second\n");
            assert_eq!(output[0]["data"]["commit"], commit.to_string());
            assert_eq!(output[0]["data"]["next"], 2);
            assert_eq!(output[1]["kind"], "git_blame");
            assert_eq!(output[1]["data"]["entries"][0]["author"], "Fixture");
            assert_eq!(
                output[1]["data"]["entries"][0]["commit"],
                commit.to_string()
            );
            assert_eq!(output[1]["data"]["entries"][1]["text"], "Second");
            let requests = server.requests.lock().unwrap().clone();
            let tools: BTreeSet<_> = requests[0]["tools"]
                .as_array()
                .unwrap()
                .iter()
                .filter_map(|tool| tool["function"]["name"].as_str())
                .collect();
            assert!(tools.contains(show.as_str()));
            assert!(tools.contains(blame.as_str()));
            assert_eq!(
                std::fs::read(repository.path().join("index")).unwrap(),
                before
            );
            assert_eq!(
                std::fs::read_to_string(fixture.root.join("-quoted [资料].txt")).unwrap(),
                "Uncommitted content\n"
            );
            fixture.node.shutdown().await.unwrap();
            fixture.controller.close().await.unwrap();
        }
    }
}
