use super::*;

#[tokio::test]
async fn anchors_only_changed_lines() {
    for remote in [false, true] {
        let (fixture, _) = start(remote).await;
        let repository = Repository::init(&fixture.root).unwrap();
        write(
            &fixture.root,
            "src/value.rs",
            "unchanged_context();\nold_value();\ntrailing_context();\n",
        );
        write(&fixture.root, "src/repeated.rs", "first();\nsecond();\n");
        write(
            &fixture.root,
            "src/deleted.rs",
            "old_context();\nremoved_value();\n",
        );
        commit(&repository, "Initial evidence");
        write(
            &fixture.root,
            "src/value.rs",
            "unchanged_context();\nnew_value();\ntrailing_context();\n",
        );
        write(
            &fixture.root,
            "src/repeated.rs",
            "duplicated_value();\nduplicated_value();\n",
        );
        std::fs::remove_file(fixture.root.join("src/deleted.rs")).unwrap();
        let scope = scope(&fixture, json!({"kind":"worktree","data":{"scope":"all"}})).await;
        let relocated = call(
            &fixture,
            "report_finding",
            finding(&scope, "src/value.rs", "new", 999, "new_value();"),
        )
        .await;
        success(&relocated);
        assert_eq!(relocated["finding"]["line"], 2);
        assert_eq!(relocated["finding"]["side"], "new");
        let old = call(
            &fixture,
            "report_finding",
            finding(&scope, "src/value.rs", "old", 2, "old_value();"),
        )
        .await;
        success(&old);
        let deleted = call(
            &fixture,
            "report_finding",
            finding(&scope, "src/deleted.rs", "old", 2, "removed_value();"),
        )
        .await;
        success(&deleted);
        assert_eq!(deleted["finding"]["line"], 2);
        assert_eq!(deleted["finding"]["side"], "old");
        for arguments in [
            finding(&scope, "src/repeated.rs", "new", 999, "duplicated_value();"),
            finding(&scope, "src/value.rs", "new", 1, "unchanged_context();"),
            finding(&scope, "src/value.rs", "old", 1, "unchanged_context();"),
            finding(&scope, "src/deleted.rs", "new", 2, "removed_value();"),
        ] {
            failure(
                &call(&fixture, "report_finding", arguments).await,
                "invalid_request",
            );
        }
        let complete = call(&fixture, "review_complete", receipts(&scope, true)).await;
        success(&complete);
        assert_eq!(complete["findings"], 3);
        stop(fixture).await;
    }
}
