use sailry_client::Client;
use sailry_link::{Link, NetworkScope};
use sailry_node_runtime::Node;
use sailry_protocol::{Command, Output};
use std::{ffi::OsString, path::Path};

#[tokio::test]
async fn preserves_current_resources_on_both_paths() {
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
        let files = entries(&profile.join("storage"));

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
        let database = rusqlite::Connection::open_with_flags(
            profile.join("storage/node.sqlite3"),
            rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
        )
        .unwrap();
        assert_eq!(
            database
                .pragma_query_value::<i64, _>(None, "user_version", |row| row.get(0))
                .unwrap(),
            1
        );
        database.close().unwrap();
        drop(client);
        node.shutdown().await.unwrap();
        assert_eq!(entries(&profile.join("storage")), files);
        controller.close().await.unwrap();
    }
}

#[tokio::test]
async fn rejects_changes_without_rewriting_profile() {
    for (sql, detail) in [
        ("DROP TABLE model_catalog_status", "model_catalog_status"),
        (
            "ALTER TABLE plugin_values DROP COLUMN index_data",
            "plugin_values.index_data",
        ),
        ("DROP INDEX agent_events_turn", "agent_events_turn"),
        (
            "ALTER TABLE agent_runs ADD COLUMN unused BLOB",
            "agent_runs.unused",
        ),
    ] {
        let directory = tempfile::tempdir().unwrap();
        let profile = directory.path().join("node");
        let node = Node::start(&profile).await.unwrap();
        node.shutdown().await.unwrap();
        let path = profile.join("storage/node.sqlite3");
        let database = rusqlite::Connection::open(&path).unwrap();
        database.execute_batch(sql).unwrap();
        database
            .pragma_update(None, "journal_mode", "DELETE")
            .unwrap();
        database.close().unwrap();

        let before = std::fs::read(&path).unwrap();
        let files = entries(&profile.join("storage"));
        let error = Node::start(&profile)
            .await
            .err()
            .expect("incompatible profile must be rejected");
        let message = error.to_string();
        assert!(
            message.contains("incompatible development database"),
            "{message}"
        );
        assert!(message.contains(detail), "{message}");
        assert!(
            message.contains("existing data was not changed"),
            "{message}"
        );
        assert_eq!(std::fs::read(&path).unwrap(), before);
        assert_eq!(entries(&profile.join("storage")), files);
    }
}

async fn execute(client: &Client, command: Command) -> Output {
    client.execute(client.prepare(command)).await.unwrap()
}

fn entries(directory: &Path) -> Vec<OsString> {
    let mut entries = std::fs::read_dir(directory)
        .unwrap()
        .map(|entry| entry.unwrap().file_name())
        .collect::<Vec<_>>();
    entries.sort();
    entries
}
