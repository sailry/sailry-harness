use super::*;
use std::fs::FileTimes;
use std::os::unix::fs::{MetadataExt, PermissionsExt};

#[tokio::test]
async fn preserves_published_working_snapshots() {
    for remote in [false, true] {
        for change in [
            "head",
            "index",
            "mode",
            "content",
            "same-stat",
            "new-candidate",
        ] {
            let (mut fixture, package) = start(remote).await;
            let repository = Repository::init(&fixture.root).unwrap();
            write(&fixture.root, "src/value.rs", "original_value();\n");
            write(&fixture.root, "other/clean.rs", "initial_value();\n");
            commit(&repository, "Initial snapshot");
            write(&fixture.root, "src/value.rs", "changed_value();\n");
            let scope = scope(&fixture, json!({"kind":"worktree","data":{"scope":"all"}})).await;
            let key = format!("review/{}/head", scope["review_id"].as_str().unwrap());
            let before = value(&fixture, &package, &key).await;
            assert!(before.present);
            let reads = || {
                ["diff", "old", "new"].into_iter().map(|part| (
                plugin_tool(PACKAGE, "read_review_change"),
                json!({"review_id":scope["review_id"],"path":"src/value.rs","part":part}),
            )).collect()
            };
            let (saved, _) = calls(&fixture, reads()).await;
            for content in &saved {
                success(content);
            }
            assert_eq!(saved[1]["text"], "original_value();\n");
            assert_eq!(saved[2]["text"], "changed_value();\n");
            let path = fixture.root.join("src/value.rs");
            match change {
                "head" => {
                    commit(&repository, "Moved HEAD");
                }
                "index" => {
                    let mut index = repository.index().unwrap();
                    index.add_path(Path::new("src/value.rs")).unwrap();
                    index.write().unwrap();
                }
                "mode" => {
                    let mode = std::fs::metadata(&path).unwrap().permissions().mode();
                    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(mode ^ 0o100))
                        .unwrap();
                }
                "content" => {
                    write(&fixture.root, "src/value.rs", "different_longer_value();\n");
                }
                "same-stat" | "new-candidate" => {
                    let path = if change == "new-candidate" {
                        fixture.root.join("other/clean.rs")
                    } else {
                        path.clone()
                    };
                    let original = std::fs::metadata(&path).unwrap();
                    std::fs::write(&path, "another_value();\n").unwrap();
                    let file = std::fs::File::open(&path).unwrap();
                    file.set_times(
                        FileTimes::new()
                            .set_accessed(original.accessed().unwrap())
                            .set_modified(original.modified().unwrap()),
                    )
                    .unwrap();
                    let changed = std::fs::metadata(&path).unwrap();
                    assert_eq!(changed.len(), original.len());
                    assert_eq!(changed.modified().unwrap(), original.modified().unwrap());
                    assert_eq!(changed.ino(), original.ino());
                    assert_eq!(changed.dev(), original.dev());
                    assert_eq!(changed.mode(), original.mode());
                }
                _ => unreachable!(),
            }
            let report = call(
                &fixture,
                "report_finding",
                finding(&scope, "src/value.rs", "new", 1, "changed_value();"),
            )
            .await;
            success(&report);
            assert_eq!(report["finding"]["line"], 1);
            let (retained, _) = calls(&fixture, reads()).await;
            assert_eq!(retained, saved, "change={change} remote={remote}");
            fixture = restart(fixture, remote).await;
            let (reopened, _) = calls(&fixture, reads()).await;
            assert_eq!(reopened, saved, "change={change} remote={remote}");
            let fresh =
                fixture::scope(&fixture, json!({"kind":"worktree","data":{"scope":"all"}})).await;
            assert_ne!(fresh["review_id"], scope["review_id"]);
            if change == "head" {
                assert_eq!(fresh["counts"]["files"], 0);
            } else if change == "index" {
                assert_ne!(
                    fresh["snapshot"]["target"]["data"]["index_revision"],
                    scope["snapshot"]["target"]["data"]["index_revision"],
                    "change={change} remote={remote}",
                );
                assert_ne!(fresh["snapshot"], scope["snapshot"]);
                // Staging preserves the HEAD-to-working-tree bytes in an all-scope review.
                let (current, _) = calls(
                    &fixture,
                    ["diff", "old", "new"]
                        .into_iter()
                        .map(|part| {
                            (
                                plugin_tool(PACKAGE, "read_review_change"),
                                json!({"review_id":fresh["review_id"],"path":"src/value.rs","part":part}),
                            )
                        })
                        .collect(),
                )
                .await;
                for (current, saved) in current.iter().zip(&saved) {
                    success(current);
                    assert_eq!(current["snapshot"], fresh["snapshot"]);
                    for field in [
                        "path",
                        "old_path",
                        "metadata_revision",
                        "revision",
                        "part",
                        "offset",
                        "next",
                        "lines",
                        "text",
                    ] {
                        assert_eq!(
                            current[field], saved[field],
                            "field={field} remote={remote}"
                        );
                    }
                }
            } else if change == "new-candidate" {
                assert!(
                    fresh["files"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .any(|file| file["path"] == "other/clean.rs")
                );
            } else {
                assert_ne!(
                    file(&fresh, "src/value.rs")["revision"],
                    file(&scope, "src/value.rs")["revision"]
                );
            }
            let complete = call(&fixture, "review_complete", receipts(&scope, true)).await;
            success(&complete);
            assert_eq!(complete["status"], "complete");
            assert_eq!(complete["findings"], 1);
            let after = value(&fixture, &package, &key).await;
            for field in ["snapshot", "pages", "package", "creator", "worktree"] {
                assert_eq!(after.value[field], before.value[field]);
            }
            stop(fixture).await;
        }
    }
}

#[tokio::test]
async fn freezes_commit_and_range_references() {
    for remote in [false, true] {
        let (fixture, _) = start(remote).await;
        let repository = Repository::init(&fixture.root).unwrap();
        write(&fixture.root, "src/value.rs", "initial_value();\n");
        let base = commit(&repository, "Root commit");
        write(&fixture.root, "src/value.rs", "reviewed_value();\n");
        let reviewed = commit(&repository, "Reviewed commit");
        repository
            .reference("refs/heads/review", reviewed, true, "Fixture reference")
            .unwrap();
        let commit_scope = scope(
            &fixture,
            json!({"kind":"commit","data":{"revision":"review"}}),
        )
        .await;
        let range_scope = scope(
            &fixture,
            json!({"kind":"range","data":{"base":base.to_string(),"head":"review"}}),
        )
        .await;
        assert_eq!(
            commit_scope["snapshot"]["target"]["data"]["revision"],
            reviewed.to_string()
        );
        assert_eq!(
            range_scope["snapshot"]["target"]["data"]["head"],
            reviewed.to_string()
        );
        write(&fixture.root, "src/value.rs", "later_value();\n");
        let later = commit(&repository, "Later commit");
        repository
            .reference("refs/heads/review", later, true, "Moved fixture reference")
            .unwrap();
        for scope in [&commit_scope, &range_scope] {
            success(
                &call(
                    &fixture,
                    "report_finding",
                    finding(scope, "src/value.rs", "new", 1, "reviewed_value();"),
                )
                .await,
            );
            success(&call(&fixture, "review_complete", receipts(scope, true)).await);
        }
        let root = scope(
            &fixture,
            json!({"kind":"commit","data":{"revision":base.to_string()}}),
        )
        .await;
        assert!(root["snapshot"]["target"]["data"]["parent"].is_null());
        success(
            &call(
                &fixture,
                "report_finding",
                finding(&root, "src/value.rs", "new", 1, "initial_value();"),
            )
            .await,
        );
        failure(
            &call(
                &fixture,
                "report_finding",
                finding(&root, "src/value.rs", "old", 1, "initial_value();"),
            )
            .await,
            "invalid_request",
        );
        success(&call(&fixture, "review_complete", receipts(&root, true)).await);
        write(&fixture.root, "src/merged.rs", "merged_value();\n");
        let mut index = repository.index().unwrap();
        index.add_path(Path::new("src/merged.rs")).unwrap();
        index.write().unwrap();
        let tree = repository.find_tree(index.write_tree().unwrap()).unwrap();
        let author = Signature::now("Fixture", "fixture@example.invalid").unwrap();
        let first = repository.find_commit(later).unwrap();
        let ancestor = repository.find_commit(base).unwrap();
        let side = repository
            .commit(None, &author, &author, "Side branch", &tree, &[&ancestor])
            .unwrap();
        let second = repository.find_commit(side).unwrap();
        let merge = repository
            .commit(
                Some("HEAD"),
                &author,
                &author,
                "Merge fixture",
                &tree,
                &[&first, &second],
            )
            .unwrap();
        let merged = scope(
            &fixture,
            json!({"kind":"commit","data":{"revision":merge.to_string()}}),
        )
        .await;
        assert_eq!(
            merged["snapshot"]["target"]["data"]["parent"],
            later.to_string()
        );
        assert_eq!(merged["files"].as_array().unwrap().len(), 1);
        assert_eq!(merged["files"][0]["path"], "src/merged.rs");
        success(&call(&fixture, "review_complete", receipts(&merged, true)).await);
        stop(fixture).await;
    }
}
