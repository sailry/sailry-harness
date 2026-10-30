//! Real Node hooks use isolated collectors over local and remote command paths.
use super::*;
use sailry_node_runtime::telemetry::Collector;
use sailry_protocol::{
    search::{self as web_search, Driver},
    telemetry::Update as SettingsUpdate,
};
use serde_json::{Value, json};
use std::{collections::BTreeSet, path::PathBuf};

use crate::search_fixture as search_http;

const SECRET: &str = "PRIVATE_TELEMETRY_MODEL_KEY";
const CONTENT: &str = "PRIVATE_TELEMETRY_CONVERSATION_CONTENT";
const QUERY: &str = "PRIVATE_TELEMETRY_SEARCH_QUERY";

struct Fixture {
    directory: tempfile::TempDir,
    profile: tempfile::TempDir,
    root: PathBuf,
    collector: Collector,
    node: Node,
    controller: Link,
    client: Client,
    session: Session,
}

impl Fixture {
    async fn new(remote: bool, server: &Server) -> Self {
        let directory = tempfile::tempdir().unwrap();
        let builder = tempfile::Builder::new();
        #[cfg(unix)]
        let builder = {
            use std::os::unix::fs::PermissionsExt as _;
            let mut builder = builder;
            builder.permissions(std::fs::Permissions::from_mode(0o700));
            builder
        };
        let profile = builder.tempdir_in(directory.path()).unwrap();
        let collector = Collector::register(&profile).unwrap();
        let node = Node::start_empty(profile.path()).await.unwrap();
        let root = directory.path().join("PRIVATE_TELEMETRY_PROJECT");
        std::fs::create_dir(&root).unwrap();
        let controller =
            Link::controller(directory.path().join("controller"), NetworkScope::default())
                .await
                .unwrap();
        let address = controller
            .handle()
            .pair(node.link().invite().unwrap().ticket())
            .await
            .unwrap();
        let client = Client::new(if remote {
            controller.handle().remote(address)
        } else {
            node.local()
        });
        let (session, provider_config) = configured_session(&client, &server.endpoint, &root).await;
        let session = approvals::prepare_model(&client, session, provider_config).await;
        let mut provider = provider(&client, &session).await;
        provider.name = "PRIVATE_TELEMETRY_PROVIDER_NAME".into();
        provider.request_max_retries = Some(0);
        provider.stream_max_retries = Some(0);
        let Output::Provider(provider) = client
            .execute(client.prepare(Command::SaveProvider {
                expected_revision: provider.revision,
                provider,
                secret: Some(Secret::new(SECRET.into())),
            }))
            .await
            .unwrap()
        else {
            panic!("provider expected")
        };
        let mut config = session.config.clone();
        config.credential = provider.credential;
        let Output::Session(session) = client
            .execute(client.prepare(Command::SetSessionConfig {
                session: session.id,
                expected_revision: session.revision,
                config,
            }))
            .await
            .unwrap()
        else {
            panic!("session expected")
        };
        Self {
            directory,
            profile,
            root,
            collector,
            node,
            controller,
            client,
            session,
        }
    }

    async fn submit(&self) -> TurnId {
        let Output::QueuedTurn(turn) = self
            .client
            .execute(self.client.prepare(Command::SubmitTurn {
                session: self.session.id,
                expected_revision: self.session.revision,
                message: CONTENT.into(),
            }))
            .await
            .unwrap()
        else {
            panic!("turn expected")
        };
        turn.id
    }

    async fn complete(&self) -> Page {
        let turn = self.submit().await;
        let page = finished(&self.client, self.session.id, turn).await;
        assert_eq!(status(&page, turn), Status::Completed);
        page
    }

    async fn select(&self, server: &Server) {
        let mut provider = provider(&self.client, &self.session).await;
        provider.endpoint = server.endpoint.clone();
        self.client
            .execute(self.client.prepare(Command::PutProvider {
                expected_revision: provider.revision,
                provider,
            }))
            .await
            .unwrap();
    }

    async fn search(&self, query: &str) {
        let Output::SearchPage(page) = self
            .client
            .execute(self.client.prepare(Command::SearchWeb(web_search::Request {
                query: query.into(),
                limit: 1,
                ..Default::default()
            })))
            .await
            .unwrap()
        else {
            panic!("search page expected")
        };
        assert_eq!(page.driver, Driver::Exa);
        assert_eq!(page.results[0].url, "https://example.com/source");
    }

    fn private(&self, payloads: &[Value], endpoints: &[&str]) {
        let encoded = serde_json::to_string(payloads).unwrap();
        for marker in [
            SECRET,
            CONTENT,
            QUERY,
            "PRIVATE_TELEMETRY_PROVIDER_NAME",
            "PRIVATE_TELEMETRY_PROJECT",
            "PRIVATE_TELEMETRY_FILE_CONTENT",
            "PRIVATE_TELEMETRY_MISSING_FILE.txt",
            "fixture-a",
            "https://example.com/source",
            self.directory.path().to_str().unwrap(),
            self.profile.path().to_str().unwrap(),
        ]
        .into_iter()
        .chain(endpoints.iter().copied())
        {
            assert!(
                !encoded.contains(marker),
                "private metadata entered a metric"
            );
        }
        for id in [
            self.session.id.to_string(),
            self.session.config.provider.to_string(),
            self.node.id().to_string(),
        ] {
            assert!(!encoded.contains(&id), "private identity entered a metric");
        }
    }

    async fn close(self) {
        self.controller.close().await.unwrap();
        self.node.shutdown().await.unwrap();
    }
}

async fn provider(client: &Client, session: &Session) -> Provider {
    let Output::Providers(providers) = client
        .execute(client.prepare(Command::ListProviders))
        .await
        .unwrap()
    else {
        panic!("providers expected")
    };
    providers
        .into_iter()
        .find(|provider| provider.id == session.config.provider)
        .unwrap()
}

fn status(page: &Page, turn: TurnId) -> Status {
    page.runs
        .iter()
        .find(|run| run.turn == turn)
        .unwrap()
        .status
}

fn rows<'a>(payloads: &'a [Value], table: &str) -> Vec<&'a Value> {
    payloads
        .iter()
        .filter(|batch| batch["table"] == table)
        .flat_map(|batch| batch["data"].as_array().unwrap())
        .collect()
}

fn contains(payloads: &[Value], table: &str, result: &str) -> bool {
    rows(payloads, table)
        .iter()
        .any(|row| row["result"] == result)
}

#[tokio::test]
async fn aggregates_five_tables_and_honors_consent() {
    for remote in [false, true] {
        let server = Server::scripted_tools(vec![]).await;
        let fixture = Fixture::new(remote, &server).await;
        let settings = fixture.client.telemetry_settings().await.unwrap();
        assert!(settings.metrics);
        fixture.complete().await;
        server.replace_tools(vec![(
            "read_session".into(),
            json!({"session":fixture.session.id,"limit":1}),
        )]);
        let page = fixture.complete().await;
        assert!(
            tool_details(&fixture.client, &page)
                .await
                .iter()
                .any(|(name, detail)| {
                    name == "read_session"
                        && detail
                            .result
                            .as_ref()
                            .is_some_and(|result| result["isError"] != true)
                })
        );
        let http = search_http::Server::start().await;
        fixture
            .node
            .set_search_fixture(Driver::Exa, &format!("{}/modern-mcp", http.endpoint));
        fixture.search(QUERY).await;
        assert!(
            server
                .authorization
                .lock()
                .unwrap()
                .iter()
                .all(|authorization| {
                    authorization.as_deref() == Some(format!("Bearer {SECRET}").as_str())
                })
        );

        let payloads = fixture.collector.payloads();
        assert_eq!(
            payloads
                .iter()
                .map(|batch| batch["table"].as_str().unwrap())
                .collect::<BTreeSet<_>>(),
            BTreeSet::from([
                "app_launches",
                "turns",
                "tool_calls",
                "model_requests",
                "search_requests"
            ])
        );
        assert!(contains(&payloads, "turns", "completed"));
        assert!(contains(&payloads, "tool_calls", "success"));
        assert!(contains(&payloads, "search_requests", "success"));
        let models = rows(&payloads, "model_requests");
        assert!(!models.is_empty());
        // This native GDK provider does not expose its HTTP receipts or retries.
        assert!(models.iter().all(|row| {
            row["model"] == "custom"
                && row["api"] == "chat_completions"
                && row["http_status_class"] == "unknown"
                && row.get("retries") == Some(&Value::Null)
        }));
        assert!(
            models.iter().any(
                |row| row["input"].as_u64().unwrap() > 0 && row["output"].as_u64().unwrap() > 0
            )
        );
        assert!(
            payloads
                .iter()
                .flat_map(|batch| batch["data"].as_array().unwrap())
                .all(|row| { row["installation_id"] == settings.installation_id.to_string() })
        );
        fixture.private(&payloads, &[&server.endpoint, &http.endpoint]);

        let disabled = fixture
            .client
            .save_telemetry(SettingsUpdate {
                revision: settings.revision,
                errors: settings.errors,
                metrics: false,
            })
            .await
            .unwrap();
        fixture.complete().await;
        fixture.search("PRIVATE_TELEMETRY_DISABLED_QUERY").await;
        assert!(fixture.collector.payloads().is_empty());

        let enabled = fixture
            .client
            .save_telemetry(SettingsUpdate {
                revision: disabled.revision,
                errors: disabled.errors,
                metrics: true,
            })
            .await
            .unwrap();
        fixture.complete().await;
        // Reset discards the old identity's pending aggregation, not just its ID.
        let reset = fixture
            .client
            .reset_telemetry(enabled.revision)
            .await
            .unwrap();
        assert_ne!(reset.installation_id, enabled.installation_id);
        fixture.complete().await;
        fixture.search("PRIVATE_TELEMETRY_RESET_QUERY").await;
        let payloads = fixture.collector.payloads();
        assert!(contains(&payloads, "turns", "completed"));
        assert!(contains(&payloads, "search_requests", "success"));
        assert!(
            payloads
                .iter()
                .flat_map(|batch| batch["data"].as_array().unwrap())
                .all(|row| { row["installation_id"] == reset.installation_id.to_string() })
        );
        assert!(
            !serde_json::to_string(&payloads)
                .unwrap()
                .contains(&enabled.installation_id.to_string())
        );
        fixture.private(&payloads, &[&server.endpoint, &http.endpoint]);
        fixture.close().await;
    }
}

#[tokio::test]
async fn anonymizes_private_plugin_tools() {
    const PACKAGE: &str = "private-project-tools";
    const TOOL: &str = "private_lookup";
    const VERSION: &str = "0.1.0+private-version";
    for remote in [false, true] {
        let server = Server::scripted_tools(vec![(plugin_tool(PACKAGE, TOOL), json!({}))]).await;
        let fixture = Fixture::new(remote, &server).await;
        let root = fixture.root.join("package");
        std::fs::create_dir_all(root.join("dev.sailry.platform/host")).unwrap();
        std::fs::write(
            root.join("dev.sailry.platform/host/main.js"),
            "export function lookup() { return {ok: true}; }",
        )
        .unwrap();
        std::fs::write(root.join("plugin.json"), json!({
            "$schema":"https://agent-plugins.org/schemas/1.0.0/plugin.schema.json",
            "name":PACKAGE, "version":VERSION,
            "extensions":{"dev.sailry.platform":{
                "api_version":"v1", "actions":[],
                "host":{"entry":"dev.sailry.platform/host/main.js", "resources":["dev.sailry.platform/host/main.js"], "handlers":["lookup"]},
                "tools":[{"name":TOOL, "description":"Inspect private state", "handler":{
                    "name":"lookup", "parameters":{"type":"object", "properties":{}, "required":[], "additionalProperties":false}
                }}]
            }}
        }).to_string()).unwrap();
        let Output::Plugin(package) = fixture
            .client
            .execute(fixture.client.prepare(Command::InstallPlugin {
                worktree: fixture.session.worktree,
                path: "package".into(),
                name: PACKAGE.into(),
                expected_revision: 0,
                expected_digest: None,
            }))
            .await
            .unwrap()
        else {
            panic!("plugin expected")
        };
        assert!(package.issues.is_empty(), "{:?}", package.issues);
        let page = fixture.complete().await;
        assert!(
            tool_details(&fixture.client, &page)
                .await
                .iter()
                .any(|(name, detail)| {
                    name == &plugin_tool(PACKAGE, TOOL)
                        && detail
                            .result
                            .as_ref()
                            .is_some_and(|result| result["ok"] == true)
                })
        );
        let payloads = fixture.collector.payloads();
        assert!(rows(&payloads, "tool_calls").iter().any(|row| {
            row["result"] == "success" && row["tool"] == json!({"source":"third_party"})
        }));
        fixture.private(&payloads, &[&server.endpoint, PACKAGE, TOOL, VERSION]);
        fixture.close().await;
    }
}

#[tokio::test]
async fn classifies_failed_denied_and_cancelled_exits() {
    for remote in [false, true] {
        let server = Server::scripted_tools(vec![]).await;
        let fixture = Fixture::new(remote, &server).await;
        approvals::install(&fixture.client, &["files"]).await;
        server.replace_tools(vec![(
            plugin_tool("files", "read_file"),
            json!({"path":"PRIVATE_TELEMETRY_MISSING_FILE.txt"}),
        )]);
        let page = fixture.complete().await;
        assert!(
            tool_details(&fixture.client, &page)
                .await
                .iter()
                .any(|(name, detail)| {
                    name == &plugin_tool("files", "read_file")
                        && detail
                            .result
                            .as_ref()
                            .is_some_and(|result| result["error"]["code"] == "not_found")
                })
        );
        server.replace_tools(vec![(plugin_tool("files", "write_file"), json!({
            "path":"denied.txt", "text":"PRIVATE_TELEMETRY_FILE_CONTENT", "expected_revision":null,
        }))]);
        let turn = fixture.submit().await;
        let (_, approval) = approvals::pending(&fixture.client, fixture.session.id).await;
        fixture
            .client
            .execute(fixture.client.prepare(Command::ResolveApproval {
                session: fixture.session.id,
                approval: approval.id,
                decision: Decision::Deny,
            }))
            .await
            .unwrap();
        let page = finished(&fixture.client, fixture.session.id, turn).await;
        assert_eq!(status(&page, turn), Status::Completed);
        assert!(!fixture.root.join("denied.txt").exists());

        let failing = Server::http(usize::MAX, 401).await;
        fixture.select(&failing).await;
        let turn = fixture.submit().await;
        let page = finished(&fixture.client, fixture.session.id, turn).await;
        assert_eq!(status(&page, turn), Status::Failed);

        let slow = Server::start(true).await;
        fixture.select(&slow).await;
        let turn = fixture.submit().await;
        slow.wait_count(1).await;
        fixture
            .client
            .execute(fixture.client.prepare(Command::StopTurn { turn }))
            .await
            .unwrap();
        let page = finished(&fixture.client, fixture.session.id, turn).await;
        assert_eq!(status(&page, turn), Status::Cancelled);

        let payloads = fixture.collector.payloads();
        assert!(contains(&payloads, "tool_calls", "error"));
        assert!(rows(&payloads, "tool_calls").iter().any(|row| {
            row["result"] == "error"
                && row["error_code"] == "not_found"
                && row["tool"]["name"] == "read_file"
        }));
        assert!(contains(&payloads, "tool_calls", "denied"));
        assert!(contains(&payloads, "turns", "failed"));
        assert!(contains(&payloads, "turns", "cancelled"));
        assert!(contains(&payloads, "model_requests", "error"));
        assert!(contains(&payloads, "model_requests", "cancelled"));
        fixture.private(
            &payloads,
            &[&server.endpoint, &failing.endpoint, &slow.endpoint],
        );
        fixture.close().await;
    }
}
