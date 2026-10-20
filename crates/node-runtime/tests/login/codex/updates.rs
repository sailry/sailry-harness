use super::*;
use std::io::{Cursor, Write};
use std::sync::atomic::{AtomicBool, Ordering};

fn archive(version: &str, repository: bool, changed: bool) -> Vec<u8> {
    let mut manifest: serde_json::Value = serde_json::from_str(include_str!(
        "../../../../../plugins/openai-auth/plugin.json"
    ))
    .unwrap();
    manifest["version"] = version.into();
    if changed {
        manifest["extensions"]["dev.sailry.platform"]["model_auth"][1]["scopes"] =
            json!(["another"]);
    }
    let prefix = if repository {
        "repository/openai-auth/"
    } else {
        ""
    };
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for (path, bytes) in [
        ("plugin.json", manifest.to_string().into_bytes()),
        (
            "dev.sailry.platform/auth/flow.js",
            include_bytes!("../../../../../plugins/openai-auth/dev.sailry.platform/auth/flow.js")
                .to_vec(),
        ),
    ] {
        zip.start_file(
            format!("{prefix}{path}"),
            zip::write::SimpleFileOptions::default(),
        )
        .unwrap();
        zip.write_all(&bytes).unwrap();
    }
    zip.finish().unwrap().into_inner()
}

async fn upload(fixture: &Fixture, revision: u64, version: &str, changed: bool) -> plugin::Info {
    let bytes = archive(version, false, changed);
    let Output::PluginUpload(upload) = fixture
        .client
        .execute(
            fixture
                .client
                .prepare(Command::UploadPlugin(plugin::UploadSpec {
                    size: bytes.len() as u64,
                    revision: blake3::hash(&bytes).to_hex().to_string(),
                })),
        )
        .await
        .unwrap()
    else {
        panic!("plugin upload expected");
    };
    fixture
        .client
        .upload(
            &upload,
            &mut &bytes[..],
            sailry_link::CancellationToken::new(),
            |_| {},
        )
        .await
        .unwrap();
    let Output::Plugin(info) = fixture
        .client
        .execute(fixture.client.prepare(Command::InstallPluginUpload {
            stream: upload.stream,
            source: plugin::UploadSource::Directory,
            name: "openai-auth".into(),
            expected_revision: revision,
        }))
        .await
        .unwrap()
    else {
        panic!("plugin information expected");
    };
    assert!(info.issues.is_empty(), "{:?}", info.issues);
    info
}

async fn repository(fixture: &Fixture, revision: u64, official: bool) -> plugin::Info {
    let Output::Plugin(info) = fixture
        .client
        .execute(
            fixture.client.prepare(Command::InstallPluginSource {
                source: plugin::skills::Resolved {
                    repository: if official {
                        "https://github.com/sailry/sailry-plugins.git"
                    } else {
                        "https://github.com/other/plugins.git"
                    }
                    .into(),
                    git_ref: "a".repeat(40),
                    commit: "a".repeat(40),
                },
                path: "openai-auth".into(),
                name: "openai-auth".into(),
                expected_revision: revision,
            }),
        )
        .await
        .unwrap()
    else {
        panic!("plugin information expected");
    };
    assert!(info.issues.is_empty(), "{:?}", info.issues);
    info
}

fn authorizations(fixture: &Fixture, provider: ProviderId) -> serde_json::Value {
    let db = rusqlite::Connection::open_with_flags(
        fixture.node.profile().join("storage/node.sqlite3"),
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
    )
    .unwrap();
    let body: String = db
        .query_row(
            "SELECT json_extract(body,'$.authorizations') FROM providers WHERE id=?1",
            [provider.to_string()],
            |row| row.get(0),
        )
        .unwrap();
    serde_json::from_str(&body).unwrap()
}

fn catalog(client: &Client, provider: &Provider) -> Request {
    client.prepare(Command::DiscoverModels(Box::new(
        discovery::Source::Saved {
            provider: provider.id,
            expected_revision: provider.revision,
            oauth: None,
        },
    )))
}

fn usage(client: &Client, provider: &Provider, package: &plugin::Info) -> Request {
    client
        .prepare(Command::RequestProviderHttp {
            provider: provider.id,
            expected_revision: provider.revision,
            request: "usage".into(),
            input: Default::default(),
        })
        .with_plugin(plugin::Context {
            package: package.summary.reference(),
            worktree: None,
            session: None,
            turn: None,
            invocation: None,
            surface: Default::default(),
        })
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn retains_accounts_across_package_updates() {
    for remote in [false, true] {
        for imported in [false, true] {
            let changed = Arc::new(AtomicBool::new(false));
            let selected = changed.clone();
            let server = Server::start_with_request(ModelApi::Anthropic, move |request| {
                if request.path.contains("/zip/") {
                    Reply::Bytes(archive("1.0.1", true, selected.load(Ordering::SeqCst)))
                } else if request.path == "/backend-api/wham/usage" {
                    assert_eq!(request.headers["authorization"], format!("Bearer {ACCESS}"));
                    assert_eq!(request.headers["chatgpt-account-id"], ACCOUNT);
                    Reply::Json(json!({"rate_limit":{"primary_window":{"limit_window_seconds":18000,"used_percent":25,"reset_after_seconds":100},"secondary_window":{"limit_window_seconds":604800,"used_percent":10,"reset_after_seconds":200}},"credits":null}))
                } else { server::standard(request, 3600) }
            }).await;
            let fixture = Fixture::source(remote, &server).await;
            let initial = if imported {
                Some(upload(&fixture, 1, "1.0.0", false).await)
            } else {
                None
            };
            let provider = configured(&fixture, None).await;
            let session = super::super::execution::session(&fixture, &provider).await;
            let Output::QueuedTurn(turn) = fixture
                .client
                .execute(fixture.client.prepare(Command::QueueTurn {
                    session: session.id,
                    expected_revision: session.revision,
                    message: "Retain the isolated capture".into(),
                }))
                .await
                .unwrap()
            else {
                panic!("queued turn expected");
            };
            let captured = super::execution::captured(&fixture, &turn);
            let credentials = fixture
                .client
                .execute(fixture.client.prepare(Command::ListCredentials))
                .await
                .unwrap();
            let private = authorizations(&fixture, provider.id);
            let installed = if let Some(initial) = initial {
                upload(&fixture, initial.summary.revision, "1.0.1", false).await
            } else {
                repository(&fixture, 1, true).await
            };
            let Output::Plugin(status) = fixture
                .client
                .execute(fixture.client.prepare(Command::InstallBundledPlugin {
                    name: "provider-status".into(),
                    expected_revision: 0,
                }))
                .await
                .unwrap()
            else {
                panic!("status package expected");
            };
            let Output::DiscoveredModels(models) = fixture
                .client
                .execute(catalog(&fixture.client, &provider))
                .await
                .unwrap()
            else {
                panic!("models expected");
            };
            assert!(!models.models.is_empty());
            let Output::PluginHttp(response) = fixture
                .client
                .execute(usage(&fixture.client, &provider, &status))
                .await
                .unwrap()
            else {
                panic!("usage response expected");
            };
            assert_eq!(response.status, 200);
            let body: serde_json::Value = serde_json::from_str(&response.body).unwrap();
            assert_eq!(body["rate_limit"]["primary_window"]["used_percent"], 25);
            assert_eq!(body["rate_limit"]["secondary_window"]["used_percent"], 10);
            assert_eq!(
                fixture
                    .client
                    .execute(fixture.client.prepare(Command::ListCredentials))
                    .await
                    .unwrap(),
                credentials
            );
            assert_eq!(authorizations(&fixture, provider.id), private);
            assert_eq!(super::execution::captured(&fixture, &turn), captured);
            let requests = server
                .requests
                .lock()
                .unwrap()
                .iter()
                .filter(|request| !request.path.contains("/zip/"))
                .count();
            let foreign = repository(&fixture, installed.summary.revision, false).await;
            assert_eq!(
                fixture
                    .client
                    .execute(catalog(&fixture.client, &provider))
                    .await
                    .unwrap_err()
                    .code,
                ErrorCode::NotConfigured
            );
            assert_eq!(
                fixture
                    .client
                    .execute(usage(&fixture.client, &provider, &status))
                    .await
                    .unwrap_err()
                    .code,
                ErrorCode::NotConfigured
            );
            assert_eq!(
                server
                    .requests
                    .lock()
                    .unwrap()
                    .iter()
                    .filter(|request| !request.path.contains("/zip/"))
                    .count(),
                requests
            );
            changed.store(true, Ordering::SeqCst);
            if imported {
                upload(&fixture, foreign.summary.revision, "1.0.2", true).await;
            } else {
                repository(&fixture, foreign.summary.revision, true).await;
            }
            assert_eq!(
                fixture
                    .client
                    .execute(catalog(&fixture.client, &provider))
                    .await
                    .unwrap_err()
                    .code,
                ErrorCode::NotConfigured
            );
            assert_eq!(
                fixture
                    .client
                    .execute(usage(&fixture.client, &provider, &status))
                    .await
                    .unwrap_err()
                    .code,
                ErrorCode::NotConfigured
            );
            assert_eq!(
                fixture
                    .client
                    .execute(fixture.client.prepare(Command::ListCredentials))
                    .await
                    .unwrap(),
                credentials
            );
            assert_eq!(authorizations(&fixture, provider.id), private);
            assert_eq!(super::execution::captured(&fixture, &turn), captured);
            assert_eq!(
                server
                    .requests
                    .lock()
                    .unwrap()
                    .iter()
                    .filter(|request| request.path == "/oauth/token")
                    .count(),
                1
            );
            fixture.close().await;
        }
    }
}
