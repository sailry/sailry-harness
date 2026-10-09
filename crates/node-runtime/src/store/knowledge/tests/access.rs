use super::*;

#[test]
fn admitted_operations_retain_package_after_removal() {
    let db = database();
    let owner = package(&db, "owner", 'a', &["storage.read"]);
    let original = insert(&db, &owner, storage());
    package(&db, "owner", 'b', &[]);
    db.execute("UPDATE plugins SET installed=0 WHERE name='owner'", [])
        .unwrap();
    let ordinary = captured(&owner, Command::ListKnowledgeCollections);
    assert_eq!(list(&db, &ordinary).unwrap_err().code, ErrorCode::NotFound);
    // Turn and invocation authority is checked by the shared admission boundary.
    // This SQLite fixture exercises the handlers' immutable package resolution.
    for callback in [false, true] {
        let mut admitted = captured(&owner, Command::ListKnowledgeCollections);
        let context = admitted.plugin.as_mut().unwrap();
        if callback {
            context.invocation = Some(sailry_protocol::RequestId::new());
        } else {
            context.turn = Some(sailry_protocol::TurnId::new());
        }
        assert_eq!(list(&db, &admitted).unwrap().len(), 1);
        admitted.command = Command::RebuildKnowledgeCollection {
            id: original.id,
            expected_revision: if callback { 2 } else { 1 },
        };
        let rebuilt = create(&db, &admitted);
        assert_eq!(rebuilt.revision, if callback { 3 } else { 2 });
        assert_eq!(
            catalog(&db).unwrap().collections[0].capture.package,
            Some(owner.clone())
        );
    }
}

#[test]
fn captures_owner_without_caller_override() {
    let db = database();
    let owner = package(&db, "owner", 'a', &["storage.read"]);
    package(&db, "other", 'b', &["storage.read"]);
    for selected in [Some("other".into()), Some("owner".into())] {
        let denied = captured(
            &owner,
            Command::CreateKnowledgeCollection {
                definition: storage(),
                plugin: selected,
            },
        );
        assert_eq!(
            execute(&db, &denied).unwrap_err().code,
            ErrorCode::PermissionDenied
        );
    }
    assert!(raw(&db).is_empty());
    let collection = insert(&db, &owner, storage());
    assert_eq!(
        collection.owner,
        Owner::Plugin {
            name: "owner".into()
        }
    );
    let stored = catalog(&db).unwrap().collections.remove(0);
    assert_eq!(stored.capture.package, Some(owner));
    assert_eq!(stored.capture.worktree, None);
    assert_eq!(stored.capture.session, None);
    let absent = request(Command::CreateKnowledgeCollection {
        definition: storage(),
        plugin: Some("missing".into()),
    });
    assert_eq!(execute(&db, &absent).unwrap_err().code, ErrorCode::NotFound);
    let unowned = request(Command::CreateKnowledgeCollection {
        definition: storage(),
        plugin: None,
    });
    assert_eq!(
        execute(&db, &unowned).unwrap_err().code,
        ErrorCode::PermissionDenied
    );
    assert_eq!(raw(&db).len(), 1);
}

#[test]
fn sharing_requires_controller_and_current_package() {
    let db = database();
    let owner = package(&db, "owner", 'a', &["storage.read"]);
    let reader = package(&db, "reader", 'b', &[]);
    let outsider = package(&db, "outsider", 'c', &["storage.read"]);
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
    );
    assert_eq!(
        allowed(&db, &captured(&reader, Command::ListKnowledgeCollections))
            .unwrap()
            .len(),
        1
    );
    assert!(
        allowed(&db, &captured(&outsider, Command::ListKnowledgeCollections))
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
        execute(&db, &mutation).unwrap_err().code,
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
        execute(&db, &update).unwrap_err().code,
        ErrorCode::PermissionDenied
    );
    let update = request(Command::UpdateKnowledgeCollection {
        id: collection.id,
        expected_revision: 1,
        definition,
    });
    create(&db, &update);
    assert!(
        allowed(&db, &captured(&reader, Command::ListKnowledgeCollections))
            .unwrap()
            .is_empty()
    );
    let replacement = package(&db, "owner", 'd', &["storage.read"]);
    assert_eq!(
        list(&db, &captured(&owner, Command::ListKnowledgeCollections))
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
    );
    assert_eq!(rebuilt.revision, 3);
    assert_eq!(
        catalog(&db).unwrap().collections[0].capture.package,
        Some(replacement.clone())
    );
    package(&db, "owner", 'e', &[]);
    let stale = captured(
        &replacement,
        Command::RebuildKnowledgeCollection {
            id: collection.id,
            expected_revision: 3,
        },
    );
    assert_eq!(
        execute(&db, &stale).unwrap_err().code,
        ErrorCode::RevisionConflict
    );
    let current = package(&db, "owner", 'f', &[]);
    assert!(
        list(&db, &captured(&current, Command::ListKnowledgeCollections))
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
        .unwrap_err()
        .code,
        ErrorCode::PermissionDenied
    );
}

#[test]
fn confines_worktree_and_conversation_sources() {
    let db = database();
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().canonicalize().unwrap();
    std::fs::write(root.join("guide.txt"), "Reference").unwrap();
    let tree = worktree(&db, &root);
    let owner = package(
        &db,
        "owner",
        'a',
        &["files.read", "storage.read", "conversation.read"],
    );
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
        execute(&db, &command).unwrap_err().code,
        ErrorCode::PermissionDenied
    );
    command.plugin.as_mut().unwrap().worktree = Some(tree);
    let collection = create(&db, &command);
    assert_eq!(
        catalog(&db).unwrap().collections[0].capture.worktree,
        Some(tree)
    );
    assert!(
        list(&db, &captured(&owner, Command::ListKnowledgeCollections))
            .unwrap()
            .is_empty()
    );
    let mut read = captured(&owner, Command::ListKnowledgeCollections);
    read.plugin.as_mut().unwrap().worktree = Some(tree);
    assert_eq!(list(&db, &read).unwrap(), [collection]);
    let mut definition = definition(Source::Storage {
        scope: Scope::Conversation,
        prefix: "reference/".into(),
    });
    command.command = Command::CreateKnowledgeCollection {
        definition: definition.clone(),
        plugin: None,
    };
    assert_eq!(
        execute(&db, &command).unwrap_err().code,
        ErrorCode::PermissionDenied
    );
    let session = SessionId::new();
    db.execute(
        "INSERT INTO sessions(id,worktree,revision) VALUES(?1,?2,1)",
        params![session.to_string(), tree.to_string()],
    )
    .unwrap();
    command.plugin.as_mut().unwrap().session = Some(session);
    let conversation = create(&db, &command);
    assert_eq!(list(&db, &read).unwrap().len(), 1);
    read.plugin.as_mut().unwrap().session = Some(session);
    assert_eq!(list(&db, &read).unwrap().len(), 2);
    command.plugin.as_mut().unwrap().worktree = Some(WorktreeId::new());
    assert_eq!(
        execute(&db, &command).unwrap_err().code,
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
    create(&db, &command);
    let stored = catalog(&db)
        .unwrap()
        .collections
        .into_iter()
        .find(|stored| stored.collection.id == conversation.id)
        .unwrap();
    assert_eq!(stored.capture.worktree, None);
    assert_eq!(stored.capture.session, None);
}

#[test]
fn rejects_absolute_paths_for_plugins() {
    let db = database();
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().canonicalize().unwrap();
    let file = root.join("reference.txt");
    std::fs::write(&file, "Original").unwrap();
    let owner = package(&db, "owner", 'a', &[]);
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
        );
        assert!(
            list(&db, &captured(&owner, Command::ListKnowledgeCollections))
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
        .unwrap();
        assert!(matches!(output, Output::KnowledgeCollectionDeleted { id } if id == collection.id));
        assert_eq!(std::fs::read_to_string(&file).unwrap(), "Original");
    }
}

#[cfg(unix)]
#[test]
fn rejects_links_and_parent_traversal() {
    let db = database();
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().canonicalize().unwrap();
    let external = tempfile::tempdir().unwrap();
    let private = external.path().join("private.txt");
    std::fs::write(&private, "Private").unwrap();
    std::os::unix::fs::symlink(&private, root.join("file-link")).unwrap();
    std::os::unix::fs::symlink(external.path(), root.join("directory-link")).unwrap();
    let tree = worktree(&db, &root);
    let owner = package(&db, "owner", 'a', &["files.read"]);
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
        assert!(execute(&db, &command).is_err(), "accepted {path}");
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
            .is_err()
        );
    }
    assert!(raw(&db).is_empty());
    assert_eq!(std::fs::read_to_string(private).unwrap(), "Private");
}
