use super::*;
use serde_json::json;

#[tokio::test]
async fn validates_before_scripts_and_flows() {
    const PRIVATE: &str = "private-argument-fixture";
    for remote in [false, true] {
        let valid = json!({"query":"x".repeat(4097),"context":8});
        let server = Server::tools(vec![
            (
                plugin_tool("schema-probe", "plain"),
                json!({"query":PRIVATE,"context":"wrong"}),
            ),
            (
                plugin_tool("schema-probe", "flow"),
                json!({"query":PRIVATE,"context":null}),
            ),
            (plugin_tool("schema-probe", "plain"), valid.clone()),
            (
                plugin_tool("schema-probe", "plain"),
                json!({"query":"default"}),
            ),
        ])
        .await;
        let fixture = process::Fixture::packages(remote, &server, &[]).await;
        let root = fixture.root.join("package");
        std::fs::create_dir_all(root.join("dev.sailry.platform/host")).unwrap();
        std::fs::write(root.join("dev.sailry.platform/host/main.js"), r#"
export function plain(args) { if (args.context === 'wrong') throw new Error('unexpected handler execution'); return args; }
export function flow() { throw new Error('unexpected flow execution'); }
"#).unwrap();
        let schema = json!({"type":"object","required":["query"],"additionalProperties":false,"properties":{"query":{"type":"string"},"context":{"type":"integer","minimum":0,"default":0}}});
        let flow = json!({"name":"flow","parameters":schema,"flow":{"operations":["storage.get"]}});
        std::fs::write(root.join("plugin.json"), json!({
            "$schema":"https://agent-plugins.org/schemas/1.0.0/plugin.schema.json","name":"schema-probe","version":"0.1.0",
            "extensions":{"dev.sailry.platform":{"api_version":"v1","actions":["storage.read"],
                "host":{"entry":"dev.sailry.platform/host/main.js","resources":["dev.sailry.platform/host/main.js"],"handlers":["plain","flow"]},
                "tools":[{"name":"plain","description":"Echo neutral arguments","handler":{"name":"plain","parameters":schema}},
                    {"name":"flow","description":"Inspect neutral flow arguments","handler":flow}]}}
        }).to_string()).unwrap();
        let Output::Plugin(package) = fixture
            .client
            .execute(fixture.client.prepare(Command::InstallPlugin {
                worktree: fixture.session.worktree,
                path: "package".into(),
                name: "schema-probe".into(),
                expected_revision: 0,
                expected_digest: None,
            }))
            .await
            .unwrap()
        else {
            panic!("plugin expected")
        };
        assert!(package.issues.is_empty(), "{:?}", package.issues);
        let turn = approvals::submit(&fixture.client, fixture.session.id).await;
        let page = finished(&fixture.client, fixture.session.id, turn).await;
        assert_eq!(
            page.runs.last().unwrap().status,
            Status::Completed,
            "{page:?}"
        );
        let outputs = results(&fixture.client, &page).await;
        assert_eq!(outputs.len(), 4, "{outputs:?}");
        for output in &outputs[..2] {
            assert_eq!(output["isError"], true);
            assert_eq!(output["error"]["code"], "invalid_request");
            assert!(
                output["error"]["message"]
                    .as_str()
                    .unwrap()
                    .contains("/context"),
                "{output}"
            );
            assert!(!output.to_string().contains(PRIVATE));
            assert!(!output.to_string().contains("unexpected"));
        }
        assert_eq!(outputs[2], valid);
        assert_eq!(outputs[3], json!({"query":"default"}));
        assert!(page.approvals.is_empty());
        fixture.node.shutdown().await.unwrap();
        fixture.controller.close().await.unwrap();
    }
}
