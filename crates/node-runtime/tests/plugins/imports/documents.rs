use super::*;
use sailry_protocol::plugin::skills::{Document, DocumentSource};

fn command(package: &plugin::Reference, skill: &str) -> Command {
    Command::ReadSkillDocument {
        source: DocumentSource::Installed {
            package: package.clone(),
            skill: skill.into(),
        },
    }
}

async fn read(client: &Client, package: &plugin::Reference) -> Document {
    let pending = client
        .dispatch(client.prepare(command(package, "analysis")))
        .await
        .unwrap();
    assert!(!pending.receipt.durable);
    let Output::SkillDocument(document) = pending.completion.await.unwrap().unwrap() else {
        panic!("skill document expected");
    };
    document
}

async fn discover(client: &Client, worktree: WorktreeId) -> ImportDiscovery {
    let Output::SkillImportDiscovery(discovery) = execute(
        client,
        Command::DiscoverLocalSkills {
            worktree,
            path: "imports".into(),
        },
    )
    .await
    else {
        panic!("skill discovery expected");
    };
    discovery
}

#[tokio::test]
async fn reads_retained_versions_without_source_access() {
    for remote in [false, true] {
        let (directory, node, controller, client, worktree) = support::empty(remote).await;
        let root = directory.path().join("source/imports/analysis");
        fs::create_dir_all(&root).unwrap();
        fs::write(
            root.join("SKILL.md"),
            "---\nname: analysis\ndescription: Inspect a sample\n---\nOriginal body\n",
        )
        .unwrap();
        let discovery = discover(&client, worktree).await;
        let first = info(execute(&client, install(&discovery, 0)).await);
        let original = read(&client, &first.summary.reference()).await;
        assert_eq!(original.skill, first.skills[0]);
        assert_eq!(original.content, "Original body\n");

        fs::write(
            root.join("SKILL.md"),
            "---\nname: analysis\ndescription: Inspect revised data\n---\nUpdated body\n",
        )
        .unwrap();
        assert_eq!(read(&client, &first.summary.reference()).await, original);
        let discovery = discover(&client, worktree).await;
        let second = info(execute(&client, install(&discovery, 1)).await);
        assert_eq!(second.summary.name, first.summary.name);
        assert_ne!(second.summary.digest, first.summary.digest);
        let updated = read(&client, &second.summary.reference()).await;
        assert_eq!(updated.skill, second.skills[0]);
        assert_eq!(updated.content, "Updated body\n");
        fs::remove_dir_all(directory.path().join("source/imports")).unwrap();
        let disabled = info(
            execute(
                &client,
                Command::SetPluginEnabled {
                    name: second.summary.name.clone(),
                    expected_revision: second.summary.revision,
                    enabled: false,
                },
            )
            .await,
        );
        assert!(!disabled.summary.enabled);
        execute(
            &client,
            Command::RemovePlugin {
                name: disabled.summary.name,
                expected_revision: disabled.summary.revision,
            },
        )
        .await;
        let inventory = execute(&client, Command::ListPlugins).await;
        let db = rusqlite::Connection::open_with_flags(
            node.profile().join("storage/node.sqlite3"),
            rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
        )
        .unwrap();
        let retained = || {
            db.query_row("SELECT count(*) FROM requests", [], |row| {
                row.get::<_, i64>(0)
            })
            .unwrap()
        };
        let before = retained();
        assert_eq!(read(&client, &first.summary.reference()).await, original);
        assert_eq!(read(&client, &second.summary.reference()).await, updated);
        for skill in [
            "unknown",
            "../private",
            "analysis/SKILL.md",
            "analysis:other",
        ] {
            assert_eq!(
                client
                    .execute(client.prepare(command(&second.summary.reference(), skill)))
                    .await
                    .unwrap_err()
                    .code,
                ErrorCode::NotFound
            );
        }
        let reference = second.summary.reference();
        for invalid in [
            plugin::Reference {
                name: "unknown".into(),
                ..reference.clone()
            },
            plugin::Reference {
                digest: "0".repeat(64),
                ..reference.clone()
            },
            plugin::Reference {
                settings_revision: 999,
                ..reference.clone()
            },
        ] {
            assert_eq!(
                client
                    .execute(client.prepare(command(&invalid, "analysis")))
                    .await
                    .unwrap_err()
                    .code,
                ErrorCode::NotConfigured
            );
        }
        let mut wrong = client.prepare(command(&reference, "analysis"));
        wrong.target = NodeId([0; 32]);
        assert_eq!(
            client.execute(wrong).await.unwrap_err().code,
            ErrorCode::WrongTarget
        );
        assert_eq!(execute(&client, Command::ListPlugins).await, inventory);
        assert_eq!(retained(), before);
        controller.close().await.unwrap();
        node.shutdown().await.unwrap();
    }
}
