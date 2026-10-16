use super::*;

#[tokio::test]
async fn explores_without_shell_approval() {
    for remote in [false, true] {
        let server = Server::tools(vec![
            (name("search_files"), json!({"query":"needle","path":"src","type":"rust","context":1,"limit":2})),
            (name("search_files"), json!({"query":"needle","path":"src","type":"rust","context":1,"limit":3,"offset":2})),
            (name("search_files"), json!({"query":"needle","path":"src","mode":"files"})),
            (name("search_files"), json!({"query":"needle","path":"src","mode":"count","type":"js"})),
            (name("search_files"), json!({"query":"first.*second","regex":true,"multiline":true,"path":"src","type":"python"})),
            (name("find_files"), json!({"globs":["**/*.rs"],"path":"src","limit":1})),
            (name("find_files"), json!({"globs":["**/*.rs"],"path":"src","limit":1,"offset":1})),
        ]).await;
        let fixture = process::Fixture::packages(remote, &server, &[]).await;
        install(&fixture).await;
        std::fs::create_dir(fixture.root.join("src")).unwrap();
        std::fs::write(fixture.root.join(".gitignore"), "src/private.rs\n").unwrap();
        for (path, content) in [
            ("main.rs", "before\nneedle\nbetween\nneedle\nafter\n"),
            ("extra.js", "needle\nneedle\n"),
            ("script.py", "first\nsecond\n"),
            ("fresh.rs", "fresh source"),
            ("private.rs", "needle private"),
        ] {
            std::fs::write(fixture.root.join("src").join(path), content).unwrap();
        }
        for (path, modified) in [("main.rs", 1000), ("fresh.rs", 2000)] {
            std::fs::File::options()
                .write(true)
                .open(fixture.root.join("src").join(path))
                .unwrap()
                .set_times(
                    std::fs::FileTimes::new()
                        .set_modified(std::time::UNIX_EPOCH + Duration::from_secs(modified)),
                )
                .unwrap();
        }
        let turn = submit(&fixture).await;
        let page = finished(&fixture.client, fixture.session.id, turn.id).await;
        assert_eq!(
            page.runs.last().unwrap().status,
            Status::Completed,
            "{page:?}"
        );
        assert!(page.approvals.is_empty(), "{page:?}");
        let outputs = results(&page);
        assert_eq!(outputs.len(), 7, "{outputs:?}");
        assert_eq!(outputs[0]["data"]["next_offset"], 2);
        assert_eq!(outputs[0]["data"]["matches"][0]["matched"], false);
        assert_eq!(outputs[0]["data"]["matches"][1]["matched"], true);
        assert_eq!(outputs[1]["data"]["matches"][0]["line_number"], 3);
        assert_eq!(outputs[1]["data"]["matches"].as_array().unwrap().len(), 3);
        assert!(outputs[1]["data"]["next_offset"].is_null());
        assert_eq!(
            outputs[2]["data"]["paths"],
            json!(["src/extra.js", "src/main.rs"])
        );
        assert!(outputs[2]["data"].get("matches").is_none());
        assert_eq!(
            outputs[3]["data"]["counts"],
            json!([{"path":"src/extra.js","count":2}])
        );
        assert!(outputs[3]["data"].get("paths").is_none());
        assert_eq!(outputs[4]["data"]["matches"].as_array().unwrap().len(), 2);
        assert_eq!(outputs[5]["data"]["paths"], json!(["src/fresh.rs"]));
        assert_eq!(outputs[5]["data"]["next_offset"], 1);
        assert_eq!(outputs[6]["data"]["paths"], json!(["src/main.rs"]));
        assert!(outputs[6]["data"]["next_offset"].is_null());
        assert!(
            !serde_json::to_string(&outputs)
                .unwrap()
                .contains("needle private")
        );
        fixture.node.shutdown().await.unwrap();
        fixture.controller.close().await.unwrap();
    }
}
