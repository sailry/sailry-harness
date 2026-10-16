use super::*;

pub(super) const PACKAGE: &str = "code-review";

pub(super) async fn start(remote: bool) -> (Fixture, plugin::Info) {
    let server = Server::turn_tools(vec![]).await;
    let mut fixture = Fixture::packages(remote, &server, &["code-review"]).await;
    let Output::Plugin(current) = execute(
        &fixture.client,
        Command::ReadPlugin {
            name: PACKAGE.into(),
        },
    )
    .await
    else {
        panic!("plugin expected")
    };
    let Output::Plugin(mut package) = execute(
        &fixture.client,
        Command::InstallBundledPlugin {
            name: PACKAGE.into(),
            expected_revision: current.summary.revision,
        },
    )
    .await
    else {
        panic!("installed plugin expected")
    };
    assert!(package.issues.is_empty(), "{:?}", package.issues);
    if !package.summary.enabled {
        let Output::Plugin(enabled) = execute(
            &fixture.client,
            Command::SetPluginEnabled {
                name: PACKAGE.into(),
                expected_revision: package.summary.revision,
                enabled: true,
            },
        )
        .await
        else {
            panic!("enabled plugin expected")
        };
        package = enabled;
    }
    planning::configure(&mut fixture, WorkMode::Code).await;
    (fixture, package)
}

pub(super) async fn calls(fixture: &Fixture, calls: Vec<(String, Value)>) -> (Vec<Value>, Server) {
    calls_in(fixture, &fixture.session, calls).await
}

pub(super) async fn calls_in(
    fixture: &Fixture,
    session: &Session,
    calls: Vec<(String, Value)>,
) -> (Vec<Value>, Server) {
    let server = Server::turn_tools(calls.clone()).await;
    route(&fixture.client, session, &server.endpoint).await;
    let turn = submit(&fixture.client, session).await;
    let page = completed(&fixture.client, session.id, turn.id).await;
    let results = crate::approvals::rules::results(&page, turn.id);
    assert_eq!(results.len(), calls.len(), "{page:?}");
    for ((_, name, _), (expected, _)) in results.iter().zip(&calls) {
        assert_eq!(name, expected);
    }
    (
        results.into_iter().map(|(_, _, result)| result).collect(),
        server,
    )
}

pub(super) async fn call(fixture: &Fixture, tool: &str, arguments: Value) -> Value {
    calls(fixture, vec![(plugin_tool(PACKAGE, tool), arguments)])
        .await
        .0
        .remove(0)
}

pub(super) fn success(result: &Value) {
    assert_ne!(result["isError"], true, "{result}");
    assert!(result.get("error").is_none(), "{result}");
}

pub(super) fn failure(result: &Value, code: &str) {
    assert_eq!(result["isError"], true, "{result}");
    assert_eq!(result["error"]["code"], code, "{result}");
}

pub(super) async fn scope(fixture: &Fixture, target: Value) -> Value {
    let result = call(fixture, "review_scope", json!({"target":target})).await;
    success(&result);
    result
}

pub(super) fn write(root: &Path, path: &str, text: impl AsRef<[u8]>) {
    let path = root.join(path);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, text).unwrap();
}

pub(super) fn commit(repository: &Repository, message: &str) -> Oid {
    let mut index = repository.index().unwrap();
    index
        .add_all(["*"], git2::IndexAddOption::DEFAULT, None)
        .unwrap();
    index.write().unwrap();
    let tree = repository.find_tree(index.write_tree().unwrap()).unwrap();
    let author = Signature::now("Fixture", "fixture@example.invalid").unwrap();
    let parent = repository
        .head()
        .ok()
        .map(|head| head.peel_to_commit().unwrap());
    let parents: Vec<_> = parent.iter().collect();
    repository
        .commit(Some("HEAD"), &author, &author, message, &tree, &parents)
        .unwrap()
}

pub(super) fn file<'a>(scope: &'a Value, path: &str) -> &'a Value {
    scope["files"]
        .as_array()
        .unwrap()
        .iter()
        .find(|file| file["path"] == path)
        .unwrap_or_else(|| panic!("missing {path}: {scope}"))
}

pub(super) fn finding(scope: &Value, path: &str, side: &str, line: usize, snippet: &str) -> Value {
    json!({
        "review_id":scope["review_id"],"path":path,"revision":file(scope,path)["revision"],
        "side":side,"line":line,"snippet":snippet,"severity":"P1",
        "title":"Incorrect changed value","explanation":"The changed value violates the caller contract",
        "evidence":"The changed line is the value returned to the caller",
    })
}

pub(super) fn receipts(scope: &Value, finalize: bool) -> Value {
    json!({
        "review_id":scope["review_id"],
        "reviewed_files":scope["files"].as_array().unwrap().iter()
            .map(|file| json!({"path":file["path"],"revision":file["revision"]})).collect::<Vec<_>>(),
        "reviewed_groups":scope["groups"].as_array().unwrap().iter()
            .map(|group| group["id"].clone()).collect::<Vec<_>>(),
        "finalize":finalize,
    })
}

pub(super) async fn value(
    fixture: &Fixture,
    package: &plugin::Info,
    key: &str,
) -> plugin::storage::Entry {
    let request = fixture
        .client
        .prepare(Command::ReadPluginValue { key: key.into() })
        .with_plugin(plugin::Context {
            invocation: None,
            turn: None,
            surface: Default::default(),
            package: package.summary.reference(),
            worktree: Some(fixture.session.worktree),
            session: Some(fixture.session.id),
        });
    let Output::PluginValue(entry) = fixture.client.execute(request).await.unwrap() else {
        panic!("plugin value expected")
    };
    entry
}

pub(super) async fn permission(fixture: &mut Fixture, permission: Permission) {
    let mut config = fixture.session.config.clone();
    config.permission = permission;
    let Output::Session(session) = execute(
        &fixture.client,
        Command::SetSessionConfig {
            session: fixture.session.id,
            expected_revision: fixture.session.revision,
            config,
        },
    )
    .await
    else {
        panic!("session expected")
    };
    fixture.session = session;
}

pub(super) async fn stop(fixture: Fixture) {
    fixture.node.shutdown().await.unwrap();
    fixture.controller.close().await.unwrap();
}
