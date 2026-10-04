use super::*;
use sailry_client::usage::View;
use sailry_link::CancellationToken;
use sailry_protocol::usage::{Dimension, Key, Query};
use tokio::sync::{mpsc, watch};

#[path = "usage/cost.rs"]
mod cost;
#[path = "usage/overview.rs"]
mod overview;
#[path = "usage/transport.rs"]
mod transport;
use transport::Observed;

async fn execute(client: &Client, command: Command) -> Output {
    client.execute(client.prepare(command)).await.unwrap()
}

async fn observed(view: &mut watch::Receiver<View>, ready: impl Fn(&View) -> bool) {
    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            if ready(&view.borrow_and_update()) {
                return;
            }
            view.changed().await.unwrap();
        }
    })
    .await
    .expect("usage report deadline");
}

#[tokio::test]
async fn shares_frozen_usage_after_recovery() {
    for remote in [false, true] {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().join("project");
        std::fs::create_dir(&root).unwrap();
        let profile = directory.path().join("node");
        let node = Node::start(&profile).await.unwrap();
        let controller =
            Link::controller(directory.path().join("controller"), NetworkScope::default())
                .await
                .unwrap();
        let address = controller
            .handle()
            .pair(node.link().invite().unwrap().ticket())
            .await
            .unwrap();
        let inner = if remote {
            controller.handle().remote(address)
        } else {
            node.local()
        };
        let writer = Client::new(inner.clone());
        let server = Server::start(false).await;
        let (session, provider) = configured(&writer, &server.endpoint, &root).await;
        let transport = Arc::new(Observed::new(inner));
        let client = Arc::new(Client::new(transport.clone()));
        let now = chrono::Utc::now().timestamp_millis();
        let query = Query {
            start_ms: now - 86_400_000,
            end_ms: now + 86_400_000,
            dimension: Dimension::Model,
            projects: vec![],
            worktrees: vec![],
            providers: vec![],
            models: vec![],
            before: None,
        };
        let (sender, mut view) = watch::channel(View::default());
        let (refresh, requests) = mpsc::channel(1);
        let stop = CancellationToken::new();
        let worker = tokio::spawn({
            let client = client.clone();
            let stop = stop.clone();
            let query = query.clone();
            async move { client.watch_usage(query, sender, stop, requests).await }
        });
        observed(&mut view, |view| view.connected && view.report.is_some()).await;
        assert!(
            view.borrow()
                .report
                .as_ref()
                .unwrap()
                .totals
                .tokens
                .is_none()
        );
        let Output::QueuedTurn(queued) = execute(
            &writer,
            Command::QueueTurn {
                session: session.id,
                expected_revision: 1,
                message: "Frozen usage".into(),
            },
        )
        .await
        else {
            panic!("turn expected");
        };
        let mut config = session.config.clone();
        config.model = "fixture-b".into();
        execute(
            &writer,
            Command::SetSessionConfig {
                session: session.id,
                expected_revision: 1,
                config,
            },
        )
        .await;
        execute(&writer, Command::StartQueuedTurn { turn: queued.id }).await;
        finished(&writer, session.id, queued.id).await;
        let submit = || Command::SubmitTurn {
            session: session.id,
            expected_revision: 2,
            message: "Current usage".into(),
        };
        let Output::QueuedTurn(next) = execute(&writer, submit()).await else {
            panic!("turn expected");
        };
        finished(&writer, session.id, next.id).await;
        let committed = writer.read_usage(query.clone()).await.unwrap();
        observed(&mut view, |view| {
            !view.refreshing && view.report.as_deref() == Some(&committed)
        })
        .await;
        let complete = view.borrow().report.clone().unwrap();
        assert_eq!(complete.totals.responses, 2);
        assert_eq!(complete.requests.items.len(), 2);
        assert_eq!(complete.requests.items[0].turn, next.id);
        assert_eq!(complete.requests.items[0].model, "fixture-b");
        assert_eq!(complete.requests.items[1].turn, queued.id);
        assert_eq!(complete.requests.items[1].model, "fixture-a");
        assert!(
            complete
                .requests
                .items
                .iter()
                .all(|call| call.session == session.id && call.provider == provider.id)
        );
        assert_eq!(complete.groups.len(), 2);
        assert_eq!(complete.resources.len(), 5);
        assert!(complete.resources.contains(&Key::Model {
            provider: provider.id,
            model: "fixture-a".into()
        }));
        assert!(complete.resources.contains(&Key::Model {
            provider: provider.id,
            model: "fixture-b".into()
        }));
        assert_eq!(complete.totals.tokens.as_ref().unwrap().input, 24);
        assert!(complete.groups.iter().any(|group| group.key
            == Key::Model {
                provider: provider.id,
                model: "fixture-a".into()
            }));
        let mut filtered = query.clone();
        filtered.projects = vec![session.project.unwrap()];
        filtered.worktrees = vec![session.worktree];
        filtered.providers = vec![provider.id];
        filtered.models = vec!["fixture-a".into()];
        assert_eq!(
            client.read_usage(filtered).await.unwrap().totals.responses,
            1
        );
        for mode in [1, 3, 4, 5] {
            transport.mode(mode);
            refresh.send(()).await.unwrap();
            observed(&mut view, |view| !view.refreshing && view.error.is_some()).await;
            assert_eq!(view.borrow().report.as_ref().unwrap(), &complete);
            refresh.send(()).await.unwrap();
            observed(&mut view, |view| !view.refreshing && view.error.is_none()).await;
        }
        transport.mode(2);
        refresh.send(()).await.unwrap();
        transport.held().await;
        let Output::QueuedTurn(last) = execute(&writer, submit()).await else {
            panic!("turn expected");
        };
        finished(&writer, session.id, last.id).await;
        let committed = writer.read_usage(query.clone()).await.unwrap();
        transport.release();
        observed(&mut view, |view| {
            !view.refreshing && view.report.as_deref() == Some(&committed)
        })
        .await;
        let complete = view.borrow().report.clone().unwrap();
        assert_eq!(complete.totals.responses, 3);
        transport.disconnect();
        observed(&mut view, |view| !view.connected).await;
        assert_eq!(view.borrow().report.as_ref().unwrap(), &complete);
        observed(&mut view, |view| {
            view.connected && !view.refreshing && view.error.is_none()
        })
        .await;
        assert_eq!(view.borrow().report.as_ref().unwrap(), &complete);
        let before = transport.reads();
        transport.mode(2);
        refresh.send(()).await.unwrap();
        transport.held().await;
        stop.cancel();
        tokio::time::timeout(Duration::from_secs(1), worker)
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        assert_eq!(transport.reads(), before + 1);
        assert!(!view.borrow().connected && !view.borrow().refreshing);
        controller.close().await.unwrap();
        node.shutdown().await.unwrap();
        let node = Node::start(&profile).await.unwrap();
        let restored = Client::new(node.local())
            .read_usage(query.clone())
            .await
            .unwrap();
        assert_eq!(restored, *complete);
        assert_eq!(server.requests.lock().unwrap().len(), 3);
        node.shutdown().await.unwrap();
    }
}
