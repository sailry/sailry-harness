use super::*;
use sailry_protocol::{
    plugin::desktop::Surface,
    usage::{Dimension, Query},
};

#[tokio::test]
async fn binds_canonical_reads_to_enabled_page_packages() {
    for remote in [false, true] {
        let (directory, node, controller, client, worktree) = fixture(remote).await;
        let Output::Plugin(package) = execute(
            &client,
            Command::ReadPlugin {
                name: "statistics".into(),
            },
        )
        .await
        else {
            panic!("package expected")
        };
        let mut context = Context {
            invocation: None,
            turn: None,
            surface: Surface::Workspace,
            package: package.summary.reference(),
            worktree: None,
            session: None,
        };
        let query = Query {
            start_ms: 0,
            end_ms: 86400000,
            dimension: Dimension::Model,
            projects: vec![],
            worktrees: vec![],
            providers: vec![],
            models: vec![],
            before: None,
        };
        let scoped = client.with_usage_scope(context.clone());
        let mut foreign = context.clone();
        foreign.package.settings_revision += 1;
        assert!(
            client
                .with_usage_scope(foreign.clone())
                .read_usage(query.clone())
                .await
                .is_err()
        );
        let matching = client.with_matching_usage_scope(foreign);
        let report = scoped.read_usage(query.clone()).await.unwrap();
        assert_eq!(report.node, client.target());
        assert_eq!(report.query, query);
        assert_eq!(report.totals.responses, 0);
        for rejected in [
            Context {
                surface: Surface::Composer,
                ..context.clone()
            },
            Context {
                surface: Surface::Project,
                ..context.clone()
            },
            Context {
                turn: Some(TurnId::new()),
                ..context.clone()
            },
            Context {
                invocation: Some(RequestId::new()),
                ..context.clone()
            },
            Context {
                surface: Surface::Settings,
                ..context.clone()
            },
        ] {
            assert_eq!(
                client
                    .with_usage_scope(rejected)
                    .read_usage(query.clone())
                    .await
                    .unwrap_err()
                    .code,
                ErrorCode::PermissionDenied
            );
        }
        let (updates, mut observed) =
            tokio::sync::watch::channel(sailry_client::usage::View::default());
        let (_refresh, requests) = tokio::sync::mpsc::channel(1);
        let stop = sailry_link::CancellationToken::new();
        let worker = tokio::spawn({
            let client = client.with_usage_scope(context.clone());
            let stop = stop.clone();
            let query = query.clone();
            async move { client.watch_usage(query, updates, stop, requests).await }
        });
        tokio::time::timeout(
            Duration::from_secs(10),
            observed
                .wait_for(|view| view.report.is_some() && !view.refreshing && view.error.is_none()),
        )
        .await
        .expect("initial usage report deadline")
        .unwrap();
        assert!(package.extension.as_ref().unwrap().settings_page.is_none());
        context.worktree = Some(worktree);
        assert_eq!(
            client
                .with_usage_scope(context.clone())
                .read_usage(query.clone())
                .await
                .unwrap(),
            report
        );
        context.surface = Surface::Composer;
        assert_eq!(
            client
                .with_usage_scope(context.clone())
                .read_usage(query.clone())
                .await
                .unwrap_err()
                .code,
            ErrorCode::PermissionDenied
        );
        let Output::Plugin(disabled) = execute(
            &client,
            Command::SetPluginEnabled {
                name: "statistics".into(),
                expected_revision: package.summary.revision,
                enabled: false,
            },
        )
        .await
        else {
            panic!("package expected")
        };
        context.package = disabled.summary.reference();
        context.surface = Surface::Workspace;
        context.worktree = None;
        assert_eq!(
            client
                .with_usage_scope(context)
                .read_usage(query.clone())
                .await
                .unwrap_err()
                .code,
            ErrorCode::NotConfigured
        );
        assert_eq!(
            matching.read_usage(query.clone()).await.unwrap_err().code,
            ErrorCode::NotConfigured
        );
        tokio::time::timeout(
            Duration::from_secs(10),
            observed.wait_for(|view| {
                view.error
                    .as_ref()
                    .is_some_and(|error| error.code == ErrorCode::NotConfigured)
                    && !view.refreshing
            }),
        )
        .await
        .expect("revoked usage report deadline")
        .unwrap();
        assert!(observed.borrow().report.is_none());
        execute(
            &client,
            Command::SetPluginEnabled {
                name: "statistics".into(),
                expected_revision: disabled.summary.revision,
                enabled: true,
            },
        )
        .await;
        assert!(matching.read_usage(query.clone()).await.is_ok());
        assert!(scoped.read_usage(query.clone()).await.is_ok());
        tokio::time::timeout(
            Duration::from_secs(10),
            observed
                .wait_for(|view| view.report.is_some() && !view.refreshing && view.error.is_none()),
        )
        .await
        .expect("restored usage report deadline")
        .unwrap();
        stop.cancel();
        worker.await.unwrap().unwrap();
        let root = directory.path().join("source/package");
        declare(&root, &[Action::ReadUsage]);
        let path = root.join("plugin.json");
        let mut manifest: serde_json::Value =
            serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        manifest["extensions"]["dev.sailry.platform"]["settings_page"] =
            serde_json::json!({"navigation":{"label":"Usage"}});
        fs::write(&path, manifest.to_string()).unwrap();
        let independent = info(execute(&client, install(worktree, 0)).await);
        for surface in [Surface::Workspace, Surface::Settings] {
            let context = Context {
                package: independent.summary.reference(),
                surface,
                worktree: None,
                session: None,
                turn: None,
                invocation: None,
            };
            assert_eq!(
                client
                    .with_usage_scope(context)
                    .read_usage(query.clone())
                    .await
                    .unwrap()
                    .totals
                    .responses,
                0
            );
        }
        declare(&root, &[]);
        let undeclared =
            info(execute(&client, install(worktree, independent.summary.revision)).await);
        let context = Context {
            package: undeclared.summary.reference(),
            surface: Surface::Workspace,
            worktree: None,
            session: None,
            turn: None,
            invocation: None,
        };
        assert_eq!(
            client
                .with_usage_scope(context)
                .read_usage(query.clone())
                .await
                .unwrap_err()
                .code,
            ErrorCode::PermissionDenied
        );
        assert_eq!(client.read_usage(query).await.unwrap().totals.responses, 0);
        controller.close().await.unwrap();
        node.shutdown().await.unwrap();
    }
}
