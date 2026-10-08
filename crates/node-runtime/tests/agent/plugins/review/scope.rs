use super::*;
use std::collections::BTreeSet;
use std::os::unix::fs::PermissionsExt;

fn mixed(root: &Path) -> (Repository, BTreeSet<String>) {
    let repository = Repository::init(root).unwrap();
    let files = [
        (
            "src/lib.rs",
            "pub fn value() -> u32 { 1 }\n",
            "pub fn value() -> u32 { 2 }\n",
        ),
        (
            "tests/lib.rs",
            "assert_eq!(value(), 1);\n",
            "assert_eq!(value(), 2);\n",
        ),
        (
            "web/src/component.ts",
            "export const value = 1;\n",
            "export const value = 2;\n",
        ),
        (
            "web/tests/component.test.ts",
            "expect(value).toBe(1);\n",
            "expect(value).toBe(2);\n",
        ),
        ("python/src/task.py", "VALUE = 1\n", "VALUE = 2\n"),
        (
            "python/tests/test_task.py",
            "assert VALUE == 1\n",
            "assert VALUE == 2\n",
        ),
        ("locales/en.yml", "value: First\n", "value: Second\n"),
        ("locales/zh-CN.yml", "value: 第一\n", "value: 第二\n"),
        (
            "api/query.graphql",
            "type Query { value: Int }\n",
            "type Query { value: String }\n",
        ),
        (
            "db/schema.prisma",
            "model Item { id Int @id }\n",
            "model Item { id String @id }\n",
        ),
        ("scripts/task.sh", "printf first\n", "printf second\n"),
        ("config/settings.json", "{\"value\":1}\n", "{\"value\":2}\n"),
        (
            "go/task.go",
            "package task\nconst Value = 1\n",
            "package task\nconst Value = 2\n",
        ),
        ("ui/view.dart", "const value = 1;\n", "const value = 2;\n"),
    ];
    for (path, before, _) in files {
        write(root, path, before);
    }
    write(root, "assets/image.bin", b"original\0image");
    commit(&repository, "Initial sources");
    for (path, _, after) in files {
        write(root, path, after);
    }
    write(root, "assets/image.bin", b"modified\0image");
    for path in [".env", ".npmrc"] {
        write(root, path, "review-secret-must-never-reach-the-model");
        std::fs::set_permissions(root.join(path), std::fs::Permissions::from_mode(0o0)).unwrap();
    }
    for path in [
        ".ssh/id_ed25519",
        "generated/api.rs",
        "node_modules/lib/index.js",
        "Cargo.lock",
        "dist/output.js",
        "web/bundle.min.js",
        "ui/generated.g.dart",
    ] {
        write(root, path, "Skipped fixture content\n");
    }
    (
        repository,
        files.into_iter().map(|(path, _, _)| path.into()).collect(),
    )
}

#[tokio::test]
async fn covers_the_full_filtered_scope() {
    for remote in [false, true] {
        let (mut fixture, _) = start(remote).await;
        let (repository, expected) = mixed(&fixture.root);
        let index = std::fs::read(repository.path().join("index")).unwrap();
        let (created, server) = calls(
            &fixture,
            vec![(
                plugin_tool(PACKAGE, "review_scope"),
                json!({"target":{"kind":"worktree","data":{"scope":"all"}}}),
            )],
        )
        .await;
        let scope = &created[0];
        success(scope);
        let files: BTreeSet<_> = scope["files"]
            .as_array()
            .unwrap()
            .iter()
            .map(|file| file["path"].as_str().unwrap().to_owned())
            .collect();
        assert_eq!(files, expected);
        assert!(files.len() > 10);
        let skipped: BTreeMap<_, _> = scope["skipped"]
            .as_array()
            .unwrap()
            .iter()
            .map(|file| {
                (
                    file["path"].as_str().unwrap(),
                    file["reason"].as_str().unwrap(),
                )
            })
            .collect();
        for path in [".env", ".npmrc", ".ssh/id_ed25519"] {
            assert_eq!(skipped[path], "credentials");
        }
        for path in ["generated/api.rs", "ui/generated.g.dart"] {
            assert_eq!(skipped[path], "generated output");
        }
        assert_eq!(skipped["node_modules/lib/index.js"], "dependencies");
        assert_eq!(skipped["Cargo.lock"], "lockfile");
        assert_eq!(skipped["dist/output.js"], "build output");
        assert_eq!(skipped["web/bundle.min.js"], "minified output");
        assert!(skipped.contains_key("assets/image.bin"));
        let group = |path: &str| {
            scope["groups"]
                .as_array()
                .unwrap()
                .iter()
                .find(|group| {
                    group["files"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .any(|file| file == path)
                })
                .unwrap()["id"]
                .clone()
        };
        assert_eq!(group("src/lib.rs"), group("tests/lib.rs"));
        assert_eq!(
            group("web/src/component.ts"),
            group("web/tests/component.test.ts")
        );
        assert_eq!(
            group("python/src/task.py"),
            group("python/tests/test_task.py")
        );
        assert_eq!(group("locales/en.yml"), group("locales/zh-CN.yml"));
        for (path, rule) in [
            ("src/lib.rs", "rust.md"),
            ("python/src/task.py", "python.md"),
            ("web/src/component.ts", "ts_js_tsx_jsx.md"),
            ("locales/en.yml", "yaml.md"),
            ("scripts/task.sh", "default.md"),
        ] {
            assert_eq!(file(scope, path)["rule"]["file"], rule);
            assert_eq!(file(scope, path)["rule"]["path"], format!("rules/{rule}"));
        }
        let page = call(
            &fixture,
            "review_scope",
            json!({"review_id":scope["review_id"],"offset":0,"limit":3}),
        )
        .await;
        success(&page);
        assert_eq!(
            page["files"],
            Value::Array(scope["files"].as_array().unwrap()[..3].to_vec())
        );
        assert_eq!(page["next"], 3);
        let tail = call(
            &fixture,
            "review_scope",
            json!({"review_id":scope["review_id"],"offset":3,"limit":100}),
        )
        .await;
        success(&tail);
        assert_eq!(
            tail["files"],
            Value::Array(scope["files"].as_array().unwrap()[3..].to_vec())
        );
        assert_eq!(tail["groups"], scope["groups"]);
        assert_eq!(tail["skipped"], scope["skipped"]);
        assert!(tail["next"].is_null());
        let partial = call(&fixture,"review_complete",json!({
            "review_id":scope["review_id"],"reviewed_files":[],"reviewed_groups":[],"finalize":true,
        })).await;
        failure(&partial, "invalid_request");
        assert!(partial.get("remaining").is_some(), "{partial}");
        let member = file(scope, "src/lib.rs");
        let incomplete_group = json!({"review_id":scope["review_id"],
            "reviewed_files":[{"path":member["path"],"revision":member["revision"]}],
            "reviewed_groups":[member["group"]],"finalize":false});
        failure(
            &call(&fixture, "review_complete", incomplete_group).await,
            "invalid_request",
        );
        let mut duplicate = receipts(scope, false);
        let first = duplicate["reviewed_files"][0].clone();
        duplicate["reviewed_files"]
            .as_array_mut()
            .unwrap()
            .push(first);
        failure(
            &call(&fixture, "review_complete", duplicate).await,
            "invalid_request",
        );
        let mut unknown = receipts(scope, false);
        unknown["reviewed_groups"]
            .as_array_mut()
            .unwrap()
            .push(json!("group-unknown"));
        failure(
            &call(&fixture, "review_complete", unknown).await,
            "invalid_request",
        );
        let mut stale = receipts(scope, false);
        stale["reviewed_files"][0]["revision"] = json!("stale-fixture-revision");
        failure(
            &call(&fixture, "review_complete", stale).await,
            "revision_conflict",
        );
        fixture = restart(fixture, remote).await;
        let resumed = call(
            &fixture,
            "review_scope",
            json!({"review_id":scope["review_id"]}),
        )
        .await;
        success(&resumed);
        assert_eq!(resumed["files"], scope["files"]);
        assert_eq!(resumed["groups"], scope["groups"]);
        let complete = call(&fixture, "review_complete", receipts(scope, true)).await;
        success(&complete);
        assert_eq!(complete["status"], "complete");
        assert_eq!(complete["findings"], 0);
        assert!(
            complete["remaining"]["files"]
                .as_array()
                .unwrap()
                .is_empty()
        );
        assert!(
            complete["remaining"]["groups"]
                .as_array()
                .unwrap()
                .is_empty()
        );
        assert_eq!(
            std::fs::read(repository.path().join("index")).unwrap(),
            index
        );
        assert_eq!(
            std::fs::read_to_string(fixture.root.join("src/lib.rs")).unwrap(),
            "pub fn value() -> u32 { 2 }\n"
        );
        assert!(
            !scope
                .to_string()
                .contains("review-secret-must-never-reach-the-model")
        );
        for request in server.requests.lock().unwrap().iter() {
            assert!(
                !request
                    .to_string()
                    .contains("review-secret-must-never-reach-the-model")
            );
        }
        let second =
            fixture::scope(&fixture, json!({"kind":"worktree","data":{"scope":"all"}})).await;
        assert_ne!(second["review_id"], scope["review_id"]);
        stop(fixture).await;
    }
}

#[tokio::test]
async fn loads_original_language_rules() {
    for remote in [false, true] {
        let (fixture, _) = start(remote).await;
        let directory = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../plugins/code-review/skills/code-review/rules");
        let mut rules: Vec<_> = std::fs::read_dir(&directory)
            .unwrap()
            .map(|entry| entry.unwrap().file_name().to_str().unwrap().to_owned())
            .filter(|name| name != "index.md")
            .collect();
        rules.sort();
        assert_eq!(rules.len(), 54);
        let mut requests = vec![(
            "load_skill".into(),
            json!({"skill":"code-review:code-review"}),
        )];
        for path in std::iter::once("index.md").chain(rules.iter().map(String::as_str)) {
            requests.push((
                "read_skill_resource".into(),
                json!({"skill":"code-review:code-review","path":format!("rules/{path}")}),
            ));
        }
        let remaining = requests.split_off(32);
        let server = Server::batch(requests).await;
        route(&fixture.client, &fixture.session, &server.endpoint).await;
        let turn = submit(&fixture.client, &fixture.session).await;
        let page = completed(&fixture.client, fixture.session.id, turn.id).await;
        let mut results: Vec<_> = crate::approvals::rules::results(&page, turn.id)
            .into_iter()
            .map(|(_, _, result)| result)
            .collect();
        results.extend(calls(&fixture, remaining).await.0);
        assert_eq!(results.len(), 56);
        for result in &results {
            assert!(result.get("error").is_none(), "{result}");
        }
        let resources: BTreeMap<_, _> = results
            .iter()
            .map(|result| (result["path"].as_str().unwrap(), &result["content"]))
            .collect();
        assert_eq!(resources.len(), 56);
        for path in std::iter::once("index.md").chain(rules.iter().map(String::as_str)) {
            assert_eq!(
                *resources[format!("rules/{path}").as_str()],
                std::fs::read_to_string(directory.join(path)).unwrap()
            );
        }
        assert!(
            server.requests.lock().unwrap().first().unwrap()["messages"]
                .to_string()
                .contains("code-review:code-review")
        );
        stop(fixture).await;
    }
}
