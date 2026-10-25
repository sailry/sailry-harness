use super::*;
use crate::code_index::Service;
use sailry_protocol::NodeId;

#[tokio::test]
async fn binds_unicode_across_statement_boundaries() {
    let directory = tempfile::tempdir().unwrap();
    let stop = CancellationToken::new();
    let service = Service::new(
        Some(directory.path().join("profile")),
        NodeId([83; 32]),
        Arc::default(),
        stop.clone(),
    );
    let resources = service.resources().await.unwrap();
    let index = Index::open(&directory.path().join("index.turso"))
        .await
        .unwrap();
    for (revision, count) in [BATCH_SYMBOLS - 1, BATCH_SYMBOLS, BATCH_SYMBOLS + 1]
        .into_iter()
        .enumerate()
    {
        let path = format!("模块_{count}.rs");
        let names: Vec<_> = (0..count).map(|number| format!("声明_{number}")).collect();
        let source: String = names
            .iter()
            .map(|name| format!("pub fn {name}() {{}}\n"))
            .collect();
        let file = resources
            .parser
            .parse(path.clone(), &source, &stop)
            .unwrap();
        assert_eq!(file.symbols.len(), count);
        index
            .replace(&[file], &resources.fingerprint, &stop)
            .await
            .unwrap();
        let lease = index.lease(true).await.unwrap();
        let mut rows = lease
            .query(
                "SELECT name FROM symbols WHERE path=?1 ORDER BY id",
                [path.as_str()],
            )
            .await
            .unwrap();
        let mut stored = Vec::new();
        while let Some(row) = rows.next().await.unwrap() {
            stored.push(row.get::<String>(0).unwrap());
        }
        assert_eq!(stored, names);
        drop(rows);
        drop(lease);
        let page = index
            .search(
                &Search {
                    query: names[count - 1].clone(),
                    name: Some(names[count - 1].clone()),
                    path: Some(path.clone()),
                    language: Some("rust".into()),
                    ..Default::default()
                },
                &stop,
            )
            .await
            .unwrap();
        assert_eq!(page.symbols.len(), 1);
        assert_eq!(page.symbols[0].name, names[count - 1]);
        assert_eq!(page.index_revision, revision as u64 + 1);
        assert_eq!(
            page.symbols[0].file_revision,
            blake3::hash(source.as_bytes()).to_hex().to_string()
        );
    }
}
