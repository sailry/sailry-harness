use super::*;
use std::io::{BufRead, Write};

fn append(path: &Path, value: &str) {
    fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .unwrap()
        .write_all(value.as_bytes())
        .unwrap();
}

fn respond(id: &Value, result: Value) {
    println!("{}", json!({"jsonrpc":"2.0", "id":id, "result":result}));
    std::io::stdout().flush().unwrap();
}

fn task(status: &str) -> Value {
    json!({"resultType":"complete","taskId":"first","status":status,
        "createdAt":"2026-11-08T00:00:00Z","lastUpdatedAt":"2026-11-08T00:00:00Z",
        "ttlMs":null,"pollIntervalMs":10})
}

fn late() {
    println!(
        "{}",
        json!({"jsonrpc":"2.0","id":"late","method":"elicitation/create",
        "params":{"mode":"form","message":"Late input","requestedSchema":input::form_schema()}})
    );
    std::io::stdout().flush().unwrap();
}

#[test]
#[ignore = "launched only as an isolated MCP subprocess"]
fn stdio_peer() {
    assert_eq!(std::env::var("SAILRY_MCP_FIXTURE").as_deref(), Ok("1"));
    println!();
    let data = std::env::var("PLUGIN_DATA").unwrap();
    let data = Path::new(&data);
    let mode = std::env::var("SAILRY_MCP_MODE").unwrap();
    append(&data.join("launches"), "x");
    let mut first = None;
    let mut cancelled = false;
    for line in std::io::stdin().lock().lines() {
        let Ok(line) = line else { break };
        let Ok(request) = serde_json::from_str::<Value>(&line) else {
            continue;
        };
        let id = &request["id"];
        let method = request["method"].as_str().unwrap_or("");
        if id == "late" {
            fs::write(
                data.join("late.json"),
                serde_json::to_vec(&request).unwrap(),
            )
            .unwrap();
            continue;
        }
        if method == "notifications/cancelled" {
            assert_eq!(request["params"]["requestId"], first.clone().unwrap());
            fs::write(data.join("cancelled"), "ready").unwrap();
            late();
            let data = data.to_owned();
            let id = first.take().unwrap();
            std::thread::spawn(move || {
                while !data.join("release").exists() {
                    std::thread::sleep(Duration::from_millis(10));
                }
                respond(&id, json!({"content":[{"type":"text","text":"Confirmed"}]}));
                fs::write(data.join("confirmed"), "ready").unwrap();
            });
            continue;
        }
        if id.is_null() {
            continue;
        }
        match method {
            "server/discover" => {
                println!(
                    "{}",
                    json!({"jsonrpc":"2.0","id":id,"error":{"code":-32601,"message":"Method not found"}})
                );
                std::io::stdout().flush().unwrap();
            }
            "initialize" => {
                let mut capabilities = json!({"tools":{}});
                if mode == "task" {
                    capabilities["extensions"] = json!({"io.modelcontextprotocol/tasks":{}});
                }
                respond(
                    id,
                    json!({"protocolVersion":request["params"]["protocolVersion"],"capabilities":capabilities,
                    "serverInfo":{"name":"settlement","version":"1"}}),
                );
            }
            "tools/list" => respond(id, json!({"tools":[peer::descriptor("write")]})),
            "tools/call" => {
                let value = request["params"]["arguments"]["value"].as_str().unwrap();
                append(&data.join("calls"), &format!("{value}\n"));
                fs::write(data.join(format!("entered.{value}")), "ready").unwrap();
                if value == "first" {
                    first = Some(id.clone());
                    if mode == "task" {
                        let mut created = task("working");
                        created["resultType"] = json!("task");
                        respond(id, created);
                    }
                } else {
                    assert!(data.join("release").exists(), "prior call has not settled");
                    respond(
                        id,
                        json!({"content":[{"type":"text","text":"Second finished"}]}),
                    );
                }
            }
            "tasks/cancel" => {
                assert_eq!(request["params"]["taskId"], "first");
                if !cancelled {
                    cancelled = true;
                    fs::write(data.join("cancelled"), "ready").unwrap();
                    late();
                }
                respond(id, json!({"resultType":"complete"}));
            }
            "tasks/get" => {
                assert_eq!(request["params"]["taskId"], "first");
                let terminal = cancelled && data.join("release").exists();
                if terminal {
                    fs::write(data.join("confirmed"), "ready").unwrap();
                }
                respond(id, task(if terminal { "cancelled" } else { "working" }));
            }
            "ping" => respond(id, json!({})),
            _ => {}
        }
    }
    fs::write(data.join("closed"), "ready").unwrap();
    std::process::exit(0);
}

async fn next_session(fixture: &mut process::Fixture) {
    let Output::Session(session) = execute(
        &fixture.client,
        Command::CreateSession {
            project: fixture.session.project,
            worktree: Some(fixture.session.worktree),
            config: Some(fixture.session.config.clone()),
        },
    )
    .await
    else {
        panic!("session expected");
    };
    fixture.session = session;
}

#[tokio::test]
async fn waits_for_confirmation_on_shared_connection() {
    for mode in ["request", "task"] {
        let model =
            Server::scripted_tools(vec![(alias("native", "write"), json!({"value":"first"}))])
                .await;
        let mut fixture = process::Fixture::packages(&model, &[]).await;
        let mut server = peer::config("mcp::settlement::stdio_peer", mode);
        server["exclusive"] = json!(mode == "task");
        server["reuse"] = json!(true);
        package(&fixture.root, json!({"native":server}), "0.1.0");
        install(&fixture, 0).await;
        let first_session = fixture.session.id;
        let (_, first) = submit(&fixture, false).await;
        let (_, approval) = approvals::pending(&fixture.client, first_session).await;
        process::decide(&fixture.client, &approval, Decision::Approve).await;
        let data = fixture.node.profile().join("plugins/data/example");
        present(&data.join("entered.first")).await;
        execute(&fixture.client, Command::StopTurn { turn: first.id }).await;
        present(&data.join("cancelled")).await;
        present(&data.join("late.json")).await;
        let late: Value =
            serde_json::from_slice(&fs::read(data.join("late.json")).unwrap()).unwrap();
        assert!(late["error"].is_object(), "{late:?}");
        next_session(&mut fixture).await;
        model.replace_tools(vec![(alias("native", "write"), json!({"value":"second"}))]);
        let (_, second) = submit(&fixture, false).await;
        let (_, approval) = approvals::pending(&fixture.client, fixture.session.id).await;
        process::decide(&fixture.client, &approval, Decision::Approve).await;
        assert!(
            tokio::time::timeout(
                Duration::from_millis(150),
                present(&data.join("entered.second"))
            )
            .await
            .is_err()
        );
        assert_eq!(fs::read_to_string(data.join("calls")).unwrap(), "first\n");
        fs::write(data.join("release"), "ready").unwrap();
        present(&data.join("confirmed")).await;
        present(&data.join("entered.second")).await;
        let cancelled = finished(&fixture.client, first_session, first.id).await;
        let completed = finished(&fixture.client, fixture.session.id, second.id).await;
        assert_eq!(cancelled.runs.last().unwrap().status, Status::Cancelled);
        assert_eq!(
            completed.runs.last().unwrap().status,
            Status::Completed,
            "{completed:?}"
        );
        assert!(cancelled.questions.is_empty());
        assert!(completed.questions.is_empty());
        assert_eq!(fs::read(data.join("launches")).unwrap(), b"x");
        assert!(!data.join("closed").exists());
        fixture.node.shutdown().await.unwrap();
        fixture.controller.close().await.unwrap();
    }
}

#[tokio::test]
async fn drains_retired_call_during_shutdown() {
    let model =
        Server::scripted_tools(vec![(alias("native", "write"), json!({"value":"first"}))]).await;
    let fixture = process::Fixture::packages(&model, &[]).await;
    let mut server = peer::config("mcp::settlement::stdio_peer", "request");
    server["exclusive"] = json!(true);
    server["reuse"] = json!(true);
    package(&fixture.root, json!({"native":server}), "0.1.0");
    let info = install(&fixture, 0).await;
    let (_, turn) = submit(&fixture, false).await;
    let (_, approval) = approvals::pending(&fixture.client, fixture.session.id).await;
    process::decide(&fixture.client, &approval, Decision::Approve).await;
    let data = fixture.node.profile().join("plugins/data/example");
    present(&data.join("entered.first")).await;
    execute(&fixture.client, Command::StopTurn { turn: turn.id }).await;
    present(&data.join("cancelled")).await;
    execute(
        &fixture.client,
        Command::SetPluginEnabled {
            name: info.summary.name,
            expected_revision: info.summary.revision,
            enabled: false,
        },
    )
    .await;
    assert!(!data.join("closed").exists());
    tokio::time::timeout(Duration::from_secs(30), fixture.node.shutdown())
        .await
        .expect("retired MCP settlement shutdown deadline")
        .unwrap();
    fixture.controller.close().await.unwrap();
}
