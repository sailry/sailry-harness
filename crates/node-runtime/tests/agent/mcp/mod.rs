#[cfg(unix)]
use super::*;
#[cfg(unix)]
use serde_json::{Value, json};
#[cfg(unix)]
use std::{fs, path::Path};
#[cfg(unix)]
mod admission;
#[cfg(all(unix, feature = "test-support"))]
mod applications;
#[cfg(all(unix, feature = "test-support"))]
mod authorization;
#[cfg(unix)]
mod connections;
#[cfg(unix)]
mod content;
#[cfg(all(unix, feature = "test-support"))]
mod credentials;
#[cfg(unix)]
mod elicitation;
#[cfg(unix)]
mod http;
#[cfg(all(unix, feature = "test-support"))]
mod management;
#[cfg(unix)]
mod management_state;
#[cfg(unix)]
mod modern;
#[cfg(unix)]
use peer as mcp_peer;
#[cfg(unix)]
#[path = "../../support/mcp_input.rs"]
#[allow(dead_code, unused_imports)]
pub(super) mod input;
#[path = "../../support/mcp.rs"]
pub(super) mod peer;
#[cfg(unix)]
mod planning;
#[cfg(unix)]
mod policies;
#[cfg(unix)]
mod presentation;
#[cfg(all(unix, feature = "test-support"))]
mod python;
#[cfg(unix)]
mod settings;
#[cfg(unix)]
mod sse;
#[cfg(unix)]
mod stdio;
#[cfg(unix)]
mod tasks;
#[cfg(unix)]
mod urls;

#[cfg(unix)]
use peer::alias;

#[cfg(unix)]
async fn execute(client: &Client, command: Command) -> Output {
    client.execute(client.prepare(command)).await.unwrap()
}

#[cfg(unix)]
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

#[cfg(unix)]
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

#[cfg(unix)]
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

#[cfg(unix)]
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

#[cfg(unix)]
async fn case_fixture<'a>(
    shared: &'a mut Option<process::Fixture>,
    remote: bool,
    model: &Server,
    name: &str,
) -> &'a mut process::Fixture {
    if let Some(fixture) = shared {
        select_case(fixture, model, name).await;
    } else {
        *shared = Some(process::Fixture::packages(remote, model, &[]).await);
    }
    shared.as_mut().unwrap()
}

#[cfg(unix)]
async fn select_case(fixture: &mut process::Fixture, model: &Server, name: &str) {
    let root = fixture.directory.path().join(name);
    fs::create_dir(&root).unwrap();
    fixture.session = approvals::prepare_packages(&fixture.client, model, &root, &[]).await;
    fixture.root = root;
}

#[cfg(unix)]
async fn present(path: &Path) {
    tokio::time::timeout(Duration::from_secs(10), async {
        while !fs::read(path).is_ok_and(|bytes| !bytes.is_empty()) {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("fixture resource deadline");
}

#[cfg(unix)]
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
