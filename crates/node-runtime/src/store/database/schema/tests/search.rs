use super::*;
use crate::store::{database::Database, plugins::storage::search};
use turso::params;

async fn value(connection: &Connection) -> Vec<u8> {
    connection
        .prepare("SELECT value FROM plugin_values WHERE name='fixture' AND key='kept'")
        .await
        .unwrap()
        .query_row(())
        .await
        .unwrap()
        .get(0)
        .unwrap()
}

#[tokio::test]
async fn restores_missing_metadata_with_backup() {
    for corpus in [false, true] {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("node.turso");
        let node = NodeId([43; 32]);
        let database = Database::open(&path, node, None).await.unwrap();
        let connection = &database.connection;
        let body = br#"{"private":"kept"}"#;
        let index = serde_json::to_vec(&sailry_protocol::plugin::storage::Index {
            fields: ["literal".into(), String::new()],
            tags: vec!["retained".into()],
            order: 1,
            source: None,
        })
        .unwrap();
        connection.execute("BEGIN", ()).await.unwrap();
        connection
            .execute(
                "INSERT INTO plugin_values(name,key,revision,value,index_data) VALUES('fixture','kept',1,?1,?2)",
                params![body.as_slice(), corpus.then_some(index.as_slice())],
            )
            .await
            .unwrap();
        for definition in search::definitions(connection, true).await.unwrap() {
            connection.execute_batch(definition).await.unwrap();
        }
        connection
            .execute("UPDATE plugin_values SET index_data=NULL", ())
            .await
            .unwrap();
        connection.execute("COMMIT", ()).await.unwrap();
        let declarations = search::definitions(connection, true).await.unwrap();
        assert_eq!(declarations.len(), if corpus { 2 } else { 0 });
        connection
            .execute("ALTER TABLE plugin_values DROP COLUMN index_data", ())
            .await
            .unwrap();
        let before = objects(connection).await.unwrap();
        assert!(
            prepare(connection, &path, node, None)
                .await
                .unwrap()
                .needs_update()
        );
        assert_eq!(objects(connection).await.unwrap(), before);
        assert_eq!(value(connection).await, body);
        database.close().await.unwrap();

        let database = Database::open(&path, node, None).await.unwrap();
        let connection = &database.connection;
        assert_eq!(value(connection).await, body);
        assert!(
            connection
                .prepare("SELECT index_data FROM plugin_values WHERE name='fixture' AND key='kept'")
                .await
                .unwrap()
                .query_row(())
                .await
                .unwrap()
                .get::<Option<Vec<u8>>>(0)
                .unwrap()
                .is_none()
        );
        assert_eq!(
            search::definitions(connection, true).await.unwrap(),
            declarations
        );
        search::validate(connection).await.unwrap();
        database.close().await.unwrap();

        let backups = std::fs::read_dir(directory.path().join("backups"))
            .unwrap()
            .map(|entry| entry.unwrap().path().join("node.turso"))
            .collect::<Vec<_>>();
        assert_eq!(backups.len(), 1);
        let backup = turso::Builder::new_local(backups[0].to_str().unwrap())
            .experimental_index_method(true)
            .read_only(true)
            .build()
            .await
            .unwrap();
        let connection = backup.connect().unwrap();
        assert_eq!(objects(&connection).await.unwrap(), before);
        assert_eq!(value(&connection).await, body);
    }
}
