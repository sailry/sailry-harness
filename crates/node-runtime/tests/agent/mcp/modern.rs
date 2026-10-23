use super::*;
use base64::{Engine as _, engine::general_purpose::STANDARD};
use image::ImageEncoder;
use question::{Answer, Response, State};
use rmcp::model::{
    CacheScope, CallToolResult, ContentBlock, DiscoverResult, ElicitRequest, ElicitRequestParams,
    Implementation, InputRequest, InputRequests, InputRequiredResult, ListToolsResult,
    ProtocolVersion, ServerCapabilities, Tool, ToolAnnotations,
};
use std::io::{BufRead, Write};

const REQUEST_STATE: &str = "report.input/with+opaque=state \"quoted\"";

// This filtered subprocess uses SDK result types while the application owns
// discovery, MRTR rounds, questions and cancellation through its real service.
#[test]
#[ignore = "launched only as an isolated MCP subprocess"]
fn stdio_peer() {
    assert_eq!(std::env::var("SAILRY_MCP_FIXTURE").as_deref(), Ok("1"));
    println!();
    let data = std::env::var("PLUGIN_DATA").unwrap();
    let data = Path::new(&data);
    let root = std::env::var("PLUGIN_ROOT").unwrap();
    let mode = std::env::var("SAILRY_MCP_MODE").unwrap();
    fs::write(data.join("pid"), std::process::id().to_string()).unwrap();
    fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(data.join("launches"))
        .unwrap()
        .write_all(b"x")
        .unwrap();
    for line in std::io::stdin().lock().lines() {
        let Ok(line) = line else { break };
        let Ok(request) = serde_json::from_str::<Value>(&line) else {
            continue;
        };
        writeln!(
            fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(data.join("requests.jsonl"))
                .unwrap(),
            "{request}"
        )
        .unwrap();
        let method = request["method"].as_str().unwrap_or("");
        assert_ne!(method, "initialize", "modern peer must use discovery");
        assert_ne!(method, "notifications/initialized");
        if request["id"].is_null() {
            continue;
        }
        let meta = &request["params"]["_meta"];
        assert_eq!(
            meta["io.modelcontextprotocol/protocolVersion"],
            serde_json::to_value(ProtocolVersion::LATEST).unwrap()
        );
        assert!(meta["io.modelcontextprotocol/clientInfo"].is_object());
        assert!(meta["io.modelcontextprotocol/clientCapabilities"].is_object());
        let result = match method {
            "server/discover" => serde_json::to_value(
                DiscoverResult::new(
                    vec![ProtocolVersion::LATEST],
                    ServerCapabilities::builder().enable_tools().build(),
                )
                .with_server_info(Implementation::new("modern-fixture", "1")),
            )
            .unwrap(),
            "tools/list" => {
                let name = if mode == "content" { "report" } else { "read" };
                let mut tool = Tool::new(
                    name,
                    "Read the captured report",
                    serde_json::Map::from_iter([
                        ("type".into(), json!("object")),
                        ("properties".into(), json!({})),
                    ]),
                );
                let mut annotations = ToolAnnotations::new();
                annotations.read_only_hint = Some(true);
                tool.annotations = Some(annotations);
                let listed = ListToolsResult::with_all_items(vec![tool])
                    .with_ttl_ms(0)
                    .with_cache_scope(CacheScope::Private);
                serde_json::to_value(listed).unwrap()
            }
            "tools/call" if matches!(mode.as_str(), "content" | "images") => {
                assert_eq!(
                    request["params"]["name"],
                    if mode == "content" { "report" } else { "read" }
                );
                let mut png = Vec::new();
                image::codecs::png::PngEncoder::new(&mut png)
                    .write_image(
                        &[0x38, 0x98, 0x70, 0x38, 0x98, 0x70],
                        2,
                        1,
                        image::ExtendedColorType::Rgb8,
                    )
                    .unwrap();
                let mut result = CallToolResult::success(vec![ContentBlock::image(
                    STANDARD.encode(png),
                    "image/png",
                )]);
                if mode == "content" {
                    let content: Value = serde_json::from_slice(
                        &fs::read(Path::new(&root).join("content.json")).unwrap(),
                    )
                    .unwrap();
                    result.content.insert(
                        0,
                        ContentBlock::text("Sample report: hello.txt updated; notes.txt unchanged"),
                    );
                    result.structured_content = Some(json!({
                        "sailry_content":content,"sample":true,"isError":true,"error":"domain row"
                    }));
                }
                serde_json::to_value(result).unwrap()
            }
            "tools/call" => {
                assert_eq!(request["params"]["name"], "read");
                if request["params"]["inputResponses"].is_null() {
                    assert!(request["params"]["requestState"].is_null());
                    let mut requests = InputRequests::new();
                    requests.insert(
                        "report-form".into(),
                        InputRequest::Elicitation(ElicitRequest::new(
                            ElicitRequestParams::FormElicitationParams {
                                meta: None,
                                message: "Choose how to prepare the report".into(),
                                requested_schema: serde_json::from_value(peer::form_schema())
                                    .unwrap(),
                            },
                        )),
                    );
                    serde_json::to_value(InputRequiredResult::new(
                        Some(requests),
                        Some(REQUEST_STATE.into()),
                    ))
                    .unwrap()
                } else {
                    assert_eq!(request["params"]["requestState"], REQUEST_STATE);
                    let answer = &request["params"]["inputResponses"]["report-form"];
                    assert!(matches!(
                        answer["action"].as_str(),
                        Some("accept" | "cancel")
                    ));
                    let mut result =
                        CallToolResult::success(vec![ContentBlock::text("Input received")]);
                    result.structured_content = Some(answer.clone());
                    serde_json::to_value(result).unwrap()
                }
            }
            _ => panic!("unexpected modern fixture method: {method}"),
        };
        println!(
            "{}",
            json!({"jsonrpc":"2.0","id":request["id"],"result":result})
        );
        std::io::stdout().flush().unwrap();
    }
    std::process::exit(0);
}

fn requests(data: &Path) -> Vec<Value> {
    fs::read_to_string(data.join("requests.jsonl"))
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect()
}

pub(super) fn assert_lifecycle(data: &Path) {
    let requests = requests(data);
    assert_eq!(requests[0]["method"], "server/discover");
    assert!(requests.iter().all(|request| !matches!(
        request["method"].as_str(),
        Some("initialize" | "notifications/initialized" | "elicitation/create")
    )));
    assert_eq!(fs::read_to_string(data.join("launches")).unwrap(), "x");
}

async fn fixture(remote: bool, model: &Server) -> process::Fixture {
    let fixture = process::Fixture::packages(remote, model, &[]).await;
    package(
        &fixture.root,
        json!({"native":peer::config("mcp::modern::stdio_peer", "input")}),
        "1.0.0",
    );
    let info = install(&fixture, 0).await;
    assert!(info.issues.is_empty(), "{:?}", info.issues);
    fixture
}

fn values() -> Value {
    json!({"title":"Report 中文🙂","copies":2,"ratio":1.5,"publish":false,"format":"md","sections":["summary","details"]})
}

#[tokio::test]
async fn resumes_input() {
    for remote in [false, true] {
        for cancel in [false, true] {
            let model = Server::tools(vec![(alias("native", "read"), json!({}))]).await;
            let fixture = fixture(remote, &model).await;
            let (_, turn) = submit(&fixture, false).await;
            let (page, question) = questions::pending(&fixture.client, fixture.session.id).await;
            assert!(page.approvals.is_empty());
            assert_eq!(model.requests.lock().unwrap().len(), 1);
            execute(
                &fixture.client,
                Command::ResolveQuestion {
                    session: fixture.session.id,
                    question: question.id,
                    response: if cancel {
                        Response::Cancel
                    } else {
                        Response::Answer(Answer::Form(values().as_object().unwrap().clone()))
                    },
                },
            )
            .await;
            let page = finished(&fixture.client, fixture.session.id, turn.id).await;
            assert_eq!(page.runs[0].status, Status::Completed, "{page:?}");
            assert_eq!(page.questions.len(), 1);
            let output: Vec<_> = page
                .entries
                .iter()
                .flat_map(|entry| &entry.parts)
                .filter_map(|part| match part {
                    Part::ToolResult { name, result, .. } if name == &alias("native", "read") => {
                        Some(result)
                    }
                    _ => None,
                })
                .collect();
            assert_eq!(output.len(), 1, "{output:?}");
            if cancel {
                assert_eq!(page.questions[0].state, State::Cancelled);
                assert_eq!(output[0]["output"]["action"], "cancel");
            } else {
                assert!(matches!(page.questions[0].state, State::Answered(_)));
                assert_eq!(
                    output[0]["output"],
                    json!({"action":"accept","content":values()})
                );
            }
            assert_eq!(model.requests.lock().unwrap().len(), 2);
            let data = fixture.node.profile().join("plugins/data/example");
            assert_lifecycle(&data);
            assert_eq!(
                requests(&data)
                    .iter()
                    .filter(|request| request["method"] == "tools/call")
                    .count(),
                2
            );
            fixture.node.shutdown().await.unwrap();
            reaped(&data);
            fixture.controller.close().await.unwrap();
        }
    }
}

#[tokio::test]
async fn cancels_pending_input() {
    for remote in [false, true] {
        let model = Server::tools(vec![(alias("native", "read"), json!({}))]).await;
        let fixture = fixture(remote, &model).await;
        let (_, turn) = submit(&fixture, false).await;
        let (_, question) = questions::pending(&fixture.client, fixture.session.id).await;
        execute(&fixture.client, Command::StopTurn { turn: turn.id }).await;
        let page = finished(&fixture.client, fixture.session.id, turn.id).await;
        assert_eq!(page.runs[0].status, Status::Cancelled, "{page:?}");
        assert!(matches!(
            page.questions[0].state,
            State::Cancelled | State::Interrupted
        ));
        assert_eq!(
            fixture
                .client
                .execute(fixture.client.prepare(Command::ResolveQuestion {
                    session: fixture.session.id,
                    question: question.id,
                    response: Response::Answer(Answer::Form(values().as_object().unwrap().clone())),
                }))
                .await
                .unwrap_err()
                .code,
            ErrorCode::Conflict
        );
        assert_eq!(model.requests.lock().unwrap().len(), 1);
        let data = fixture.node.profile().join("plugins/data/example");
        assert_lifecycle(&data);
        reaped(&data);
        fixture.node.shutdown().await.unwrap();
        fixture.controller.close().await.unwrap();
    }
}
