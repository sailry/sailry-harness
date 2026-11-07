use super::*;
use crate::plugins::web::server;

#[tokio::test]
async fn replaces_held_evaluations_after_revocation() {
    let fixture = Fixture::new().await;
    let package = fixture.install("status-fixture", SOURCE, &[], 0).await;
    let scope = Scope {
        worktree: Some(fixture.worktree),
        session: None,
    };
    let (entered, release) = fixture.node.hold_plugin_read_fixture();
    let mut feed = Feed::new(fixture.client.clone(), scope);
    let read = tokio::time::timeout(Duration::from_secs(5), entered)
        .await
        .unwrap()
        .unwrap();
    assert!(matches!(read.command, Command::ReadPluginValue { .. }));
    let disabled = info(
        execute(
            &fixture.client,
            Command::SetPluginEnabled {
                name: package.summary.name.clone(),
                expected_revision: package.summary.revision,
                enabled: false,
            },
        )
        .await,
    );
    feed.until(|view| view.connected && view.entries.is_empty())
        .await;
    let enabled = info(
        execute(
            &fixture.client,
            Command::SetPluginEnabled {
                name: package.summary.name.clone(),
                expected_revision: disabled.summary.revision,
                enabled: true,
            },
        )
        .await,
    );
    // The held read is deliberately not released: revocation stops that VM.
    feed.until(|view| fixture::label(view) == Some("0")).await;
    assert!(!release.is_cancelled());
    let context = fixture.context(&enabled, scope);
    fixture.write(&context, "counter", json!(4), 0).await;
    let retry = Feed::new(fixture.client.clone(), scope);
    feed.until(|view| fixture::label(view) == Some("4")).await;
    release.cancel();
    retry.close().await;
    feed.close().await;
    fixture.close().await;
}

#[tokio::test]
async fn bounds_scopes_and_releases_cancelled_reads() {
    let http = server::Server::start().await;
    let fixture = Fixture::new().await;
    let other = fixture.directory.path().join("other");
    fs::create_dir(&other).unwrap();
    let Output::Project(project) = execute(
        &fixture.client,
        Command::RegisterProject {
            name: "Other status scope".into(),
            path: other.to_str().unwrap().into(),
        },
    )
    .await
    else {
        panic!("project expected")
    };
    let Output::Snapshot(snapshot) = execute(&fixture.client, Command::Snapshot).await else {
        panic!("snapshot expected")
    };
    let second = snapshot
        .worktrees
        .iter()
        .find(|entry| entry.project == Some(project.id))
        .unwrap()
        .id;
    for index in 0..6 {
        let name = format!("status-{index}");
        for worktree in [fixture.worktree, second] {
            fixture
                .node
                .set_web_fixture(&format!("{name}.{worktree}.sailry.test"), http.address);
        }
        let source = format!(
            r#"
                import {{context,fetchWeb}} from 'sailry/sdk';
                export async function badge() {{
                    const scope=context();
                    await fetchWeb({{url:'http://'+scope.package.name+'.'+scope.worktree+'.sailry.test:{}/hold'}});
                    return [{{id:'badge',visible:true}}];
                }}
                export function invoke() {{ return null; }}
            "#,
            http.address.port()
        );
        fixture.install(&name, &source, &[], 0).await;
    }
    let first = Feed::new(
        fixture.client.clone(),
        Scope {
            worktree: Some(fixture.worktree),
            session: None,
        },
    );
    let second = Feed::new(
        fixture.client.clone(),
        Scope {
            worktree: Some(second),
            session: None,
        },
    );
    tokio::time::timeout(Duration::from_secs(5), async {
        while http.requests.lock().unwrap().len() < 4 {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    assert!(
        tokio::time::timeout(Duration::from_millis(200), async {
            while http.requests.lock().unwrap().len() == 4 {
                tokio::task::yield_now().await;
            }
        })
        .await
        .is_err()
    );
    assert_eq!(http.requests.lock().unwrap().len(), 4);
    first.close().await;
    second.close().await;
    tokio::time::timeout(Duration::from_secs(5), http.disconnected.cancelled())
        .await
        .unwrap();
    let resumed = Feed::new(
        fixture.client.clone(),
        Scope {
            worktree: Some(fixture.worktree),
            session: None,
        },
    );
    tokio::time::timeout(Duration::from_secs(5), async {
        while http.requests.lock().unwrap().len() < 8 {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    assert_eq!(http.requests.lock().unwrap().len(), 8);
    resumed.close().await;
    fixture.close().await;
}
