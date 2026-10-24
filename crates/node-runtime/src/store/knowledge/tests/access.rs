use super::*;

#[tokio::test]
async fn admitted_operations_retain_package_after_removal() {
    let db = database().await;
    let owner = package(&db, "owner", 'a', &["storage.read"]).await;
    let original = insert(&db, &owner, storage()).await;
    package(&db, "owner", 'b', &[]).await;
    db.execute("UPDATE plugins SET installed=0 WHERE name='owner'", ())
        .await
        .unwrap();
    let ordinary = captured(&owner, Command::ListKnowledgeCollections);
    assert_eq!(
        list(&db, &ordinary).await.unwrap_err().code,
        ErrorCode::NotFound
    );
    // Turn and invocation authority is checked by the shared admission boundary.
    // This native fixture exercises the handlers' immutable package resolution.
    for callback in [false, true] {
        let mut admitted = captured(&owner, Command::ListKnowledgeCollections);
        let context = admitted.plugin.as_mut().unwrap();
        if callback {
            context.invocation = Some(sailry_protocol::RequestId::new());
        } else {
            context.turn = Some(sailry_protocol::TurnId::new());
        }
        assert_eq!(list(&db, &admitted).await.unwrap().len(), 1);
        admitted.command = Command::RebuildKnowledgeCollection {
            id: original.id,
            expected_revision: if callback { 2 } else { 1 },
        };
        let rebuilt = create(&db, &admitted).await;
        assert_eq!(rebuilt.revision, if callback { 3 } else { 2 });
        assert_eq!(
            catalog(&db).await.unwrap().collections[0].capture.package,
            Some(owner.clone())
        );
    }
}

#[tokio::test]
async fn captures_owner_without_caller_override() {
    let db = database().await;
    let owner = package(&db, "owner", 'a', &["storage.read"]).await;
    package(&db, "other", 'b', &["storage.read"]).await;
    for selected in [Some("other".into()), Some("owner".into())] {
        let denied = captured(
            &owner,
            Command::CreateKnowledgeCollection {
                definition: storage(),
                plugin: selected,
            },
        );
        assert_eq!(
            execute(&db, &denied).await.unwrap_err().code,
            ErrorCode::PermissionDenied
        );
    }
    assert!(raw(&db).await.is_empty());
    let collection = insert(&db, &owner, storage()).await;
    assert_eq!(
        collection.owner,
        Owner::Plugin {
            name: "owner".into()
        }
    );
    let stored = catalog(&db).await.unwrap().collections.remove(0);
    assert_eq!(stored.capture.package, Some(owner));
    assert_eq!(stored.capture.worktree, None);
    assert_eq!(stored.capture.session, None);
    let absent = request(Command::CreateKnowledgeCollection {
        definition: storage(),
        plugin: Some("missing".into()),
    });
    assert_eq!(
        execute(&db, &absent).await.unwrap_err().code,
        ErrorCode::NotFound
    );
    let unowned = request(Command::CreateKnowledgeCollection {
        definition: storage(),
        plugin: None,
    });
    assert_eq!(
        execute(&db, &unowned).await.unwrap_err().code,
        ErrorCode::PermissionDenied
    );
    assert_eq!(raw(&db).await.len(), 1);
}

#[tokio::test]
async fn sharing_requires_controller_and_current_package() {
    let db = database().await;
    let owner = package(&db, "owner", 'a', &["storage.read"]).await;
    let reader = package(&db, "reader", 'b', &[]).await;
    let outsider = package(&db, "outsider", 'c', &["storage.read"]).await;
    let mut shared = storage();
    shared.shared_with = vec!["reader".into()];
    assert_eq!(
        execute(
            &db,
            &captured(
                &owner,
                Command::CreateKnowledgeCollection {
                    definition: shared.clone(),
                    plugin: None
                }
            )
        )
        .await
        .unwrap_err()
        .code,
        ErrorCode::PermissionDenied
    );
    let collection = create(
        &db,
        &request(Command::CreateKnowledgeCollection {
            definition: shared,
            plugin: Some("owner".into()),
        }),
    )
    .await;
    assert_eq!(
        allowed(&db, &captured(&reader, Command::ListKnowledgeCollections))
            .await
            .unwrap()
            .len(),
        1
    );
    assert!(
        allowed(&db, &captured(&outsider, Command::ListKnowledgeCollections))
            .await
            .unwrap()
            .is_empty()
    );
    let mutation = captured(
        &reader,
        Command::DeleteKnowledgeCollection {
            id: collection.id,
            expected_revision: 1,
        },
    );
    assert_eq!(
        execute(&db, &mutation).await.unwrap_err().code,
        ErrorCode::PermissionDenied
    );
    let mut definition = collection.definition.clone();
    definition.shared_with.clear();
    let update = captured(
        &owner,
        Command::UpdateKnowledgeCollection {
            id: collection.id,
            expected_revision: 1,
            definition: definition.clone(),
        },
    );
    assert_eq!(
        execute(&db, &update).await.unwrap_err().code,
        ErrorCode::PermissionDenied
    );
    let update = request(Command::UpdateKnowledgeCollection {
        id: collection.id,
        expected_revision: 1,
        definition,
    });
    create(&db, &update).await;
    assert!(
        allowed(&db, &captured(&reader, Command::ListKnowledgeCollections))
            .await
            .unwrap()
            .is_empty()
    );
    let replacement = package(&db, "owner", 'd', &["storage.read"]).await;
    assert_eq!(
        list(&db, &captured(&owner, Command::ListKnowledgeCollections))
            .await
            .unwrap_err()
            .code,
        ErrorCode::RevisionConflict
    );
    let rebuilt = create(
        &db,
        &captured(
            &replacement,
            Command::RebuildKnowledgeCollection {
                id: collection.id,
                expected_revision: 2,
            },
        ),
    )
    .await;
    assert_eq!(rebuilt.revision, 3);
    assert_eq!(
        catalog(&db).await.unwrap().collections[0].capture.package,
        Some(replacement.clone())
    );
    package(&db, "owner", 'e', &[]).await;
    let stale = captured(
        &replacement,
        Command::RebuildKnowledgeCollection {
            id: collection.id,
            expected_revision: 3,
        },
    );
    assert_eq!(
        execute(&db, &stale).await.unwrap_err().code,
        ErrorCode::RevisionConflict
    );
    let current = package(&db, "owner", 'f', &[]).await;
    assert!(
        list(&db, &captured(&current, Command::ListKnowledgeCollections))
            .await
            .unwrap()
            .is_empty()
    );
    assert_eq!(
        execute(
            &db,
            &captured(
                &current,
                Command::RebuildKnowledgeCollection {
                    id: collection.id,
                    expected_revision: 3
                }
            )
        )
        .await
        .unwrap_err()
        .code,
        ErrorCode::PermissionDenied
    );
}

#[tokio::test]
async fn confines_worktree_and_conversation_sources() {
    let db = database().await;
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().canonicalize().unwrap();
    std::fs::write(root.join("guide.txt"), "Reference").unwrap();
    let tree = worktree(&db, &root).await;
    let owner = package(
        &db,
        "owner",
        'a',
        &["files.read", "storage.read", "conversation.read"],
    )
    .await;
    let mut command = captured(
        &owner,
        Command::CreateKnowledgeCollection {
            definition: definition(Source::Worktree {
                worktree: tree,
                path: "guide.txt".into(),
            }),
            plugin: None,
        },
    );
    assert_eq!(
        execute(&db, &command).await.unwrap_err().code,
        ErrorCode::PermissionDenied
    );
    command.plugin.as_mut().unwrap().worktree = Some(tree);
    let collection = create(&db, &command).await;
    assert_eq!(
        catalog(&db).await.unwrap().collections[0].capture.worktree,
        Some(tree)
    );
    assert!(
        list(&db, &captured(&owner, Command::ListKnowledgeCollections))
            .await
            .unwrap()
            .is_empty()
    );
    let mut read = captured(&owner, Command::ListKnowledgeCollections);
    read.plugin.as_mut().unwrap().worktree = Some(tree);
    assert_eq!(list(&db, &read).await.unwrap(), [collection]);
    let mut definition = definition(Source::Storage {
        scope: Scope::Conversation,
        prefix: "reference/".into(),
    });
    command.command = Command::CreateKnowledgeCollection {
        definition: definition.clone(),
        plugin: None,
    };
    assert_eq!(
        execute(&db, &command).await.unwrap_err().code,
        ErrorCode::PermissionDenied
    );
    let session = SessionId::new();
    db.execute(
        "INSERT INTO sessions(rowid,id,worktree,revision) VALUES(1,?1,?2,1)",
        params![session.to_string(), tree.to_string()],
    )
    .await
    .unwrap();
    command.plugin.as_mut().unwrap().session = Some(session);
    let conversation = create(&db, &command).await;
    assert_eq!(list(&db, &read).await.unwrap().len(), 1);
    read.plugin.as_mut().unwrap().session = Some(session);
    assert_eq!(list(&db, &read).await.unwrap().len(), 2);
    command.plugin.as_mut().unwrap().worktree = Some(WorktreeId::new());
    assert_eq!(
        execute(&db, &command).await.unwrap_err().code,
        ErrorCode::PermissionDenied
    );
    command.plugin.as_mut().unwrap().worktree = Some(tree);
    definition.source = Source::Storage {
        scope: Scope::Node,
        prefix: "reference/".into(),
    };
    command.command = Command::UpdateKnowledgeCollection {
        id: conversation.id,
        expected_revision: 1,
        definition,
    };
    create(&db, &command).await;
    let stored = catalog(&db)
        .await
        .unwrap()
        .collections
        .into_iter()
        .find(|stored| stored.collection.id == conversation.id)
        .unwrap();
    assert_eq!(stored.capture.worktree, None);
    assert_eq!(stored.capture.session, None);
}

#[tokio::test]
async fn rejects_absolute_paths_for_plugins() {
    let db = database().await;
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().canonicalize().unwrap();
    let file = root.join("reference.txt");
    std::fs::write(&file, "Original").unwrap();
    let owner = package(&db, "owner", 'a', &[]).await;
    for source in [
        Source::Directory {
            path: root.to_str().unwrap().into(),
        },
        Source::File {
            path: file.to_str().unwrap().into(),
        },
    ] {
        assert_eq!(
            execute(
                &db,
                &captured(
                    &owner,
                    Command::CreateKnowledgeCollection {
                        definition: definition(source.clone()),
                        plugin: None
                    }
                )
            )
            .await
            .unwrap_err()
            .code,
            ErrorCode::PermissionDenied
        );
        let collection = create(
            &db,
            &request(Command::CreateKnowledgeCollection {
                definition: definition(source),
                plugin: Some("owner".into()),
            }),
        )
        .await;
        assert!(
            list(&db, &captured(&owner, Command::ListKnowledgeCollections))
                .await
                .unwrap()
                .iter()
                .any(|entry| entry.id == collection.id)
        );
        let (output, _) = execute(
            &db,
            &captured(
                &owner,
                Command::DeleteKnowledgeCollection {
                    id: collection.id,
                    expected_revision: 1,
                },
            ),
        )
        .await
        .unwrap();
        assert!(matches!(output, Output::KnowledgeCollectionDeleted { id } if id == collection.id));
        assert_eq!(std::fs::read_to_string(&file).unwrap(), "Original");
    }
}

#[cfg(unix)]
#[tokio::test]
async fn rejects_links_and_parent_traversal() {
    let db = database().await;
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().canonicalize().unwrap();
    let external = tempfile::tempdir().unwrap();
    let private = external.path().join("private.txt");
    std::fs::write(&private, "Private").unwrap();
    std::os::unix::fs::symlink(&private, root.join("file-link")).unwrap();
    std::os::unix::fs::symlink(external.path(), root.join("directory-link")).unwrap();
    let tree = worktree(&db, &root).await;
    let owner = package(&db, "owner", 'a', &["files.read"]).await;
    for path in [
        "file-link",
        "directory-link",
        "directory-link/private.txt",
        "../private.txt",
    ] {
        let mut command = captured(
            &owner,
            Command::CreateKnowledgeCollection {
                definition: definition(Source::Worktree {
                    worktree: tree,
                    path: path.into(),
                }),
                plugin: None,
            },
        );
        command.plugin.as_mut().unwrap().worktree = Some(tree);
        assert!(execute(&db, &command).await.is_err(), "accepted {path}");
    }
    for source in [
        Source::File {
            path: root.join("file-link").to_str().unwrap().into(),
        },
        Source::Directory {
            path: root.join("directory-link").to_str().unwrap().into(),
        },
        Source::File {
            path: root
                .join("directory-link/private.txt")
                .to_str()
                .unwrap()
                .into(),
        },
    ] {
        assert!(
            execute(
                &db,
                &request(Command::CreateKnowledgeCollection {
                    definition: definition(source),
                    plugin: None
                })
            )
            .await
            .is_err()
        );
    }
    assert!(raw(&db).await.is_empty());
    assert_eq!(std::fs::read_to_string(private).unwrap(), "Private");
}
