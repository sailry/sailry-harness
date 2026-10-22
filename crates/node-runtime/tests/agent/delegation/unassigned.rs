//! Plain conversations exercise neutral declarations through the actual model catalog.
use super::*;

#[tokio::test]
async fn delegates_from_private_workspaces() {
    for remote in [false, true] {
        for background in [false, true] {
            let directory = tempfile::tempdir().unwrap();
            let node = Node::start_empty(directory.path().join("node"))
                .await
                .unwrap();
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
            let spawn = plugin_tool("example", "spawn");
            let command = plugin_tool("example", "command");
            let server = Server::available_tools(vec![
                (
                    spawn.clone(),
                    json!({"task":"Report the working directory","background":background}),
                ),
                (
                    command.clone(),
                    json!({"command":if cfg!(windows) { "cd" } else { "pwd -P" }}),
                ),
            ])
            .await;
            let Output::Provider(provider) = client
                .execute(client.prepare(Command::PutProvider {
                    provider: Provider {
                        request_max_retries: None,
                        stream_max_retries: None,
                        id: ProviderId::new(),
                        revision: 0,
                        name: "Private workspace fixture".into(),
                        api: ModelApi::ChatCompletions,
                        authentication: sailry_protocol::Authentication::ApiKey,
                        endpoint: server.endpoint.clone(),
                        enabled: true,
                        models: vec![model("fixture-a")],
                        default_model: "fixture-a".into(),
                        credential: None,
                        preset: None,
                        binding: None,
                        consent: None,
                        oauth: None,
                        options: None,
                    },
                    expected_revision: 0,
                }))
                .await
                .unwrap()
            else {
                panic!("provider expected")
            };
            let Output::Session(session) = client
                .execute(client.prepare(Command::CreateSession {
                    project: None,
                    worktree: None,
                    config: Some(SessionConfig {
                        features: Default::default(),
                        provider: provider.id,
                        model: "fixture-a".into(),
                        permission: Permission::Full,
                        assistant: None,
                        resource: None,
                        effort: Effort::High,
                        mode: WorkMode::Code,
                        options: Default::default(),
                        credential: None,
                    }),
                }))
                .await
                .unwrap()
            else {
                panic!("session expected")
            };
            let Output::Session(session) = client
                .execute(client.prepare(Command::SetSessionRoles {
                    session: session.id,
                    expected_revision: session.revision,
                    roles: vec![],
                }))
                .await
                .unwrap()
            else {
                panic!("session expected")
            };
            assert!(session.project.is_none() && session.roles.profiles.is_empty());
            let Output::Snapshot(snapshot) = client
                .execute(client.prepare(Command::Snapshot))
                .await
                .unwrap()
            else {
                panic!("snapshot expected")
            };
            assert!(snapshot.projects.is_empty());
            let root = std::path::PathBuf::from(
                &snapshot
                    .worktrees
                    .iter()
                    .find(|tree| tree.id == session.worktree)
                    .unwrap()
                    .path,
            );
            // Package acquisition uses an external registered source, never Node storage.
            let staging = directory.path().join("source");
            let source = staging.join("fixture");
            std::fs::create_dir_all(source.join("dev.sailry.platform")).unwrap();
            std::fs::write(source.join("dev.sailry.platform/main.js"),
                "export function prepare(input){return input} export function command({command}){return {command,cwd:'',timeout_ms:10000,background:false,attachments:[]}} export function result({output}){return output.data??output}").unwrap();
            std::fs::write(source.join("plugin.json"),json!({
                "$schema":"https://agent-plugins.org/schemas/1.0.0/plugin.schema.json","name":"example","version":"0.1.0",
                "extensions":{"dev.sailry.platform":{"api_version":"v1","actions":["agents.delegate","process.control"],
                    "host":{"entry":"dev.sailry.platform/main.js","resources":["dev.sailry.platform/main.js"],"handlers":["prepare","command","result"]},
                    "tools":[
                        {"name":"spawn","description":"Delegate a fixture task","handler":{"name":"prepare","operation":"agents.delegate","result":"result","parameters":{"type":"object","required":["task"],"properties":{"task":{"type":"string"},"background":{"type":"boolean"}},"additionalProperties":false}}},
                        {"name":"command","description":"Read the current directory","handler":{"name":"command","operation":"process.run","result":"result","parameters":{"type":"object","required":["command"],"properties":{"command":{"type":"string"}},"additionalProperties":false}}}
                    ]
                }}
            }).to_string()).unwrap();
            let Output::Project(project) = client
                .execute(client.prepare(Command::RegisterProject {
                    name: "Package source".into(),
                    path: staging.to_str().unwrap().into(),
                }))
                .await
                .unwrap()
            else {
                panic!("project expected")
            };
            let Output::Snapshot(snapshot) = client
                .execute(client.prepare(Command::Snapshot))
                .await
                .unwrap()
            else {
                panic!("snapshot expected")
            };
            let worktree = snapshot
                .worktrees
                .iter()
                .find(|tree| tree.project == Some(project.id) && tree.main)
                .unwrap()
                .id;
            let Output::Plugin(package) = client
                .execute(client.prepare(Command::InstallPlugin {
                    worktree,
                    path: "fixture".into(),
                    name: "example".into(),
                    expected_revision: 0,
                    expected_digest: None,
                }))
                .await
                .unwrap()
            else {
                panic!("package expected")
            };
            assert!(package.issues.is_empty(), "{:?}", package.issues);
            client
                .execute(client.prepare(Command::RemoveProject { expected: project }))
                .await
                .unwrap();
            let Output::Snapshot(snapshot) = client
                .execute(client.prepare(Command::Snapshot))
                .await
                .unwrap()
            else {
                panic!("snapshot expected")
            };
            assert!(snapshot.projects.is_empty());
            let Output::QueuedTurn(turn) = client
                .execute(client.prepare(Command::SubmitTurn {
                    session: session.id,
                    expected_revision: session.revision,
                    message: "Delegate and report the working directory".into(),
                }))
                .await
                .unwrap()
            else {
                panic!("turn expected")
            };
            let parent = finished(&client, session.id, turn.id).await;
            assert_eq!(
                parent
                    .runs
                    .iter()
                    .find(|run| run.turn == turn.id)
                    .unwrap()
                    .status,
                Status::Completed,
                "{parent:?}"
            );
            let child = children(&client, session.id, 1).await.pop().unwrap();
            assert_eq!(child.project, None);
            assert_eq!(child.worktree, session.worktree);
            let child_turn = history(&client, child.id).await.runs[0].turn;
            let child_page = finished(&client, child.id, child_turn).await;
            assert_eq!(
                child_page
                    .runs
                    .iter()
                    .find(|run| run.turn == child_turn)
                    .unwrap()
                    .status,
                Status::Completed,
                "{child_page:?}"
            );
            let expected = std::fs::canonicalize(&root).unwrap();
            for page in [&parent, &child_page] {
                let outputs = results(page);
                let cwd = outputs
                    .iter()
                    .find_map(|output| output.pointer("/stdout/text").and_then(Value::as_str))
                    .expect("actual command output expected");
                assert_eq!(std::fs::canonicalize(cwd.trim()).unwrap(), expected);
            }
            let requests = server.requests.lock().unwrap();
            assert!(requests.iter().any(|request| {
                request["tools"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|tool| tool["function"]["name"] == spawn)
            }));
            assert!(requests.iter().any(|request| {
                let tools = request["tools"].as_array().unwrap();
                tools.iter().any(|tool| tool["function"]["name"] == command)
                    && tools.iter().all(|tool| tool["function"]["name"] != spawn)
            }));
        }
    }
}
