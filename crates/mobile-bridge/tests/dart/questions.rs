use super::{configuration::support, tools};
use sailry_client::Client;
use sailry_node_runtime::Node;
use serde_json::json;

#[test]
#[ignore = "requires built bridge and dart pub get in tests/mobile-contract"]
fn answers_and_cancels() {
    let directory = tempfile::tempdir().unwrap();
    std::fs::create_dir(directory.path().join("project")).unwrap();
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let server = runtime.block_on(support::Server::tools(vec![
        ("ask_user".into(), json!({"prompt": "补充要求 中文 🙂", "input": {"kind": "text", "multiline": true, "max_bytes": 64}})),
        ("ask_user".into(), json!({"prompt": "选择一项", "input": {"kind": "choice", "options": ["保留", "修改"], "multiple": false, "allow_other": true}})),
        ("ask_user".into(), json!({"prompt": "选择多项", "input": {"kind": "choice", "options": ["代码", "文档", "测试"], "multiple": true, "allow_other": true}})),
        ("ask_user".into(), json!({"prompt": "单行补充", "input": {"kind": "text", "multiline": false, "max_bytes": 100}})),
    ]));
    let (node, sessions) = runtime.block_on(tools::sessions(
        directory.path(),
        &server.endpoint,
        "ffi-questions",
    ));
    let invitation = node.link().invite().unwrap();
    let child = super::command("questions.dart", directory.path(), invitation.ticket())
        .env("SAILRY_SESSIONS", serde_json::to_string(&sessions).unwrap())
        .spawn()
        .unwrap();
    let status = super::wait(child);
    let pages = runtime.block_on(tools::pages(&Client::new(node.local()), &sessions));
    runtime.block_on(node.shutdown()).unwrap();
    assert!(status.success());
    let node = runtime
        .block_on(Node::start(directory.path().join("node")))
        .unwrap();
    assert_eq!(
        runtime.block_on(tools::pages(&Client::new(node.local()), &sessions)),
        pages
    );
    runtime.block_on(node.shutdown()).unwrap();
    let requests = server.requests.lock().unwrap();
    assert_eq!(requests.len(), 6);
    assert!(
        requests
            .iter()
            .all(|request| request["model"] == "ffi-questions")
    );
    let results: Vec<serde_json::Value> = requests[4]["messages"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|message| message["role"] == "tool")
        .map(|message| serde_json::from_str(message["content"].as_str().unwrap()).unwrap())
        .collect();
    assert_eq!(
        results,
        vec![
            json!({"status": "answered", "answer": " 第一行 中文 🙂\nSecond line "}),
            json!({"status": "answered", "answers": [" 自定义 中文 🙂 "]}),
            json!({"status": "answered", "answers": ["代码", "测试", "其他 🙂"]}),
            json!({"status": "cancelled"}),
        ]
    );
}
