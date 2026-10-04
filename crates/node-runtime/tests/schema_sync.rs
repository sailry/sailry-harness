use sailry_client::Client;
use sailry_link::{Link, NetworkScope};
use sailry_node_runtime::Node;
use sailry_protocol::{Command, Output};

#[tokio::test]
async fn keeps_resources_and_configuration_on_both_paths() {
    for remote in [false, true] {
        let directory = tempfile::tempdir().unwrap();
        let profile = directory.path().join("node");
        let node = Node::start(&profile).await.unwrap();
        let identity = node.id();
        let controller =
            Link::controller(directory.path().join("controller"), NetworkScope::default())
                .await
                .unwrap();
        let address = controller
            .handle()
            .pair(node.link().invite().unwrap().ticket())
            .await
            .unwrap();
        let client = Client::new(if remote {
            controller.handle().remote(address)
        } else {
            node.local()
        });
        let source = directory.path().join("source");
        std::fs::create_dir(&source).unwrap();
        execute(
            &client,
            Command::RegisterProject {
                name: "Schema fixture".into(),
                path: source.to_str().unwrap().into(),
            },
        )
        .await;
        let Output::Plugins(plugins) = execute(&client, Command::ListPlugins).await else {
            panic!("plugin list expected");
        };
        let goals = plugins
            .iter()
            .find(|plugin| plugin.name == "goals")
            .unwrap();
        let Output::Plugin(goals) = execute(
            &client,
            Command::SetPluginEnabled {
                name: "goals".into(),
                expected_revision: goals.revision,
                enabled: false,
            },
        )
        .await
        else {
            panic!("plugin information expected");
        };
        let Output::Snapshot(before) = execute(&client, Command::Snapshot).await else {
            panic!("snapshot expected");
        };
        drop(client);
        node.shutdown().await.unwrap();

        let database_path = profile.join("storage/node.sqlite3");
        let database = rusqlite::Connection::open(&database_path).unwrap();
        database.execute_batch("ALTER TABLE plugin_values DROP COLUMN index_data; DROP INDEX agent_events_turn; DROP TABLE model_catalog_status; CREATE TABLE retained(value BLOB NOT NULL)").unwrap();
        database
            .execute(
                "INSERT INTO retained(value) VALUES(?1)",
                [b"unmodified".as_slice()],
            )
            .unwrap();
        database.close().unwrap();

        let node = Node::start(&profile).await.unwrap();
        assert_eq!(node.id(), identity);
        let client = Client::new(if remote {
            controller.handle().remote(node.link().address())
        } else {
            node.local()
        });
        let Output::Snapshot(after) = execute(&client, Command::Snapshot).await else {
            panic!("snapshot expected");
        };
        assert_eq!(after.projects, before.projects);
        assert_eq!(after.worktrees, before.worktrees);
        let Output::Plugin(restored) = execute(
            &client,
            Command::ReadPlugin {
                name: "goals".into(),
            },
        )
        .await
        else {
            panic!("plugin information expected");
        };
        assert_eq!(restored, goals);
        drop(client);
        node.shutdown().await.unwrap();

        let database = rusqlite::Connection::open(&database_path).unwrap();
        assert!(
            database
                .prepare("SELECT index_data FROM plugin_values")
                .is_ok()
        );
        assert!(
            database
                .prepare("SELECT body FROM model_catalog_status")
                .is_ok()
        );
        assert!(
            database
                .query_row(
                    "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE name='agent_events_turn')",
                    [],
                    |row| row.get::<_, bool>(0)
                )
                .unwrap()
        );
        assert_eq!(
            database
                .query_row("SELECT value FROM retained", [], |row| row
                    .get::<_, Vec<u8>>(0))
                .unwrap(),
            b"unmodified"
        );
        assert_eq!(
            database
                .pragma_query_value::<i64, _>(None, "user_version", |row| row.get(0))
                .unwrap(),
            1
        );
        let backups = std::fs::read_dir(profile.join("storage"))
            .unwrap()
            .filter(|entry| {
                entry
                    .as_ref()
                    .unwrap()
                    .file_name()
                    .to_string_lossy()
                    .starts_with("schema-backup-")
            })
            .count();
        assert_eq!(backups, 1);
        database.close().unwrap();
        controller.close().await.unwrap();
    }
}

async fn execute(client: &Client, command: Command) -> Output {
    client.execute(client.prepare(command)).await.unwrap()
}
