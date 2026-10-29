use super::*;
use serde_json::{Value, json};
#[path = "../support/web.rs"]
mod server;

async fn client(node: &Node, directory: &std::path::Path, remote: bool) -> (Client, Link) {
    let controller = Link::controller(directory.join("controller"), NetworkScope::default())
        .await
        .unwrap();
    let address = controller
        .handle()
        .pair(node.link().invite().unwrap().ticket())
        .await
        .unwrap();
    (
        Client::new(if remote {
            controller.handle().remote(address)
        } else {
            node.local()
        }),
        controller,
    )
}

async fn pages(client: &Client, page: &Page) -> Vec<Value> {
    results(client, page)
        .await
        .into_iter()
        .filter(|result| result.get("content").is_some())
        .collect()
}

#[tokio::test]
async fn binds_anonymous_reads_without_native_search() {
    for remote in [false, true] {
        let http = server::Server::start().await;
        let model = Server::tools(vec![
            (
                plugin_tool("web-search", "web_fetch"),
                json!({"url":format!("{}/redirect/1", http.origin)}),
            ),
            (
                plugin_tool("web-search", "web_fetch"),
                json!({"url":format!("{}/json", http.origin)}),
            ),
            (
                plugin_tool("web-search", "web_fetch"),
                json!({"url":format!("{}/markdown", http.origin)}),
            ),
            (
                plugin_tool("web-search", "web_fetch"),
                json!({"url":format!("{}/pdf", http.origin)}),
            ),
        ])
        .await;
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().join("project");
        std::fs::create_dir(&root).unwrap();
        let node = Node::start_empty(directory.path().join("node"))
            .await
            .unwrap();
        node.set_web_fixture(server::HOST, http.address);
        let (client, controller) = client(&node, directory.path(), remote).await;
        let session = approvals::prepare_packages(&client, &model, &root, &["web-search"]).await;
        let Output::Plugin(package) = client
            .execute(client.prepare(Command::ReadPlugin {
                name: "web-search".into(),
            }))
            .await
            .unwrap()
        else {
            panic!("web package expected")
        };
        assert!(package.issues.is_empty(), "{:?}", package.issues);
        assert!(package.summary.enabled);
        let Output::QueuedTurn(turn) = client
            .execute(client.prepare(Command::SubmitTurn {
                session: session.id,
                expected_revision: session.revision,
                message: "Read the public pages".into(),
            }))
            .await
            .unwrap()
        else {
            panic!("turn expected")
        };
        let page = finished(&client, session.id, turn.id).await;
        assert_eq!(page.runs[0].status, Status::Completed, "{:?}", page.runs);
        assert!(page.approvals.is_empty());
        let pages = pages(&client, &page).await;
        assert_eq!(pages.len(), 4, "{page:?}");
        assert_eq!(pages[0]["content"], server::TEXT);
        assert_eq!(pages[1]["content"], server::JSON);
        assert_eq!(pages[2]["content"], server::MARKDOWN);
        assert!(
            pages[3]["content"]
                .as_str()
                .unwrap()
                .contains("PDF acceptance")
        );
        assert_eq!(pages[0]["title"], "Public page");
        assert!(pages.iter().all(|page| page["untrusted"] == true));
        assert_eq!(pages[0]["untrusted"], true);
        assert_eq!(pages[1]["untrusted"], true);
        {
            let requests = model.requests.lock().unwrap();
            let tools = requests[0]["tools"].as_array().unwrap();
            assert!(
                tools
                    .iter()
                    .any(|tool| tool["function"]["name"] == "web_fetch"),
                "{tools:?}"
            );
            assert!(!tools.iter().any(|tool| tool["type"] == "web_search"));
        }
        for headers in http.requests.lock().unwrap().iter() {
            assert!(headers.starts_with("GET "));
            assert!(!headers.to_ascii_lowercase().contains("authorization:"));
            assert!(!headers.to_ascii_lowercase().contains("cookie:"));
        }
        assert_eq!(http.requests.lock().unwrap().len(), 5);
        node.shutdown().await.unwrap();
        controller.close().await.unwrap();
    }
}

#[tokio::test]
async fn shares_cached_unicode_and_json_pages() {
    for remote in [false, true] {
        let http = server::Server::start().await;
        let directory = tempfile::tempdir().unwrap();
        let node = Node::start_empty(directory.path().join("node"))
            .await
            .unwrap();
        node.set_web_fixture(server::HOST, http.address);
        let (client, controller) = client(&node, directory.path(), remote).await;
        for path in ["/paged", "/long-json"] {
            let url = format!("{}{path}", http.origin);
            let mut content = String::new();
            let mut offset = 0;
            loop {
                let Output::WebPage(page) = client
                    .execute(
                        client.prepare(Command::FetchWeb(sailry_protocol::web::Request {
                            url: url.clone(),
                            offset,
                        })),
                    )
                    .await
                    .unwrap()
                else {
                    panic!("web page expected")
                };
                assert!(page.untrusted);
                assert!(page.content.chars().count() <= sailry_protocol::web::PAGE_CHARS);
                assert_eq!(page.truncated, page.next_offset.is_some());
                content.push_str(&page.content);
                let Some(next) = page.next_offset else { break };
                assert!(next > offset);
                offset = next;
            }
            if path == "/paged" {
                assert_eq!(content, server::paged());
            } else {
                let value: Value = serde_json::from_str(&content).unwrap();
                assert_eq!(value["content"], server::paged());
            }
        }
        assert_eq!(http.requests.lock().unwrap().len(), 2);
        node.shutdown().await.unwrap();
        controller.close().await.unwrap();
    }
}

#[tokio::test]
async fn stops_bound_fetches_with_the_turn() {
    for remote in [false, true] {
        let http = server::Server::start().await;
        let model = Server::tools(vec![(
            plugin_tool("web-search", "web_fetch"),
            json!({"url":format!("{}/hold", http.origin)}),
        )])
        .await;
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().join("project");
        std::fs::create_dir(&root).unwrap();
        let node = Node::start_empty(directory.path().join("node"))
            .await
            .unwrap();
        node.set_web_fixture(server::HOST, http.address);
        let (client, controller) = client(&node, directory.path(), remote).await;
        let session = approvals::prepare_packages(&client, &model, &root, &["web-search"]).await;
        let Output::QueuedTurn(turn) = client
            .execute(client.prepare(Command::SubmitTurn {
                session: session.id,
                expected_revision: session.revision,
                message: "Read the page".into(),
            }))
            .await
            .unwrap()
        else {
            panic!("turn expected")
        };
        tokio::time::timeout(Duration::from_secs(5), http.held.cancelled())
            .await
            .unwrap();
        client
            .execute(client.prepare(Command::StopTurn { turn: turn.id }))
            .await
            .unwrap();
        let page = finished(&client, session.id, turn.id).await;
        assert_eq!(page.runs[0].status, Status::Cancelled);
        assert!(page.approvals.is_empty());
        tokio::time::timeout(Duration::from_secs(3), http.disconnected.cancelled())
            .await
            .unwrap();
        node.shutdown().await.unwrap();
        controller.close().await.unwrap();
    }
}

#[tokio::test]
async fn shares_limits_and_explicit_cancellation() {
    for remote in [false, true] {
        let http = server::Server::start().await;
        let directory = tempfile::tempdir().unwrap();
        let node = Node::start_empty(directory.path().join("node"))
            .await
            .unwrap();
        node.set_web_fixture(server::HOST, http.address);
        let (client, controller) = client(&node, directory.path(), remote).await;
        let client = Arc::new(client);
        for (url, code) in [
            (
                format!("http://{}/html", http.address),
                ErrorCode::PermissionDenied,
            ),
            (
                format!("{}/private", http.origin),
                ErrorCode::PermissionDenied,
            ),
            (format!("{}/large", http.origin), ErrorCode::Unavailable),
            (
                format!("{}/redirect/6", http.origin),
                ErrorCode::Unavailable,
            ),
        ] {
            let request = client.prepare(Command::FetchWeb(sailry_protocol::web::Request {
                url,
                offset: 0,
            }));
            assert!(!request.command.durable());
            assert_eq!(client.execute(request).await.unwrap_err().code, code);
        }
        let request = client.prepare(Command::FetchWeb(sailry_protocol::web::Request {
            url: format!("{}/hold", http.origin),
            offset: 0,
        }));
        let id = request.id;
        let fetch = {
            let client = client.clone();
            tokio::spawn(async move { client.execute(request).await })
        };
        tokio::time::timeout(Duration::from_secs(3), http.held.cancelled())
            .await
            .unwrap();
        assert_eq!(
            client
                .execute(client.prepare(Command::CancelWebFetch { request: id }))
                .await
                .unwrap(),
            Output::WebFetchCancelled { request: id }
        );
        assert_eq!(fetch.await.unwrap().unwrap_err().code, ErrorCode::Cancelled);
        tokio::time::timeout(Duration::from_secs(3), http.disconnected.cancelled())
            .await
            .unwrap();
        node.shutdown().await.unwrap();
        controller.close().await.unwrap();
    }
}

#[tokio::test]
#[ignore = "requires anonymous public GitHub and documentation access"]
async fn reads_public_sources_without_credentials() {
    for remote in [false, true] {
        let directory = tempfile::tempdir().unwrap();
        let node = Node::start_empty(directory.path().join("node"))
            .await
            .unwrap();
        let (client, controller) = client(&node, directory.path(), remote).await;
        let Output::WebPage(page) = client
            .execute(
                client.prepare(Command::FetchWeb(sailry_protocol::web::Request {
                    url: "https://api.github.com/repos/duxweb/dux-ai".into(),
                    offset: 0,
                })),
            )
            .await
            .unwrap()
        else {
            panic!("web page expected")
        };
        assert_eq!(page.status, 200, "{}", page.content);
        assert!(page.untrusted);
        let json: Value = serde_json::from_str(&page.content).unwrap();
        assert_eq!(json["full_name"], "duxweb/dux-ai");
        let Output::WebPage(page) = client
            .execute(
                client.prepare(Command::FetchWeb(sailry_protocol::web::Request {
                    url: "https://docs.rs/reqwest/0.13.4/reqwest/struct.ClientBuilder.html".into(),
                    offset: 0,
                })),
            )
            .await
            .unwrap()
        else {
            panic!("documentation page expected")
        };
        assert_eq!(page.status, 200, "{}", page.content);
        assert!(page.untrusted);
        assert!(
            page.title
                .as_deref()
                .is_some_and(|title| title.contains("ClientBuilder"))
        );
        let mut content = page.content;
        let mut next = page.next_offset;
        while let Some(offset) = next {
            let Output::WebPage(page) = client
                .execute(
                    client.prepare(Command::FetchWeb(sailry_protocol::web::Request {
                        url: "https://docs.rs/reqwest/0.13.4/reqwest/struct.ClientBuilder.html"
                            .into(),
                        offset,
                    })),
                )
                .await
                .unwrap()
            else {
                panic!("documentation page expected")
            };
            assert_eq!(page.status, 200);
            assert!(page.untrusted);
            assert!(page.content.chars().count() <= sailry_protocol::web::PAGE_CHARS);
            assert!(page.next_offset.is_none_or(|next| next > offset));
            content.push_str(&page.content);
            next = page.next_offset;
        }
        assert!(content.contains("read_timeout"));
        node.shutdown().await.unwrap();
        controller.close().await.unwrap();
    }
}

#[tokio::test]
async fn decodes_declared_and_fallback_charsets() {
    for remote in [false, true] {
        let http = server::Server::start().await;
        let directory = tempfile::tempdir().unwrap();
        let node = Node::start_empty(directory.path().join("node"))
            .await
            .unwrap();
        node.set_web_fixture(server::HOST, http.address);
        let (client, controller) = client(&node, directory.path(), remote).await;
        for (path, expected) in [
            ("/gbk", "中文 text"),
            ("/gbk-fallback", "中文 text"),
            ("/gbk-meta", "中文 text"),
            ("/gbk-json", server::ENCODED_JSON),
        ] {
            let Output::WebPage(page) = client
                .execute(
                    client.prepare(Command::FetchWeb(sailry_protocol::web::Request {
                        url: format!("{}{path}", http.origin),
                        offset: 0,
                    })),
                )
                .await
                .unwrap()
            else {
                panic!("web page expected");
            };
            assert_eq!(page.content, expected);
            assert!(page.untrusted);
            if path == "/gbk-meta" {
                assert_eq!(page.title.as_deref(), Some("中文"));
            }
        }
        let Output::WebPage(page) = client
            .execute(
                client.prepare(Command::FetchWeb(sailry_protocol::web::Request {
                    url: format!("{}/invalid-charset", http.origin),
                    offset: 0,
                })),
            )
            .await
            .unwrap()
        else {
            panic!("web page expected");
        };
        assert_eq!(page.content, "��");
        assert!(page.untrusted);
        assert_eq!(http.requests.lock().unwrap().len(), 5);
        node.shutdown().await.unwrap();
        controller.close().await.unwrap();
    }
}

#[path = "../support/web_proxy.rs"]
mod proxy;

#[tokio::test]
async fn proxies_public_reads_and_blocks_private_redirects() {
    for remote in [false, true] {
        let http = server::Server::start().await;
        let proxy = proxy::Proxy::start(vec![http.address]).await;
        let directory = tempfile::tempdir().unwrap();
        let node = Node::start_empty(directory.path().join("node"))
            .await
            .unwrap();
        node.set_web_fixture(server::HOST, http.address);
        node.set_web_proxy_fixture(
            server::HOST,
            hyper_util::client::proxy::matcher::Matcher::builder()
                .all(proxy.url.replacen("http://", "http://fixture:private@", 1))
                .build(),
        );
        node.set_web_dns_fixture(server::HOST, vec!["198.18.0.7".parse().unwrap()]);
        let (client, controller) = client(&node, directory.path(), remote).await;
        let Output::WebPage(page) = client
            .execute(
                client.prepare(Command::FetchWeb(sailry_protocol::web::Request {
                    url: format!("{}/redirect/1", http.origin),
                    offset: 0,
                })),
            )
            .await
            .unwrap()
        else {
            panic!("web page expected");
        };
        assert_eq!(page.content, server::TEXT);
        assert!(page.untrusted);
        assert_eq!(proxy.requests.lock().unwrap().len(), 2);
        assert_eq!(
            client
                .execute(
                    client.prepare(Command::FetchWeb(sailry_protocol::web::Request {
                        url: format!("{}/private", http.origin),
                        offset: 0
                    }))
                )
                .await
                .unwrap_err()
                .code,
            ErrorCode::PermissionDenied
        );
        {
            let forwarded = proxy.requests.lock().unwrap();
            assert_eq!(forwarded.len(), 3);
            assert!(
                forwarded
                    .iter()
                    .all(|request| request.starts_with(&format!("GET {}/", http.origin)))
            );
            assert!(forwarded.iter().all(|request| {
                request
                    .to_ascii_lowercase()
                    .contains("proxy-authorization: basic zml4dhvyztpwcml2yxrl")
            }));
        }
        {
            let requests = http.requests.lock().unwrap();
            assert_eq!(requests.len(), 3);
            assert!(requests.iter().all(|headers| {
                headers.to_ascii_lowercase().contains(&format!(
                    "host: {}:{}",
                    server::HOST,
                    http.address.port()
                ))
            }));
            assert!(
                requests
                    .iter()
                    .all(|headers| !headers.to_ascii_lowercase().contains("authorization:"))
            );
        }
        node.shutdown().await.unwrap();
        controller.close().await.unwrap();
    }
}
