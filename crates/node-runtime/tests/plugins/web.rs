use super::*;
use sailry_protocol::plugin::{Action, Context};
use serde_json::{Value, json};

#[path = "../support/web.rs"]
pub(super) mod server;

async fn installed(
    client: &Client,
    root: &Path,
    worktree: WorktreeId,
    actions: &[Action],
) -> Context {
    fs::write(
        root.join("plugin.json"),
        json!({
            "$schema":sailry_protocol::plugin::schema::AGENT_PLUGINS_ID,
            "name":"example", "version":"1.0.0", "description":"Public web fixture",
            "extensions":{"dev.sailry.platform":{
                "api_version":"v1", "actions":actions,
                "host":{"entry":"dev.sailry.platform/main.js",
                    "resources":["dev.sailry.platform/main.js"],"handlers":["run"]}
            }}
        })
        .to_string(),
    )
    .unwrap();
    fs::write(root.join("dev.sailry.platform/main.js"), r#"
        import {fetchWeb,indexedValue,getValue,completeRequest,prepareKnowledgeCollection} from 'sailry/sdk';
        export async function run(input) {
            if (input.store) {
                let content='',offset=0,pages=0;
                do {
                    const page=await fetchWeb({url:input.url,offset});
                    if (!page.untrusted) throw new Error('External content must remain untrusted');
                    content+=page.content;offset=page.next_offset;pages++;
                } while (offset!==null);
                const saved=await completeRequest(indexedValue('web/page',{url:input.url,body:content},
                    {fields:['public',''],tags:[],order:0,source:{key:'web/page',pointer:'/body'}},'0'));
                if (saved.Err) throw new Error(saved.Err.message);
                const created=await completeRequest(prepareKnowledgeCollection({name:'Public pages',
                    source:{kind:'storage',scope:'node',prefix:'web/'}}));
                if (created.Err) throw new Error(created.Err.message);
                return {pages,body:(await getValue('web/page')).value.body,collection:created.Ok.data};
            }
            try { return await fetchWeb(input); }
            catch(error) { return {error:error.code}; }
        }
    "#).unwrap();
    let package = info(execute(client, install(worktree, 0)).await);
    assert!(package.issues.is_empty(), "{:?}", package.issues);
    Context {
        invocation: None,
        turn: None,
        surface: Default::default(),
        package: package.summary.reference(),
        worktree: Some(worktree),
        session: None,
    }
}

fn call(client: &Client, context: &Context, input: Value) -> Request {
    client
        .prepare(Command::CallPlugin {
            handler: "run".into(),
            input,
        })
        .with_plugin(context.clone())
}

#[tokio::test]
async fn composes_cached_pages_with_private_storage() {
    for remote in [false, true] {
        let http = server::Server::start().await;
        let (directory, node, controller, client, worktree) =
            crate::plugins::support::empty(remote).await;
        node.set_web_fixture(server::HOST, http.address);
        let context = installed(
            &client,
            &directory.path().join("source/package"),
            worktree,
            &[
                Action::ReadWeb,
                Action::ReadStorage,
                Action::WriteStorage,
                Action::WriteKnowledge,
            ],
        )
        .await;
        let Output::PluginResult(result) = client
            .execute(call(
                &client,
                &context,
                json!({"url":format!("{}/paged",http.origin),"store":true}),
            ))
            .await
            .unwrap()
        else {
            panic!("plugin result expected")
        };
        assert_eq!(result["body"], server::paged());
        assert_eq!(result["pages"], 2);
        let collection = &result["collection"];
        assert_eq!(
            collection["owner"],
            json!({"kind":"plugin","name":"example"})
        );
        assert_eq!(
            collection["definition"]["source"],
            json!({"kind":"storage","scope":"node","prefix":"web/"})
        );
        assert_eq!(collection["definition"]["shared_with"], json!([]));
        assert!(collection["definition"]["model"].is_null());
        let requests = http.requests.lock().unwrap().clone();
        assert_eq!(requests.len(), 1);
        assert!(requests[0].starts_with("GET /paged "));
        assert!(!requests[0].to_ascii_lowercase().contains("authorization:"));
        assert!(!requests[0].to_ascii_lowercase().contains("cookie:"));
        let Output::PluginResult(page) = client
            .execute(call(
                &client,
                &context,
                json!({"url":format!("{}/html",http.origin)}),
            ))
            .await
            .unwrap()
        else {
            panic!("plugin web page expected")
        };
        assert_eq!(page["content"], server::TEXT);
        assert_eq!(page["untrusted"], true);
        assert_eq!(page["status"], 200);
        assert_eq!(page["title"], "Public page");
        assert!(page["next_offset"].is_null());
        let requests = http.requests.lock().unwrap().clone();
        assert_eq!(requests.len(), 2);
        assert!(requests[1].starts_with("GET /html "));
        assert!(!requests[1].to_ascii_lowercase().contains("authorization:"));
        assert!(!requests[1].to_ascii_lowercase().contains("cookie:"));
        node.shutdown().await.unwrap();
        controller.close().await.unwrap();
    }
}

#[tokio::test]
async fn preserves_long_urls_across_service_paths() {
    for remote in [false, true] {
        let http = server::Server::start().await;
        let (directory, node, controller, client, worktree) =
            crate::plugins::support::empty(remote).await;
        node.set_web_fixture(server::HOST, http.address);
        let context = installed(
            &client,
            &directory.path().join("source/package"),
            worktree,
            &[Action::ReadWeb],
        )
        .await;
        let suffix = format!("/html?query={}", "x".repeat(8193));
        let expected = format!("{}{suffix}", http.origin);
        for url in [expected.clone(), format!("{}/long-redirect", http.origin)] {
            let Output::PluginResult(page) = client
                .execute(call(&client, &context, json!({"url":url})))
                .await
                .unwrap()
            else {
                panic!("plugin web page expected")
            };
            assert_eq!(page["url"], expected);
            assert_eq!(page["content"], server::TEXT);
            assert_eq!(page["untrusted"], true);
        }
        {
            let requests = http.requests.lock().unwrap();
            assert_eq!(requests.len(), 3);
            assert!(requests[0].starts_with(&format!("GET {suffix} HTTP/1.1")));
            assert!(requests[2].starts_with(&format!("GET {suffix} HTTP/1.1")));
            assert!(requests.iter().all(|request| {
                !request.to_ascii_lowercase().contains("authorization:")
                    && !request.to_ascii_lowercase().contains("cookie:")
            }));
        }
        node.shutdown().await.unwrap();
        controller.close().await.unwrap();
    }
}

#[tokio::test]
async fn preserves_grants_and_credential_free_requests() {
    for remote in [false, true] {
        for granted in [false, true] {
            let http = server::Server::start().await;
            let (directory, node, controller, client, worktree) =
                crate::plugins::support::empty(remote).await;
            node.set_web_fixture(server::HOST, http.address);
            let context = installed(
                &client,
                &directory.path().join("source/package"),
                worktree,
                if granted { &[Action::ReadWeb] } else { &[] },
            )
            .await;
            let cases = if granted {
                vec![
                    (
                        json!({"url":format!("{}/html",http.origin),"headers":{"Authorization":"private"}}),
                        "invalid_request",
                    ),
                    (
                        json!({"url":format!("http://user@{}:{}/html",server::HOST,http.address.port())}),
                        "invalid_request",
                    ),
                    (
                        json!({"url":"http://127.0.0.1/private"}),
                        "permission_denied",
                    ),
                ]
            } else {
                vec![(
                    json!({"url":format!("{}/html",http.origin)}),
                    "permission_denied",
                )]
            };
            for (input, code) in cases {
                assert_eq!(
                    client
                        .execute(call(&client, &context, input))
                        .await
                        .unwrap(),
                    Output::PluginResult(json!({"error":code}))
                );
            }
            assert!(http.requests.lock().unwrap().is_empty());
            execute(
                &client,
                Command::SetPluginEnabled {
                    name: "example".into(),
                    expected_revision: 1,
                    enabled: false,
                },
            )
            .await;
            assert!(
                client
                    .execute(call(
                        &client,
                        &context,
                        json!({"url":format!("{}/html",http.origin)})
                    ))
                    .await
                    .is_err()
            );
            assert!(http.requests.lock().unwrap().is_empty());
            node.shutdown().await.unwrap();
            controller.close().await.unwrap();
        }
    }
}

#[tokio::test]
async fn cancels_held_callback_reads() {
    for remote in [false, true] {
        let http = server::Server::start().await;
        let (directory, node, controller, client, worktree) =
            crate::plugins::support::empty(remote).await;
        node.set_web_fixture(server::HOST, http.address);
        let context = installed(
            &client,
            &directory.path().join("source/package"),
            worktree,
            &[Action::ReadWeb],
        )
        .await;
        let request = call(
            &client,
            &context,
            json!({"url":format!("{}/hold",http.origin)}),
        );
        let pending = client.dispatch(request.clone()).await.unwrap();
        tokio::time::timeout(Duration::from_secs(5), http.held.cancelled())
            .await
            .unwrap();
        assert_eq!(
            client
                .execute(
                    client
                        .prepare(Command::CancelPluginCall {
                            request: request.id
                        })
                        .with_plugin(context)
                )
                .await
                .unwrap(),
            Output::PluginCallStopping {
                request: request.id
            }
        );
        assert_eq!(
            tokio::time::timeout(Duration::from_secs(5), pending.completion)
                .await
                .unwrap()
                .unwrap()
                .unwrap_err()
                .code,
            ErrorCode::Cancelled
        );
        tokio::time::timeout(Duration::from_secs(5), http.disconnected.cancelled())
            .await
            .unwrap();
        assert_eq!(http.requests.lock().unwrap().len(), 1);
        assert!(matches!(
            execute(&client, Command::Snapshot).await,
            Output::Snapshot(_)
        ));
        node.shutdown().await.unwrap();
        controller.close().await.unwrap();
    }
}
