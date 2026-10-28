//! Generic MCP policies use real Node admission, transports and model projections.
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

#[test]
#[ignore = "launched only as an isolated MCP subprocess"]
fn stdio_peer() {
    assert_eq!(std::env::var("SAILRY_MCP_FIXTURE").as_deref(), Ok("1"));
    println!();
    let data = std::env::var("PLUGIN_DATA").unwrap();
    let data = Path::new(&data);
    let mode = std::env::var("SAILRY_MCP_MODE").unwrap();
    append(&data.join("pids"), &format!("{}\n", std::process::id()));
    append(&data.join("launches"), "x");
    fs::write(data.join("pid"), std::process::id().to_string()).unwrap();
    let mut held = None;
    let tasks = input::tasks::State::default();
    let task_calls = std::sync::atomic::AtomicUsize::default();
    for line in std::io::stdin().lock().lines() {
        let Ok(line) = line else { break };
        let Ok(request) = serde_json::from_str::<Value>(&line) else {
            continue;
        };
        let id = &request["id"];
        if id.is_null() {
            continue;
        }
        let method = request["method"].as_str().unwrap_or("");
        if mode == "task" && (method == "initialize" || method.starts_with("tasks/")) {
            let result = tasks
                .respond(&request, &input::form_schema(), &task_calls)
                .unwrap();
            if method.starts_with("tasks/") {
                append(
                    &data.join("task.events"),
                    &format!("{method}:{}\n", result["status"].as_str().unwrap_or("")),
                );
                if method == "tasks/get" && result["status"] == "cancelled" {
                    fs::write(data.join("task.terminal"), "ready").unwrap();
                    while !data.join("task.release").exists() {
                        std::thread::sleep(Duration::from_millis(10));
                    }
                }
            }
            println!("{}", json!({"jsonrpc":"2.0","id":id,"result":result}));
            std::io::stdout().flush().unwrap();
            continue;
        }
        let result = match request["method"].as_str().unwrap_or("") {
            "server/discover" => {
                println!(
                    "{}",
                    json!({"jsonrpc":"2.0","id":id,"error":{"code":-32601,"message":"Method not found"}})
                );
                std::io::stdout().flush().unwrap();
                continue;
            }
            "initialize" => {
                json!({"protocolVersion":"2025-11-25","capabilities":{"tools":{}},"serverInfo":{"name":"policy-fixture","version":"1"}})
            }
            "tools/list" => {
                let names = if matches!(mode.as_str(), "mac" | "windows") {
                    let mut names = vec![
                        "list_apps",
                        "get_app_state",
                        "click",
                        "perform_secondary_action",
                        "scroll",
                        "drag",
                        "type_text",
                        "press_key",
                        "set_value",
                    ];
                    if mode == "mac" {
                        names.push("query");
                    }
                    names
                } else {
                    vec!["read", "write", "hold", "fail"]
                };
                let descriptors: Vec<_> = names
                    .into_iter()
                    .map(|name| {
                        let mut descriptor = peer::descriptor(name);
                        descriptor["annotations"]["readOnlyHint"] = json!(
                            name == "read"
                                || name == "list_apps"
                                || name == "query"
                                || (mode == "windows" && name == "get_app_state")
                        );
                        descriptor
                    })
                    .collect();
                json!({"tools":descriptors})
            }
            "tools/call" => {
                let name = request["params"]["name"].as_str().unwrap();
                append(
                    &data.join("calls.jsonl"),
                    &format!("{}\n", request["params"]),
                );
                if (mode.starts_with("sent_") || mode == "task") && name == "write" {
                    let value = request["params"]["arguments"]["value"].as_str().unwrap();
                    if value == "first" {
                        held = Some(
                            fs::OpenOptions::new()
                                .write(true)
                                .create_new(true)
                                .open(data.join("active"))
                                .unwrap(),
                        );
                        fs::write(data.join("pid.first"), std::process::id().to_string()).unwrap();
                        fs::write(data.join("entered.first"), "ready").unwrap();
                        if mode == "task" {
                            let result = tasks
                                .respond(&request, &input::form_schema(), &task_calls)
                                .unwrap();
                            println!("{}", json!({"jsonrpc":"2.0","id":id,"result":result}));
                            std::io::stdout().flush().unwrap();
                        }
                        if mode == "sent_error" {
                            println!(
                                "{}",
                                json!({"jsonrpc":"2.0","id":id,"error":{"code":-32603,"message":"fixture lost the operation result"}})
                            );
                            std::io::stdout().flush().unwrap();
                        }
                        continue;
                    }
                    let first: i32 = fs::read_to_string(data.join("pid.first"))
                        .unwrap()
                        .parse()
                        .unwrap();
                    // SAFETY: probe only the earlier proxy PID recorded by this fixture.
                    assert_ne!(
                        unsafe { libc::kill(first, 0) },
                        0,
                        "previous proxy must be reaped before another input"
                    );
                    let active = data.join("active");
                    let guard = fs::OpenOptions::new()
                        .write(true)
                        .create_new(true)
                        .open(&active)
                        .unwrap();
                    fs::write(data.join("entered.second"), "ready").unwrap();
                    if mode == "task" {
                        append(&data.join("task.events"), "second\n");
                    }
                    drop(guard);
                    fs::remove_file(active).unwrap();
                }
                if name == "fail" {
                    std::process::exit(29);
                }
                if name == "hold" {
                    fs::write(data.join("holding"), "ready").unwrap();
                    continue;
                }
                if mode == "exclusive" && name == "write" {
                    let value = request["params"]["arguments"]["value"].as_str().unwrap();
                    let active = data.join("active");
                    let guard = fs::OpenOptions::new()
                        .write(true)
                        .create_new(true)
                        .open(&active)
                        .expect("exclusive server calls must never overlap");
                    fs::write(data.join(format!("entered.{value}")), "ready").unwrap();
                    while !data.join(format!("release.{value}")).exists() {
                        std::thread::sleep(Duration::from_millis(10));
                    }
                    drop(guard);
                    fs::remove_file(active).unwrap();
                    append(&data.join("effects"), value);
                }
                if mode == "images" {
                    use base64::{Engine as _, engine::general_purpose::STANDARD};
                    use image::ImageEncoder;
                    let mut png = Vec::new();
                    image::codecs::png::PngEncoder::new(&mut png)
                        .write_image(&[1, 2, 3], 1, 1, image::ExtendedColorType::Rgb8)
                        .unwrap();
                    json!({"content":[{"type":"text","text":"Snapshot"},{"type":"image","mimeType":"image/png","data":STANDARD.encode(png)}],"structuredContent":{"nested":{"kind":"computer","data":{"untouched":true}}}})
                } else {
                    json!({"content":[{"type":"text","text":"Policy result"}]})
                }
            }
            "ping" => json!({}),
            _ => continue,
        };
        println!("{}", json!({"jsonrpc":"2.0","id":id,"result":result}));
        std::io::stdout().flush().unwrap();
    }
    append(&data.join("eof"), "x");
    if held.is_some() {
        if mode == "task" {
            let trace = tasks.trace();
            let requests = &trace["task-1"];
            let cancelled = requests
                .iter()
                .position(|(method, _)| method == "tasks/cancel")
                .expect("task cancellation must precede transport closure");
            assert!(
                requests[cancelled + 1..]
                    .iter()
                    .any(|(method, terminal)| { method == "tasks/get" && *terminal })
            );
            append(&data.join("task.events"), "eof\n");
        }
        fs::write(data.join("eof.first"), "ready").unwrap();
        while !data.join("cleanup.release").exists() {
            std::thread::sleep(Duration::from_millis(10));
        }
        drop(held);
        fs::remove_file(data.join("active")).unwrap();
    }
    if mode == "nonzero" {
        std::process::exit(31);
    }
    if mode == "timeout" {
        loop {
            std::thread::sleep(Duration::from_secs(60));
        }
    }
    std::process::exit(0);
}

fn config(mode: &str) -> Value {
    let mut value = peer::config("mcp::policies::stdio_peer", mode);
    value["reuse"] = false.into();
    value
}

fn all_reaped(data: &Path) {
    for pid in fs::read_to_string(data.join("pids")).unwrap().lines() {
        let pid: i32 = pid.parse().unwrap();
        // SAFETY: signal 0 probes only the PID recorded by this isolated fixture.
        assert_ne!(unsafe { libc::kill(pid, 0) }, 0, "MCP proxy must be reaped");
    }
}

async fn session(fixture: &mut process::Fixture) {
    let Output::Session(next) = execute(
        &fixture.client,
        Command::CreateSession {
            project: fixture.session.project,
            worktree: Some(fixture.session.worktree),
            config: Some(fixture.session.config.clone()),
        },
    )
    .await
    else {
        panic!("session expected")
    };
    fixture.session = next;
}

#[tokio::test]
async fn closes_each_turn() {
    for remote in [false, true] {
        let model = Server::turn_tools(vec![(alias("native", "read"), json!({}))]).await;
        let fixture = process::Fixture::packages(remote, &model, &[]).await;
        package(&fixture.root, json!({"native":config("normal")}), "0.1.0");
        install(&fixture, 0).await;
        let data = fixture.node.profile().join("plugins/data/example");
        for count in 1..=2 {
            let (_, turn) = submit(&fixture, false).await;
            let page = finished(&fixture.client, fixture.session.id, turn.id).await;
            assert_eq!(
                page.runs.last().unwrap().status,
                Status::Completed,
                "{page:?}"
            );
            assert!(page.runs.last().unwrap().error.is_none());
            assert_eq!(fs::read(data.join("launches")).unwrap().len(), count);
            assert_eq!(fs::read(data.join("eof")).unwrap().len(), count);
            all_reaped(&data);
        }
        fixture.node.shutdown().await.unwrap();
        fixture.controller.close().await.unwrap();
    }
}

#[tokio::test]
async fn reports_unconfirmed_cleanup() {
    for remote in [false, true] {
        for mode in ["nonzero", "timeout"] {
            let model = Server::tools(vec![(alias("native", "read"), json!({}))]).await;
            let fixture = process::Fixture::packages(remote, &model, &[]).await;
            package(&fixture.root, json!({"native":config(mode)}), "0.1.0");
            install(&fixture, 0).await;
            let (_, turn) = submit(&fixture, false).await;
            let page = finished(&fixture.client, fixture.session.id, turn.id).await;
            let run = page.runs.last().unwrap();
            assert_eq!(run.status, Status::Failed, "{remote}/{mode}: {page:?}");
            assert_eq!(run.error.as_ref().unwrap().code, ErrorCode::OutcomeUnknown);
            let data = fixture.node.profile().join("plugins/data/example");
            assert_eq!(fs::read(data.join("launches")).unwrap(), b"x");
            assert_eq!(
                fs::read_to_string(data.join("calls.jsonl"))
                    .unwrap()
                    .lines()
                    .count(),
                1
            );
            all_reaped(&data);
            fixture.node.shutdown().await.unwrap();
            fixture.controller.close().await.unwrap();
        }
    }
}

#[tokio::test]
async fn closes_failed_and_cancelled_calls() {
    for remote in [false, true] {
        for termination in ["fail", "cancel", "shutdown"] {
            let name = if termination == "fail" {
                "fail"
            } else {
                "hold"
            };
            let model = Server::tools(vec![(alias("native", name), json!({}))]).await;
            let fixture = process::Fixture::packages(remote, &model, &[]).await;
            package(&fixture.root, json!({"native":config("normal")}), "0.1.0");
            install(&fixture, 0).await;
            let (_, turn) = submit(&fixture, false).await;
            let (_, approval) = approvals::pending(&fixture.client, fixture.session.id).await;
            process::decide(&fixture.client, &approval, Decision::Approve).await;
            let data = fixture.node.profile().join("plugins/data/example");
            if termination != "fail" {
                present(&data.join("holding")).await;
            }
            if termination == "shutdown" {
                fixture.node.shutdown().await.unwrap();
            } else {
                if termination == "cancel" {
                    execute(&fixture.client, Command::StopTurn { turn: turn.id }).await;
                }
                let page = finished(&fixture.client, fixture.session.id, turn.id).await;
                assert_eq!(
                    page.runs.last().unwrap().status,
                    if termination == "fail" {
                        Status::Failed
                    } else {
                        Status::Cancelled
                    },
                    "{page:?}"
                );
                if termination == "fail" {
                    assert_eq!(
                        page.runs.last().unwrap().error.as_ref().unwrap().code,
                        ErrorCode::OutcomeUnknown
                    );
                }
                let Output::Session(current) = execute(
                    &fixture.client,
                    Command::ReadSession {
                        session: fixture.session.id,
                    },
                )
                .await
                else {
                    panic!("session expected")
                };
                execute(
                    &fixture.client,
                    Command::RemoveSession {
                        session: current.id,
                        expected_revision: current.revision,
                    },
                )
                .await;
                fixture.node.shutdown().await.unwrap();
            }
            all_reaped(&data);
            assert_eq!(
                fs::read_to_string(data.join("calls.jsonl"))
                    .unwrap()
                    .lines()
                    .count(),
                1
            );
            fixture.controller.close().await.unwrap();
        }
    }
}

#[tokio::test]
async fn serializes_installed_services() {
    for remote in [false, true] {
        let model = Server::scripted_tools(vec![(
            alias("native", "write"),
            json!({"value":"cancelled"}),
        )])
        .await;
        let mut fixture = process::Fixture::packages(remote, &model, &[]).await;
        let mut server = config("exclusive");
        server["exclusive"] = true.into();
        package(&fixture.root, json!({"native":server}), "0.1.0");
        let installed = install(&fixture, 0).await;
        let (_, cancelled) = submit(&fixture, false).await;
        let cancelled_session = fixture.session.id;
        let (_, pending) = approvals::pending(&fixture.client, cancelled_session).await;

        // Waiting for approval must not acquire the shared service lock.
        session(&mut fixture).await;
        model.replace_tools(vec![(alias("native", "write"), json!({"value":"first"}))]);
        let (_, first) = submit(&fixture, false).await;
        let first_session = fixture.session.id;
        let (_, approval) = approvals::pending(&fixture.client, first_session).await;
        process::decide(&fixture.client, &approval, Decision::Approve).await;
        let data = fixture.node.profile().join("plugins/data/example");
        present(&data.join("entered.first")).await;

        process::decide(&fixture.client, &pending, Decision::Approve).await;
        execute(&fixture.client, Command::StopTurn { turn: cancelled.id }).await;
        let page = finished(&fixture.client, cancelled_session, cancelled.id).await;
        assert_eq!(page.runs.last().unwrap().status, Status::Cancelled);

        // A read-only descriptor on another connection bypasses the service lock.
        session(&mut fixture).await;
        model.replace_tools(vec![(alias("native", "read"), json!({}))]);
        let (_, read) = submit(&fixture, false).await;
        let page = finished(&fixture.client, fixture.session.id, read.id).await;
        assert_eq!(page.runs.last().unwrap().status, Status::Completed);
        assert!(page.approvals.is_empty());
        assert!(!data.join("release.first").exists());

        // New content digest must still share the installed service's lock.
        let mut server = config("exclusive");
        server["exclusive"] = true.into();
        package(&fixture.root, json!({"native":server}), "0.1.1");
        let updated = install(&fixture, installed.summary.revision).await;
        assert_ne!(installed.summary.digest, updated.summary.digest);
        session(&mut fixture).await;
        model.replace_tools(vec![(alias("native", "write"), json!({"value":"second"}))]);
        let (_, second) = submit(&fixture, false).await;
        let (_, approval) = approvals::pending(&fixture.client, fixture.session.id).await;
        process::decide(&fixture.client, &approval, Decision::Approve).await;
        fs::write(data.join("release.first"), "ready").unwrap();
        present(&data.join("entered.second")).await;
        fs::write(data.join("release.second"), "ready").unwrap();
        let page = finished(&fixture.client, fixture.session.id, second.id).await;
        assert_eq!(
            page.runs.last().unwrap().status,
            Status::Completed,
            "{page:?}"
        );
        let page = finished(&fixture.client, first_session, first.id).await;
        assert_eq!(
            page.runs.last().unwrap().status,
            Status::Completed,
            "{page:?}"
        );
        assert_eq!(
            fs::read_to_string(data.join("effects")).unwrap(),
            "firstsecond"
        );
        assert!(!data.join("entered.cancelled").exists());
        let calls: Vec<Value> = fs::read_to_string(data.join("calls.jsonl"))
            .unwrap()
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();
        assert_eq!(
            calls.iter().filter(|call| call["name"] == "write").count(),
            2
        );
        fixture.node.shutdown().await.unwrap();
        all_reaped(&data);
        fixture.controller.close().await.unwrap();
    }
}

fn images(value: &Value) -> usize {
    match value {
        Value::Object(fields) => {
            usize::from(fields.get("type").and_then(Value::as_str) == Some("image_url"))
                + fields.values().map(images).sum::<usize>()
        }
        Value::Array(values) => values.iter().map(images).sum(),
        _ => 0,
    }
}

#[tokio::test]
async fn projects_only_selected_images() {
    for remote in [false, true] {
        let mut calls = vec![(alias("other", "read"), json!({}))];
        calls.extend((0..8).map(|_| (alias("native", "read"), json!({}))));
        let model = Server::tools(calls).await;
        let fixture = process::Fixture::packages(remote, &model, &[]).await;
        let mut retained = config("images");
        retained["keep_images"] = 3.into();
        package(
            &fixture.root,
            json!({"native":retained,"other":config("images")}),
            "0.1.0",
        );
        install(&fixture, 0).await;
        let (_, turn) = submit(&fixture, false).await;
        let page = finished(&fixture.client, fixture.session.id, turn.id).await;
        assert_eq!(
            page.runs.last().unwrap().status,
            Status::Completed,
            "{page:?}"
        );
        let view = super::super::progress::observe(&fixture.client, fixture.session.id).await;
        assert_eq!(view.calls.len(), 9);
        assert_eq!(
            view.calls
                .iter()
                .map(|call| call.images(&page).len())
                .sum::<usize>(),
            9
        );
        let canonical = serde_json::to_vec(&page).unwrap();
        let output = results(&fixture.client, &page).await;
        assert!(
            serde_json::to_string(&output)
                .unwrap()
                .contains("\"kind\":\"computer\"")
        );
        assert!(
            output
                .iter()
                .all(|result| result["structuredContent"]["nested"]
                    == json!({"kind":"computer","data":{"untouched":true}}))
        );
        {
            let requests = model.requests.lock().unwrap();
            assert_eq!(requests.len(), 10);
            assert_eq!(images(&requests[7]), 7);
            assert_eq!(images(&requests[8]), 8);
            assert_eq!(images(&requests[9]), 4);
            let snapshots = requests[9]["messages"]
                .as_array()
                .unwrap()
                .iter()
                .filter(|message| message["role"] == "tool" && message["content"] == "Snapshot")
                .count();
            assert_eq!(snapshots, 9);
            assert!(output.iter().all(|result| {
                result["structuredContent"]
                    .get("omitted_screenshots")
                    .is_none()
            }));
        }
        assert_eq!(
            serde_json::to_vec(&history(&fixture.client, fixture.session.id).await).unwrap(),
            canonical
        );
        fixture.node.shutdown().await.unwrap();
        fixture.controller.close().await.unwrap();
    }
}

#[tokio::test]
async fn preserves_discovered_annotations() {
    for remote in [false, true] {
        for platform in ["mac", "windows"] {
            let alias = alias("desktop", "get_app_state");
            let mut names = vec![
                "list_apps",
                "get_app_state",
                "click",
                "perform_secondary_action",
                "scroll",
                "drag",
                "type_text",
                "press_key",
                "set_value",
            ];
            if platform == "mac" {
                names.push("query");
            }
            let mut calls: Vec<_> = names
                .iter()
                .map(|name| {
                    (
                        "tool_search".into(),
                        json!({"query":self::alias("desktop", name),"limit":1}),
                    )
                })
                .collect();
            calls.push((alias.clone(), json!({})));
            let model = Server::tools(calls).await;
            let fixture = process::Fixture::packages(remote, &model, &[]).await;
            let mut server = config(platform);
            server["exclusive"] = true.into();
            package(&fixture.root, json!({"desktop":server}), "0.1.0");
            let path = fixture.root.join("package/plugin.json");
            let mut manifest: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
            manifest["extensions"] = json!({"dev.sailry.platform":{"api_version":"v1","scope":"host","tools":[{"name":"list_apps","server":"desktop"}]}});
            fs::write(path, serde_json::to_vec(&manifest).unwrap()).unwrap();
            let installed = install(&fixture, 0).await;
            assert!(installed.issues.is_empty(), "{:?}", installed.issues);
            let (_, turn) = submit(&fixture, false).await;
            if platform == "mac" {
                let (_, approval) = approvals::pending(&fixture.client, fixture.session.id).await;
                process::decide(&fixture.client, &approval, Decision::Approve).await;
            }
            let page = finished(&fixture.client, fixture.session.id, turn.id).await;
            assert_eq!(
                page.runs.last().unwrap().status,
                Status::Completed,
                "{page:?}"
            );
            assert_eq!(page.approvals.is_empty(), platform == "windows");
            {
                let requests = model.requests.lock().unwrap();
                assert!(requests[0]["tools"].as_array().unwrap().iter().all(|tool| {
                    !tool["function"]["name"]
                        .as_str()
                        .unwrap()
                        .starts_with("mcp_example_desktop_")
                }));
                let tools = requests[names.len()]["tools"].as_array().unwrap();
                let discovered = tools
                    .iter()
                    .filter(|tool| {
                        tool["function"]["name"]
                            .as_str()
                            .is_some_and(|name| name.starts_with("mcp_example_desktop_"))
                    })
                    .count();
                assert_eq!(discovered, if platform == "mac" { 10 } else { 9 });
                assert!(tools.iter().any(|tool| tool["function"]["name"] == alias
                    && tool["function"]["parameters"]["properties"]["value"]["type"] == "string"));
            }
            fixture.node.shutdown().await.unwrap();
            fixture.controller.close().await.unwrap();
        }
    }
}

#[tokio::test]
async fn holds_sent_calls_until_cleanup() {
    for remote in [false, true] {
        for mode in ["sent_cancel", "sent_error"] {
            let mut calls = vec![(alias("native", "write"), json!({"value":"first"}))];
            if mode == "sent_error" {
                calls.push((alias("native", "read"), json!({})));
            }
            let model = Server::scripted_tools(calls).await;
            let mut fixture = process::Fixture::packages(remote, &model, &[]).await;
            let mut server = config(mode);
            server["exclusive"] = true.into();
            package(&fixture.root, json!({"native":server}), "0.1.0");
            install(&fixture, 0).await;
            let first_session = fixture.session.id;
            let (_, first) = submit(&fixture, false).await;
            let (_, approval) = approvals::pending(&fixture.client, first_session).await;
            process::decide(&fixture.client, &approval, Decision::Approve).await;
            let data = fixture.node.profile().join("plugins/data/example");
            present(&data.join("entered.first")).await;
            if mode == "sent_cancel" {
                execute(&fixture.client, Command::StopTurn { turn: first.id }).await;
            } else {
                // The next tool in this same turn must fail without waiting on
                // its own retained guard or sending another MCP request.
                tokio::time::timeout(Duration::from_secs(3), async {
                    while model.requests.lock().unwrap().len() < 3 {
                        tokio::time::sleep(Duration::from_millis(10)).await;
                    }
                })
                .await
                .expect("closed connection blocked its own turn");
                assert_eq!(
                    fs::read_to_string(data.join("calls.jsonl"))
                        .unwrap()
                        .lines()
                        .count(),
                    1
                );
            }
            present(&data.join("eof.first")).await;
            session(&mut fixture).await;
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
                .is_err(),
                "second input ran before cleanup acknowledgement"
            );
            assert_eq!(
                fs::read_to_string(data.join("calls.jsonl"))
                    .unwrap()
                    .lines()
                    .count(),
                1
            );
            fs::write(data.join("cleanup.release"), "ready").unwrap();
            present(&data.join("entered.second")).await;
            let page = finished(&fixture.client, first_session, first.id).await;
            assert_eq!(
                page.runs.last().unwrap().status,
                if mode == "sent_cancel" {
                    Status::Cancelled
                } else {
                    Status::Completed
                },
                "{page:?}"
            );
            if mode == "sent_error" {
                assert!(
                    results(&fixture.client, &page)
                        .await
                        .iter()
                        .any(|result| result["data"]["code"] == "outcome_unknown")
                );
            }
            let page = finished(&fixture.client, fixture.session.id, second.id).await;
            assert_eq!(
                page.runs.last().unwrap().status,
                Status::Completed,
                "{page:?}"
            );
            assert!(page.runs.last().unwrap().error.is_none());
            assert_eq!(
                fs::read_to_string(data.join("calls.jsonl"))
                    .unwrap()
                    .lines()
                    .count(),
                2
            );
            all_reaped(&data);
            fixture.node.shutdown().await.unwrap();
            fixture.controller.close().await.unwrap();
        }
    }
}

#[tokio::test]
async fn settles_tasks_before_releasing_services() {
    for remote in [false, true] {
        let model =
            Server::scripted_tools(vec![(alias("native", "write"), json!({"value":"first"}))])
                .await;
        let mut fixture = process::Fixture::packages(remote, &model, &[]).await;
        let mut server = config("task");
        server["exclusive"] = true.into();
        package(&fixture.root, json!({"native":server}), "0.1.0");
        install(&fixture, 0).await;
        let first_session = fixture.session.id;
        let (_, first) = submit(&fixture, false).await;
        let (_, approval) = approvals::pending(&fixture.client, first_session).await;
        process::decide(&fixture.client, &approval, Decision::Approve).await;
        questions::pending(&fixture.client, first_session).await;
        let data = fixture.node.profile().join("plugins/data/example");

        session(&mut fixture).await;
        model.replace_tools(vec![(alias("native", "write"), json!({"value":"second"}))]);
        let (_, second) = submit(&fixture, false).await;
        let (_, approval) = approvals::pending(&fixture.client, fixture.session.id).await;
        process::decide(&fixture.client, &approval, Decision::Approve).await;
        execute(&fixture.client, Command::StopTurn { turn: first.id }).await;
        present(&data.join("task.terminal")).await;
        assert!(
            tokio::time::timeout(
                Duration::from_millis(150),
                present(&data.join("entered.second"))
            )
            .await
            .is_err(),
            "second input ran before task termination was confirmed"
        );
        assert!(!data.join("eof.first").exists());
        fs::write(data.join("task.release"), "ready").unwrap();
        present(&data.join("eof.first")).await;
        assert!(!data.join("entered.second").exists());
        assert_eq!(
            fs::read_to_string(data.join("calls.jsonl"))
                .unwrap()
                .lines()
                .count(),
            1
        );
        fs::write(data.join("cleanup.release"), "ready").unwrap();
        present(&data.join("entered.second")).await;

        let page = finished(&fixture.client, first_session, first.id).await;
        assert_eq!(page.runs.last().unwrap().status, Status::Cancelled);
        assert!(page.runs.last().unwrap().error.is_none(), "{page:?}");
        assert!(matches!(
            page.questions[0].state,
            question::State::Cancelled | question::State::Interrupted
        ));
        let page = finished(&fixture.client, fixture.session.id, second.id).await;
        assert_eq!(page.runs.last().unwrap().status, Status::Completed);
        assert!(page.runs.last().unwrap().error.is_none(), "{page:?}");
        let trace = fs::read_to_string(data.join("task.events")).unwrap();
        let events: Vec<_> = trace.lines().collect();
        let cancelled = events
            .iter()
            .position(|event| *event == "tasks/cancel:")
            .unwrap();
        let terminal = events
            .iter()
            .position(|event| *event == "tasks/get:cancelled")
            .unwrap();
        let eof = events.iter().position(|event| *event == "eof").unwrap();
        let dispatched = events.iter().position(|event| *event == "second").unwrap();
        assert!(cancelled < terminal && terminal < eof && eof < dispatched);
        assert_eq!(
            fs::read_to_string(data.join("calls.jsonl"))
                .unwrap()
                .lines()
                .count(),
            2
        );
        all_reaped(&data);
        fixture.node.shutdown().await.unwrap();
        fixture.controller.close().await.unwrap();
    }
}
