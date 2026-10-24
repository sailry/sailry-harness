use super::*;
use crate::store::database::{Database, Storage, conflict};
use tokio::sync::{mpsc, oneshot};

tokio::task_local! {
    static HOLD: (&'static str, mpsc::UnboundedSender<oneshot::Sender<()>>);
}

pub(in crate::store::knowledge::semantic) async fn pause(phase: &str) -> Result<(), Fault> {
    let sender = HOLD
        .try_with(|(held, sender)| (*held == phase).then(|| sender.clone()))
        .ok()
        .flatten();
    if let Some(sender) = sender {
        let (reply, release) = oneshot::channel();
        if sender.send(reply).is_ok() {
            release.await.map_err(storage_error)?;
        }
    }
    Ok(())
}

struct Fixture {
    storage: Storage,
    package: plugin::Reference,
    _root: tempfile::TempDir,
}

impl Fixture {
    async fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        let database = Database::open(&root.path().join("node.turso"), NodeId([19; 32]), None)
            .await
            .unwrap();
        let package = package(&database.connection, 'a', true).await;
        seed(&database.connection).await;
        Self {
            storage: database.into_storage(),
            package,
            _root: root,
        }
    }

    async fn plan(&self, request: Request) -> Plan {
        self.storage
            .read(move |db| {
                Box::pin(async move { prepare(db, &request, &CancellationToken::new()).await })
            })
            .await
            .unwrap()
    }
}

async fn rewrite(db: &Connection, package: &plugin::Reference, index: plugin::storage::Index) {
    let mut write = request(package);
    write.command = Command::WriteIndexedPluginValue {
        key: "meta/one".into(),
        value: json!({"id":"changed","visible":false}),
        index,
        expected_revision: 2,
    };
    let transaction = conflict::begin(db).await.unwrap();
    crate::store::plugins::execute_storage(&transaction, &write)
        .await
        .unwrap();
    conflict::commit(transaction).await.unwrap();
}

#[tokio::test]
async fn plans_keep_rankings_and_eligibility_together() {
    let fixture = Fixture::new().await;
    let request = request(&fixture.package);
    let query = request.clone();
    let other = fixture.storage.handle.lease(false).await.unwrap();
    let (holds, mut entered) = mpsc::unbounded_channel();
    let plan = HOLD.scope(
        ("plan", holds),
        fixture.storage.read(move |db| {
            Box::pin(async move { prepare(db, &query, &CancellationToken::new()).await })
        }),
    );
    let change = async {
        let release = entered.recv().await.unwrap();
        let mut index = metadata("bodies/page", "/bodies/escaped~1id/body");
        index.source = None;
        rewrite(&other, &fixture.package, index).await;
        drop(entered);
        release.send(()).unwrap();
    };
    let (plan, ()) = tokio::join!(plan, change);
    let plan = plan.unwrap();
    assert!(plan.keyword.contains(&("meta/one".into(), 2)));
    assert!(plan.eligible.contains("meta/one"));
    let latest = fixture.plan(request).await;
    assert!(latest.keyword.contains(&("meta/one".into(), 3)));
    assert!(!latest.eligible.contains("meta/one"));
}

#[tokio::test]
async fn pages_pin_tags_and_authorized_content() {
    let fixture = Fixture::new().await;
    let mut request = request(&fixture.package);
    let Command::SearchPluginValues(query) = &mut request.command else {
        unreachable!()
    };
    query.terms = vec!["no lexical match".into()];
    query.all = vec!["visible".into()];
    let plan = fixture.plan(request.clone()).await;
    assert!(plan.keyword.is_empty());
    let other = fixture.storage.handle.lease(false).await.unwrap();
    let raw = source(
        &other,
        &plan.collections[0],
        "meta/one",
        &CancellationToken::new(),
    )
    .await
    .unwrap()
    .unwrap();
    let selected = selection(&plan, &raw);
    let query = request.clone();
    let captured = plan.clone();
    let (holds, mut entered) = mpsc::unbounded_channel();
    let page = HOLD.scope(
        ("page", holds),
        fixture.storage.read(move |db| {
            Box::pin(async move {
                materialize(db, &query, &captured, &selected, &CancellationToken::new()).await
            })
        }),
    );
    let change = async {
        let release = entered.recv().await.unwrap();
        let mut index = metadata("bodies/page", "/bodies/escaped~1id/body");
        index.tags = vec!["hidden".into()];
        rewrite(&other, &fixture.package, index).await;
        drop(entered);
        release.send(()).unwrap();
    };
    let (page, ()) = tokio::join!(page, change);
    let page = page.unwrap();
    assert!(!page.stale);
    assert_eq!(page.page.mode, knowledge::Mode::Semantic);
    assert_eq!(page.page.entries.len(), 1);
    assert_eq!(page.page.entries[0].revision, 2);
    assert_eq!(
        page.page.entries[0].value,
        json!({"id":"one","visible":true})
    );
    let selected = selection(&plan, &raw);
    let latest = fixture
        .storage
        .read(move |db| {
            Box::pin(async move {
                materialize(db, &request, &plan, &selected, &CancellationToken::new()).await
            })
        })
        .await
        .unwrap();
    assert!(latest.stale);
    assert!(latest.page.entries.is_empty());
}
