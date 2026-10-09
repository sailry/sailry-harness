use super::*;

#[tokio::test]
#[ignore = "requires SAILRY_TEST_PYTHON_RUNTIME pointing at verified bundled Python"]
async fn launches_the_bundled_interpreter() {
    let runtime = std::path::PathBuf::from(
        std::env::var_os("SAILRY_TEST_PYTHON_RUNTIME")
            .expect("SAILRY_TEST_PYTHON_RUNTIME is required"),
    );
    for remote in [false, true] {
        let model = Server::turn_tools(vec![(alias("native", "read"), json!({}))]).await;
        let fixture = process::Fixture::new(remote, &model).await;
        fixture.node.set_python_runtime_fixture(&runtime).unwrap();
        isolate(&fixture.client).await;
        package(
            &fixture.root,
            json!({"native":{"type":"stdio", "command":"python3", "args":["${PLUGIN_ROOT}/server.py"], "cwd":"${PLUGIN_ROOT}"}}),
            "1.0.0",
        );
        fs::write(
            fixture.root.join("package/server.py"),
            include_str!("python/server.py"),
        )
        .unwrap();
        let installed = install(&fixture, 0).await;
        assert!(
            installed.issues.is_empty(),
            "MCP issues: {:?}",
            installed.issues
        );
        assert_eq!(
            installed.mcp,
            [plugin::Mcp {
                name: "native".into(),
                transport: plugin::McpTransport::Stdio,
            }]
        );
        let (_, turn) = submit(&fixture, false).await;
        let page = finished(&fixture.client, fixture.session.id, turn.id).await;
        assert_eq!(page.runs.last().unwrap().status, Status::Completed);
        let trace = trace(
            &fixture
                .node
                .profile()
                .join("plugins/data/example/python-trace.jsonl"),
        );
        let request = model
            .requests
            .lock()
            .unwrap()
            .first()
            .cloned()
            .expect("model request");
        let names: Vec<_> = request["tools"]
            .as_array()
            .expect("model tools")
            .iter()
            .filter_map(|tool| {
                tool["function"]["name"]
                    .as_str()
                    .or_else(|| tool["name"].as_str())
            })
            .collect();
        assert!(
            names.contains(&alias("native", "read").as_str()),
            "MCP tools: {names:?}; fixture trace: {trace}"
        );
        let returned = results(&page);
        assert_eq!(returned.len(), 1, "MCP results: {returned:?}");
        assert!(
            returned[0].get("error").is_none(),
            "MCP result: {}; fixture trace: {trace}",
            returned[0]
        );
        let output = &returned.last().expect("MCP result returned")["output"];
        assert_eq!(
            output["version"],
            json!([3, 12, 13]),
            "MCP result: {}",
            returned[0]
        );
        let python = std::path::Path::new(output["python"].as_str().expect("interpreter returned"));
        assert!(python.starts_with(runtime.canonicalize().unwrap()));
        let path = output["path"].as_str().expect("MCP PATH returned");
        assert_eq!(
            std::env::split_paths(path).next().unwrap(),
            python.parent().unwrap()
        );
        fixture.node.shutdown().await.unwrap();
        fixture.controller.close().await.unwrap();
    }
}

fn trace(path: &Path) -> String {
    use std::io::Read;
    let Ok(file) = fs::File::open(path) else {
        return "fixture did not start".into();
    };
    let mut bytes = Vec::new();
    file.take(8192).read_to_end(&mut bytes).unwrap();
    String::from_utf8(bytes).expect("Python fixture trace is UTF-8")
}
