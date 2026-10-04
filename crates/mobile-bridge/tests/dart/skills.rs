use super::{configuration::support, tools};
use sailry_client::Client;
use sailry_node_runtime::Node;
use sailry_protocol::{conversation::Status, *};
use serde_json::json;
use std::fs;

#[test]
#[ignore = "requires built bridge and dart pub get in tests/mobile-contract"]
fn controls_host_skills() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().join("project");
    let skill = root.join("package/skills/analysis");
    fs::create_dir_all(skill.join("references")).unwrap();
    fs::write(root.join("package/plugin.json"), json!({"$schema": "https://agent-plugins.org/schemas/1.0.0/plugin.schema.json", "name": "example", "version": "1.0.0"}).to_string()).unwrap();
    let body = format!(
        "---\nname: analysis\ndescription: Analyze project data\nallowed-tools: run_command\n---\nLiteral {{missing_state}}\n{}\nComplete end\n",
        "完整内容🙂".repeat(1500)
    );
    fs::write(skill.join("SKILL.md"), &body).unwrap();
    fs::write(skill.join("references/guide.md"), "Guide 1.0.0 中文 🙂").unwrap();
    fs::write(
        skill.join("check.sh"),
        "printf 'script output 中文 🙂'; printf x >> script-count.txt\n",
    )
    .unwrap();
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let idle = runtime.block_on(support::Server::start(false));
    let (node, sessions) = runtime.block_on(tools::sessions(
        directory.path(),
        &idle.endpoint,
        "ffi-skills",
    ));
    let (server, references) = runtime.block_on(async {
        let client = Client::new(node.local());
        let Output::Plugins(packages) = execute(&client, Command::ListPlugins).await else { panic!("packages expected") };
        for package in packages.into_iter().filter(|package| package.enabled && package.name != "commands") {
            execute(&client, Command::SetPluginEnabled { name: package.name, expected_revision: package.revision, enabled: false }).await;
        }
        let Output::Snapshot(snapshot) = execute(&client, Command::Snapshot).await else { panic!("snapshot expected") };
        let Output::Plugin(plugin) = execute(&client, Command::InstallPlugin { worktree: snapshot.sessions[0].worktree, path: "package".into(), name: "example".into(), expected_revision: 0 }).await else { panic!("plugin expected") };
        let Output::Plugin(commands) = execute(&client, Command::ReadPlugin { name: "commands".into() }).await else { panic!("commands expected") };
        let references = vec![commands.summary.reference(), plugin.summary.reference()];
        let script = node.profile().join("plugins/packages").join(&plugin.summary.digest).join("skills/analysis/check.sh");
        let command = format!("sh '{}'", script.to_str().unwrap().replace('\'', "'\\''"));
        let server = support::Server::tools(vec![
            ("load_skill".into(), json!({"skill": "example:analysis"})),
            ("read_skill_resource".into(), json!({"skill": "example:analysis", "path": "references/guide.md"})),
            ("ask_user".into(), json!({"prompt": "补充要求 中文 🙂", "input": {"kind": "text", "multiline": true, "max_bytes": 128}})),
            (super::agent_support::plugin_tool("commands", "run_command"), json!({"command": command})),
            (super::agent_support::plugin_tool("commands", "run_command"), json!({"command": "printf denied > denied.txt"})),
        ]).await;
        let mut provider = snapshot.providers[0].clone();
        provider.endpoint = server.endpoint.clone();
        provider.models[0].context = 128000;
        execute(&client, Command::PutProvider { expected_revision: provider.revision, provider }).await;
        (server, references)
    });
    fs::write(skill.join("SKILL.md"), "Source changed after installation").unwrap();
    fs::write(skill.join("references/guide.md"), "Source changed").unwrap();
    fs::write(skill.join("check.sh"), "exit 99\n").unwrap();
    let invitation = node.link().invite().unwrap();
    let child = super::command("skills.dart", directory.path(), invitation.ticket())
        .env("SAILRY_SESSIONS", serde_json::to_string(&sessions).unwrap())
        .spawn()
        .unwrap();
    let status = super::wait(child);
    let (snapshot, pages) = runtime.block_on(async {
        let client = Client::new(node.local());
        let Output::Snapshot(snapshot) = execute(&client, Command::Snapshot).await else {
            panic!("snapshot expected")
        };
        let pages = tools::pages(&client, &sessions).await;
        node.shutdown().await.unwrap();
        (snapshot, pages)
    });
    assert!(status.success());
    assert_eq!(pages[0].runs[0].status, Status::Completed);
    assert_eq!(pages[1].runs[0].status, Status::Cancelled);
    assert!(snapshot.turns.iter().all(|turn| turn.plugins == references));
    assert_eq!(
        fs::read_to_string(root.join("script-count.txt")).unwrap(),
        "x"
    );
    assert!(!root.join("denied.txt").exists());
    assert!(!directory.path().join("controller/plugins").exists());
    runtime.block_on(async {
        let node = Node::start(directory.path().join("node")).await.unwrap();
        let client = Client::new(node.local());
        assert_eq!(tools::pages(&client, &sessions).await, pages);
        let Output::Snapshot(restored) = execute(&client, Command::Snapshot).await else {
            panic!("snapshot expected")
        };
        assert_eq!(restored.sessions, snapshot.sessions);
        assert_eq!(restored.turns, snapshot.turns);
        node.shutdown().await.unwrap();
    });
    let requests = server.requests.lock().unwrap();
    assert_eq!(requests.len(), 9);
    let results: Vec<serde_json::Value> = requests[5]["messages"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|message| message["role"] == "tool")
        .map(|message| serde_json::from_str(message["content"].as_str().unwrap()).unwrap())
        .collect();
    assert_eq!(results[0]["content"], body);
    assert_eq!(results[1]["content"], "Guide 1.0.0 中文 🙂");
    assert_eq!(results[2]["answer"], " 第一行 中文 🙂\nSecond line ");
    assert_eq!(
        results[3]["data"]["stdout"]["text"],
        "script output 中文 🙂"
    );
    assert!(results[4].get("error").is_some());
    assert!(idle.requests.lock().unwrap().is_empty());
    assert!(server.authorization.lock().unwrap().iter().all(|header| header.as_deref() == Some("Bearer isolated-ffi-configuration-credential")));
}

async fn execute(client: &Client, command: Command) -> Output {
    client.execute(client.prepare(command)).await.unwrap()
}
