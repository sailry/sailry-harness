use super::*;
use serde_json::{Value, json};
use std::{
    collections::BTreeMap, fs, io::Write, os::unix::fs::PermissionsExt, path::Path, time::Duration,
};

#[tokio::test]
async fn retains_and_releases_cursor_sessions() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().canonicalize().unwrap();
    let helpers = root.join("helpers");
    let executable = if cfg!(target_os = "macos") {
        helpers.join("Sailry Computer Use.app/Contents/MacOS/agent-desktop")
    } else {
        helpers.join("agent-desktop")
    };
    fs::create_dir_all(executable.parent().unwrap()).unwrap();
    let peer = std::env::current_exe().unwrap();
    #[cfg(target_os = "macos")]
    let peer = {
        let copied = root.join("mcp-peer");
        fs::copy(&peer, &copied).unwrap();
        copied
    };
    fs::write(root.join("binary"), peer.to_str().unwrap()).unwrap();
    let fixture = format!(
        "#!/bin/sh\nfixture='{}'\n{}",
        root.to_str().unwrap().replace('\'', "'\\''"),
        r#"home="$AGENT_DESKTOP_HOME"
printf '%s|%s\n' "$*" "$AGENT_DESKTOP_SESSION" >> "$home/calls"
case "$*" in
  'session gc --ended') printf '{"version":"2.6","ok":true,"command":"session","data":{"removed":[]}}\n' ;;
  'session start --no-trace --name sailry')
    count="$(cat "$home/count" 2>/dev/null || printf 0)"
    count=$((count + 1))
    printf '%s' "$count" > "$home/count"
    session="run-fixture-$count"
    mkdir -p "$home/sessions/$session"
    printf '{"id":"%s","name":"sailry","created_at":1,"trace":"off","artifacts":"events"}' "$session" > "$home/sessions/$session/session.json"
    printf '{"version":"2.6","ok":true,"command":"session","data":{"activation":{"environment":"AGENT_DESKTOP_SESSION","value":"%s"},"artifacts":"events","created_at":1,"name":"sailry","next":"export AGENT_DESKTOP_SESSION=%s","session_id":"%s","trace":"off"}}\n' "$session" "$session" "$session" ;;
  'session end '*)
    printf '{"id":"%s","ended_at":1}' "$3" > "$home/sessions/$3/session.json"
    printf '{"version":"2.6","ok":true,"command":"session","data":{"ended_at":1,"session_id":"%s"}}\n' "$3" ;;
  mcp)
    exec "$(cat "$fixture/binary")" --exact plugins::actions::computer::overlay::stdio_peer --ignored --nocapture --test-threads=1 ;;
  *) exit 64 ;;
esac
"#
    );
    fs::write(&executable, fixture).unwrap();
    fs::set_permissions(&executable, fs::Permissions::from_mode(0o700)).unwrap();
    let output = tokio::time::timeout(
        Duration::from_secs(120),
        tokio::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "plugins::actions::computer::overlay::lifecycle_peer",
                "--ignored",
                "--nocapture",
            ])
            .env("SAILRY_HELPERS_DIR", helpers)
            .env("SAILRY_TEST_COMPUTER_OVERLAY", root)
            .kill_on_drop(true)
            .output(),
    )
    .await
    .unwrap()
    .unwrap();
    assert!(
        output.status.success(),
        "overlay subprocess failed:\n{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
#[ignore = "launched by the isolated cursor session fixture"]
fn lifecycle_peer() {
    assert!(std::env::var_os("SAILRY_TEST_COMPUTER_OVERLAY").is_some());
    tokio::runtime::Runtime::new().unwrap().block_on(async {
        for remove in [false, true] {
            let (_directory, node, controller, client, _worktree) =
                crate::plugins::support::packages(&["computer"]).await;
            let home = node.profile().join("plugins/data/computer/agent-desktop");
            set_enabled(&client, true).await;
            check(&client).await;
            check(&client).await;
            assert_eq!(fs::read_to_string(home.join("count")).unwrap(), "1");
            assert_eq!(
                calls(&home)
                    .iter()
                    .filter(|call| *call == "mcp|run-fixture-1")
                    .count(),
                2
            );
            configure(&client, false).await;
            ended(&home, "run-fixture-1").await;
            check(&client).await;
            assert_eq!(calls(&home).last().unwrap(), "mcp|");
            configure(&client, true).await;
            check(&client).await;
            assert_eq!(calls(&home).last().unwrap(), "mcp|run-fixture-2");
            set_enabled(&client, false).await;
            ended(&home, "run-fixture-2").await;
            set_enabled(&client, true).await;
            check(&client).await;
            assert_eq!(calls(&home).last().unwrap(), "mcp|run-fixture-3");
            if remove {
                let package = current(&client).await;
                execute(
                    &client,
                    Command::RemovePlugin {
                        name: "computer".into(),
                        expected_revision: package.summary.revision,
                    },
                )
                .await;
                ended(&home, "run-fixture-3").await;
            }
            drop(client);
            controller.close().await.unwrap();
            node.shutdown().await.unwrap();
            assert!(calls(&home).contains(&"session end run-fixture-3|".into()));
            assert_eq!(
                calls(&home)
                    .iter()
                    .filter(|call| *call == "session gc --ended|")
                    .count(),
                1
            );
        }
    });
}

async fn current(client: &sailry_client::Client) -> plugin::Info {
    info(
        execute(
            client,
            Command::ReadPlugin {
                name: "computer".into(),
            },
        )
        .await,
    )
}

async fn set_enabled(client: &sailry_client::Client, enabled: bool) {
    let package = current(client).await;
    execute(
        client,
        Command::SetPluginEnabled {
            name: "computer".into(),
            expected_revision: package.summary.revision,
            enabled,
        },
    )
    .await;
}

async fn configure(client: &sailry_client::Client, enabled: bool) {
    let package = current(client).await;
    execute(
        client,
        Command::SavePluginSettings {
            package: package.summary.reference(),
            values: BTreeMap::from([("cursor_overlay".into(), json!(enabled))]),
            secrets: BTreeMap::new(),
        },
    )
    .await;
}

async fn check(client: &sailry_client::Client) {
    let package = current(client).await;
    let Output::PluginMcpChecked(result) = execute(
        client,
        Command::CheckPluginMcp {
            package: package.summary.reference(),
            server: "desktop".into(),
        },
    )
    .await
    else {
        panic!("computer MCP check expected");
    };
    assert_eq!(result.tools.len(), 1);
    assert_eq!(result.tools[0].name, "desktop_snapshot");
}

fn calls(home: &Path) -> Vec<String> {
    fs::read_to_string(home.join("calls"))
        .unwrap()
        .lines()
        .map(str::to_owned)
        .collect()
}

async fn ended(home: &Path, session: &str) {
    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            if calls(home).contains(&format!("session end {session}|")) {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("cursor session cleanup deadline");
}

#[test]
#[ignore = "launched only as an isolated computer MCP subprocess"]
fn stdio_peer() {
    use std::io::BufRead;
    assert!(std::env::var_os("AGENT_DESKTOP_HOME").is_some());
    if let Ok(session) = std::env::var("AGENT_DESKTOP_SESSION") {
        let home = std::env::var("AGENT_DESKTOP_HOME").unwrap();
        let path = Path::new(&home)
            .join("sessions")
            .join(session)
            .join("session.json");
        let manifest: Value = serde_json::from_slice(&fs::read(path).unwrap()).unwrap();
        let overlay = &manifest["cursor_overlay"];
        assert_eq!(overlay["enabled"], true);
        assert!(overlay.get("label").is_none());
        assert!(overlay.get("multi_agent").is_none());
        #[cfg(target_os = "macos")]
        {
            let image = &overlay["style"]["images"]["arrow"];
            assert!(Path::new(image["path"].as_str().unwrap()).is_file());
            assert_eq!(image["hotspot"], json!({"x": 16, "y": 16}));
        }
        #[cfg(not(target_os = "macos"))]
        {
            assert_eq!(overlay["style"]["size"], 0.75);
            assert!(overlay["style"].get("images").is_none());
        }
    }
    println!();
    for line in std::io::stdin().lock().lines() {
        let Ok(line) = line else { break };
        let Ok(request) = serde_json::from_str::<Value>(&line) else {
            continue;
        };
        let id = &request["id"];
        let result = match request["method"].as_str().unwrap_or("") {
            "initialize" => json!({
                "protocolVersion":request["params"]["protocolVersion"],
                "capabilities":{"tools":{}},
                "serverInfo":{"name":"computer-fixture","version":"1"}
            }),
            "tools/list" => json!({"tools":[{
                "name":"desktop_snapshot",
                "description":"Inspect a fixture app",
                "inputSchema":{"type":"object","properties":{}}
            }]}),
            _ if id.is_null() => continue,
            _ => {
                println!(
                    "{}",
                    json!({"jsonrpc":"2.0","id":id,"error":{"code":-32601,"message":"Method not found"}})
                );
                std::io::stdout().flush().unwrap();
                continue;
            }
        };
        println!("{}", json!({"jsonrpc":"2.0","id":id,"result":result}));
        std::io::stdout().flush().unwrap();
    }
}
