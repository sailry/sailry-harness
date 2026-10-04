use super::*;
use serde_json::{Value, json};
use std::{fs, path::Path};

fn package(root: &Path, version: &str) -> String {
    fs::create_dir_all(root.join("package/skills/analysis/references")).unwrap();
    fs::write(root.join("package/plugin.json"), json!({"$schema": "https://agent-plugins.org/schemas/1.0.0/plugin.schema.json", "name": "example", "version": version}).to_string()).unwrap();
    let body = format!(
        "---\nname: analysis\ndescription: Analyze project data\nallowed-tools: {}\n---\nVersion {version}\nLiteral {{missing_state}} {{\"key\":\"中文 🙂\"}}\nRead references/guide.md\n{}\nComplete end\n",
        plugin_tool("commands", "run_command"),
        "完整内容🙂".repeat(1500)
    );
    fs::write(root.join("package/skills/analysis/SKILL.md"), &body).unwrap();
    fs::write(
        root.join("package/skills/analysis/references/guide.md"),
        format!("Guide {version} 中文 🙂"),
    )
    .unwrap();
    fs::write(
        root.join("package/skills/analysis/check.sh"),
        "printf 'script output 中文 🙂'; printf x >> script-count.txt\n",
    )
    .unwrap();
    fs::create_dir_all(root.join("package/skills/analysis/scripts")).unwrap();
    fs::write(
        root.join("package/skills/analysis/scripts/query.sh"),
        "printf '%s\\n' \"$1\" \"$PWD\"\n",
    )
    .unwrap();
    body
}

async fn execute(client: &Client, command: Command) -> Output {
    client.execute(client.prepare(command)).await.unwrap()
}

async fn enabled_only(client: &Client, names: &[&str]) {
    let Output::Plugins(entries) = execute(client, Command::ListPlugins).await else {
        panic!("plugins expected");
    };
    for name in names {
        assert!(
            entries
                .iter()
                .any(|entry| entry.enabled && entry.name == *name)
        );
    }
    // These reference fixtures intentionally isolate their participating packages.
    for entry in entries {
        if entry.enabled && !names.contains(&entry.name.as_str()) {
            execute(
                client,
                Command::SetPluginEnabled {
                    name: entry.name,
                    expected_revision: entry.revision,
                    enabled: false,
                },
            )
            .await;
        }
    }
}

async fn install(client: &Client, worktree: WorktreeId, revision: u64) -> plugin::Info {
    let Output::Plugin(info) = execute(
        client,
        Command::InstallPlugin {
            worktree,
            path: "package".into(),
            name: "example".into(),
            expected_revision: revision,
        },
    )
    .await
    else {
        panic!("plugin expected")
    };
    info
}

async fn fixture(
    remote: bool,
    server: &Server,
) -> (tempfile::TempDir, Node, Link, Client, Session) {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().join("project");
    package(&root, "1.0.0");
    let node = Node::start(directory.path().join("node")).await.unwrap();
    let controller = Link::controller(directory.path().join("controller"), NetworkScope::default())
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
    let (session, mut provider) = configured(&client, &server.endpoint, &root).await;
    isolate_model_tools(&client).await;
    disable_tools(&client, &["browser", "media"]).await;
    provider.models = vec![Model {
        id: "fixture-a".into(),
        context: 128000,
        output: 2048,
        vision: false,
        tools: true,
        reasoning: false,
        web_search: false,
        generates: vec![],
        efforts: vec![],
        custom_efforts: false,
        default_effort: sailry_protocol::Effort::Default,
    }];
    provider.default_model = "fixture-a".into();
    execute(
        &client,
        Command::PutProvider {
            provider,
            expected_revision: 1,
        },
    )
    .await;
    install(&client, session.worktree, 0).await;
    (directory, node, controller, client, session)
}

fn results(page: &Page) -> Vec<Value> {
    page.entries
        .iter()
        .flat_map(|entry| &entry.parts)
        .filter_map(|part| match part {
            Part::ToolResult { result, .. } => Some(result.clone()),
            _ => None,
        })
        .collect()
}

mod execution;
mod forwarding;
mod management;
#[cfg(all(unix, feature = "test-support"))]
mod standalone;
mod versions;
