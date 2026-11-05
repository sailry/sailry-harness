use super::*;

async fn notices(fixture: &Fixture) -> usize {
    let Output::Snapshot(snapshot) = execute(&fixture.client, Command::Snapshot).await else {
        panic!("snapshot expected")
    };
    snapshot
        .notifications
        .iter()
        .filter(|notice| {
            matches!(
                &notice.content.details,
                Some(sailry_protocol::notification::Details::System(report))
                    if report.plugin.as_ref().is_some_and(|plugin| plugin.name == "status-fixture")
            )
        })
        .count()
}

#[tokio::test]
async fn reports_persistent_status_failures_once() {
    for remote in [false, true] {
        let fixture = Fixture::new(remote).await;
        let package = fixture
            .install("status-fixture", SOURCE, &[Change::Package], 0)
            .await;
        let scope = Scope {
            worktree: Some(fixture.worktree),
            session: None,
        };
        let context = fixture.context(&package, scope);
        let mut feed = Feed::new(fixture.client.clone(), scope);
        let initial = feed.until(|view| fixture::label(view) == Some("0")).await;
        let mut revision = 0;
        for cycle in 0..2 {
            for attempt in 0..3 {
                let message = format!("Busy cycle {cycle} attempt {attempt}");
                fixture
                    .write(
                        &context,
                        "mode",
                        json!({"error":"busy","message":message}),
                        revision,
                    )
                    .await;
                revision += 1;
                let retry = (attempt > 0).then(|| Feed::new(fixture.client.clone(), scope));
                let failed = feed
                    .until(|view| {
                        view.failures
                            .first()
                            .is_some_and(|failure| failure.fault.message.ends_with(&message))
                    })
                    .await;
                assert_eq!(failed.entries, initial.entries);
                assert_eq!(failed.failures[0].fault.code, ErrorCode::Busy);
                assert_eq!(notices(&fixture).await, cycle + usize::from(attempt > 0));
                if let Some(retry) = retry {
                    retry.close().await;
                }
            }
            fixture.write(&context, "mode", Value::Null, revision).await;
            revision += 1;
            let retry = Feed::new(fixture.client.clone(), scope);
            feed.until(|view| view.failures.is_empty()).await;
            retry.close().await;
        }
        fixture.write(&context, "fail", json!(true), 0).await;
        let current = feed.current();
        let request = fixture
            .client
            .prepare_status(&scope, &current.entries[0])
            .unwrap();
        assert_eq!(
            fixture.client.execute(request).await.unwrap_err().code,
            ErrorCode::InvalidRequest
        );
        assert_eq!(notices(&fixture).await, 3);
        feed.close().await;
        fixture.close().await;
    }
}

#[tokio::test]
async fn refreshes_declarations_and_revokes_entries() {
    for remote in [false, true] {
        let fixture = Fixture::new(remote).await;
        let package = fixture.install("status-fixture", SOURCE, &[], 0).await;
        let scope = Scope {
            worktree: Some(fixture.worktree),
            session: None,
        };
        let mut feed = Feed::new(fixture.client.clone(), scope);
        let initial = feed.until(|view| fixture::label(view) == Some("0")).await;
        let stale = fixture
            .client
            .prepare_status(&scope, &initial.entries[0])
            .unwrap();
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
        assert!(fixture.client.execute(stale).await.is_err());
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
        feed.until(|view| fixture::label(view) == Some("0")).await;

        let mut manifest = fixture.declaration("status-fixture", &[]);
        manifest["version"] = json!("0.1.1");
        manifest["extensions"][plugin::NAMESPACE]["ui"][0]["id"] = json!("replacement");
        let replaced = fixture
            .install_with(
                "status-fixture",
                &SOURCE.replace("id:'badge'", "id:'replacement'"),
                manifest,
                enabled.summary.revision,
            )
            .await;
        assert_ne!(package.summary.digest, replaced.summary.digest);
        let refreshed = feed
            .until(|view| {
                view.entries
                    .iter()
                    .any(|entry| entry.declaration.id == "replacement" && entry.state.visible)
            })
            .await;
        assert!(
            refreshed
                .entries
                .iter()
                .all(|entry| entry.package == replaced.summary.reference())
        );
        assert!(
            !refreshed
                .entries
                .iter()
                .any(|entry| entry.declaration.id == "badge")
        );
        execute(
            &fixture.client,
            Command::RemovePlugin {
                name: replaced.summary.name,
                expected_revision: replaced.summary.revision,
            },
        )
        .await;
        feed.until(|view| view.connected && view.entries.is_empty() && view.failures.is_empty())
            .await;
        feed.close().await;
        fixture.close().await;
    }
}

#[tokio::test]
async fn retains_last_good_after_failed_reads() {
    let fixture = Fixture::new(false).await;
    let package = fixture
        .install("status-fixture", SOURCE, &[Change::Package], 0)
        .await;
    let scope = Scope {
        worktree: Some(fixture.worktree),
        session: None,
    };
    let context = fixture.context(&package, scope);
    let mut feed = Feed::new(fixture.client.clone(), scope);
    let initial = feed.until(|view| fixture::label(view) == Some("0")).await;
    let mut revision = 0;
    for mode in ["undeclared", "oversize", "reply", "write"] {
        fixture.write(&context, "mode", json!(mode), revision).await;
        revision += 1;
        let failed = feed.until(|view| !view.failures.is_empty()).await;
        assert_eq!(failed.entries, initial.entries);
        assert_eq!(failed.failures.len(), 1);
        assert_eq!(failed.failures[0].package, package.summary.reference());
        assert_eq!(
            failed.failures[0].fault.code,
            if mode == "write" {
                ErrorCode::PermissionDenied
            } else {
                ErrorCode::InvalidRequest
            }
        );
        assert!(failed.failures[0].fault.details.is_none());
        fixture.write(&context, "mode", Value::Null, revision).await;
        revision += 1;
        if mode == "undeclared" {
            // Ordinary triggers cannot immediately repeat a failing export.
            assert!(
                tokio::time::timeout(Duration::from_millis(1200), async {
                    loop {
                        feed.updates.changed().await.unwrap();
                        if feed.current().failures.is_empty() {
                            break;
                        }
                    }
                })
                .await
                .is_err()
            );
        }
        // A new subscriber explicitly retries without waiting for ordinary back-off.
        let retry = Feed::new(fixture.client.clone(), scope);
        feed.until(|view| {
            view.connected && view.failures.is_empty() && fixture::label(view) == Some("0")
        })
        .await;
        retry.close().await;
    }
    assert!(!fixture.read(&context, "forbidden").await.present);
    feed.close().await;
    fixture.close().await;
}

#[tokio::test]
async fn rejects_unknown_and_mismatched_scopes() {
    for remote in [false, true] {
        let fixture = Fixture::new(remote).await;
        let session = fixture.session(None).await;
        for (scope, code) in [
            (
                Scope {
                    worktree: Some(WorktreeId::new()),
                    session: None,
                },
                ErrorCode::NotFound,
            ),
            (
                Scope {
                    worktree: None,
                    session: Some(SessionId::new()),
                },
                ErrorCode::NotFound,
            ),
            (
                Scope {
                    worktree: Some(WorktreeId::new()),
                    session: Some(session.id),
                },
                ErrorCode::WrongTarget,
            ),
        ] {
            let mut feed = Feed::new(fixture.client.clone(), scope);
            let failed = feed.until(|view| view.error.is_some()).await;
            assert_eq!(failed.error.unwrap().code, code);
            assert!(failed.entries.is_empty());
            assert!(!failed.connected);
            feed.close().await;
        }
        let scope = Scope {
            worktree: Some(fixture.worktree),
            session: Some(session.id),
        };
        let mut feed = Feed::new(fixture.client.clone(), scope);
        feed.until(|view| view.connected).await;
        execute(
            &fixture.client,
            Command::RemoveSession {
                session: session.id,
                expected_revision: session.revision,
            },
        )
        .await;
        let removed = feed
            .until(|view| {
                view.error
                    .as_ref()
                    .is_some_and(|fault| fault.code == ErrorCode::NotFound)
            })
            .await;
        assert!(removed.entries.is_empty());
        feed.close().await;
        fixture.close().await;
    }
}

#[tokio::test]
async fn recomputes_after_restart() {
    for remote in [false, true] {
        let fixture = Fixture::new(remote).await;
        let package = fixture
            .install("status-fixture", SOURCE, &[Change::Package], 0)
            .await;
        let scope = Scope {
            worktree: Some(fixture.worktree),
            session: None,
        };
        let context = fixture.context(&package, scope);
        fixture.write(&context, "counter", json!(9), 0).await;
        let mut feed = Feed::new(fixture.client.clone(), scope);
        let before = feed.until(|view| fixture::label(view) == Some("9")).await;
        feed.close().await;
        let Fixture {
            directory,
            node,
            controller,
            client,
            ..
        } = fixture;
        drop(client);
        node.shutdown().await.unwrap();
        let node = Node::start_empty_with_plugin_catalog(
            directory.path().join("node"),
            "http://127.0.0.1:9/",
        )
        .await
        .unwrap();
        let client = Arc::new(Client::new(if remote {
            controller.handle().remote(node.link().address())
        } else {
            node.local()
        }));
        let mut feed = Feed::new(client.clone(), scope);
        let after = feed.until(|view| fixture::label(view) == Some("9")).await;
        assert_eq!(before.entries, after.entries);
        assert!(after.failures.is_empty());
        feed.close().await;
        drop(client);
        controller.close().await.unwrap();
        node.shutdown().await.unwrap();
    }
}
