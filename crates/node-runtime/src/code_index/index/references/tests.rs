use super::*;
use sailry_protocol::NodeId;

async fn usages(
    index: &Index,
    name: &str,
) -> (Vec<(String, Usage)>, BTreeMap<String, Vec<String>>) {
    let stop = CancellationToken::new();
    let mut lease = index.lease(true).await.unwrap();
    let tx = lease.connection_mut().transaction().await.unwrap();
    let reader = Reader::new(&tx, &stop);
    let found = reader
        .references(&BTreeSet::from([name.to_owned()]), 10)
        .await
        .unwrap();
    let imports = reader
        .imports(&BTreeSet::from(["main.py".to_owned()]))
        .await
        .unwrap();
    tx.commit().await.unwrap();
    (found, imports)
}

#[tokio::test]
async fn replaces_and_removes_rows_with_files() {
    let directory = tempfile::tempdir().unwrap();
    let stop = CancellationToken::new();
    let service = super::super::super::Service::new(
        Some(directory.path().join("profile")),
        NodeId([84; 32]),
        Arc::default(),
        stop.clone(),
    );
    let resources = service.resources().await.unwrap();
    let storage = directory.path().join("storage");
    crate::profile::private_directory(&storage).unwrap();
    let index = Index::open(&storage.join("index.turso")).await.unwrap();
    let parse = |source: &str| {
        resources
            .parser
            .parse("main.py".into(), source, &stop)
            .unwrap()
    };
    let calling = "import helpers\n\ndef run():\n    helpers.go()\n";
    index
        .replace(&[parse(calling)], &resources.fingerprint, &stop)
        .await
        .unwrap();
    let (found, imports) = usages(&index, "go").await;
    assert_eq!(found.len(), 1);
    let (path, usage) = &found[0];
    assert_eq!(path, "main.py");
    assert_eq!(
        (usage.kind.as_str(), usage.line, usage.column),
        ("call", 3, 12)
    );
    assert_eq!(usage.scope.as_ref().unwrap().name, "run");
    assert_eq!(imports["main.py"], ["helpers"]);

    index
        .replace(
            &[parse("def run():\n    pass\n")],
            &resources.fingerprint,
            &stop,
        )
        .await
        .unwrap();
    let (found, imports) = usages(&index, "go").await;
    assert!(found.is_empty() && imports.is_empty());

    index
        .replace(&[parse(calling)], &resources.fingerprint, &stop)
        .await
        .unwrap();
    index.remove(&["main.py".to_owned()], &stop).await.unwrap();
    let (found, imports) = usages(&index, "go").await;
    assert!(found.is_empty() && imports.is_empty());
}
