use super::*;
use serde_json::{Value, json};
use std::{fs, path::Path};
mod admission;
#[cfg(feature = "test-support")]
mod applications;
#[cfg(feature = "test-support")]
mod authorization;
mod connections;
mod content;
#[cfg(feature = "test-support")]
mod credentials;
mod elicitation;
mod http;
#[cfg(feature = "test-support")]
mod management;
mod management_state;
mod modern;
use peer as mcp_peer;
#[path = "../../support/mcp_input.rs"]
#[allow(dead_code, unused_imports)]
pub(super) mod input;
#[path = "../../support/mcp.rs"]
pub(super) mod peer;
mod planning;
mod policies;
mod presentation;
#[cfg(feature = "test-support")]
mod python;
mod settings;
mod sse;
mod stdio;
mod tasks;
mod urls;

use peer::alias;

async fn execute(client: &Client, command: Command) -> Output {
    client.execute(client.prepare(command)).await.unwrap()
}

fn package(root: &Path, servers: Value, version: &str) {
    fs::create_dir_all(root.join("package/skills/analysis")).unwrap();
    fs::create_dir_all(root.join("package").join(plugin::NAMESPACE)).unwrap();
    fs::write(root.join("package/plugin.json"), json!({"$schema": "https://agent-plugins.org/schemas/1.0.0/plugin.schema.json", "name": "example", "version": version}).to_string()).unwrap();
    fs::write(root.join("package/mcp.json"), json!({"$schema": "https://agent-plugins.org/schemas/1.0.0/mcp.schema.json", "mcpServers": servers}).to_string()).unwrap();
    fs::write(
        root.join("package/skills/analysis/SKILL.md"),
        "---\nname: analysis\ndescription: Analyze data\n---\nComplete skill beside MCP\n",
    )
    .unwrap();
    fs::write(root.join("package/version.txt"), version).unwrap();
}

async fn install(fixture: &process::Fixture, revision: u64) -> plugin::Info {
    let Output::Plugin(info) = execute(
        &fixture.client,
        Command::InstallPlugin {
            worktree: fixture.session.worktree,
            path: "package".into(),
            name: "example".into(),
            expected_revision: revision,
            expected_digest: None,
        },
    )
    .await
    else {
        panic!("plugin expected")
    };
    info
}

async fn isolate(client: &Client) {
    let Output::Plugins(packages) = execute(client, Command::ListPlugins).await else {
        panic!("plugin inventory expected")
    };
    for package in packages.into_iter().filter(|package| package.enabled) {
        execute(
            client,
            Command::SetPluginEnabled {
                name: package.name,
                expected_revision: package.revision,
                enabled: false,
            },
        )
        .await;
    }
}

async fn submit(fixture: &process::Fixture, queued: bool) -> (Request, QueuedTurn) {
    let Output::Plugins(packages) = execute(&fixture.client, Command::ListPlugins).await else {
        panic!("plugin inventory expected")
    };
    if let Some(package) = packages
        .into_iter()
        .find(|package| package.name == "commands" && package.enabled)
    {
        execute(
            &fixture.client,
            Command::SetPluginEnabled {
                name: package.name,
                expected_revision: package.revision,
                enabled: false,
            },
        )
        .await;
    }
    let command = if queued {
        Command::QueueTurn {
            session: fixture.session.id,
            expected_revision: fixture.session.revision,
            message: "Use the configured MCP tools".into(),
        }
    } else {
        Command::SubmitTurn {
            session: fixture.session.id,
            expected_revision: fixture.session.revision,
            message: "Use the configured MCP tools".into(),
        }
    };
    let request = fixture.client.prepare(command);
    let Output::QueuedTurn(turn) = fixture.client.execute(request.clone()).await.unwrap() else {
        panic!("turn expected")
    };
    (request, turn)
}

async fn present(path: &Path) {
    tokio::time::timeout(Duration::from_secs(10), async {
        while !fs::read(path).is_ok_and(|bytes| !bytes.is_empty()) {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("fixture resource deadline");
}

fn reaped(data: &Path) {
    let pid: i32 = fs::read_to_string(data.join("pid"))
        .unwrap()
        .parse()
        .unwrap();
    // SAFETY: signal 0 checks this fixture PID without sending a signal or using pointers.
    assert_ne!(
        unsafe { libc::kill(pid, 0) },
        0,
        "MCP child must be reaped after connection closure"
    );
}
