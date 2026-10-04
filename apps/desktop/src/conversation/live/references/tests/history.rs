use super::*;
use sailry_protocol::{Session, Worktree};
use std::{cell::RefCell, rc::Rc};

fn repository(fixture: &Fixture) -> git2::Repository {
    let root = fixture.directory.path().join("project");
    let repository = git2::Repository::init(&root).unwrap();
    repository.set_head("refs/heads/main").unwrap();
    std::fs::write(root.join("notes.txt"), "Original worktree\n").unwrap();
    let mut index = repository.index().unwrap();
    index.add_path(std::path::Path::new("notes.txt")).unwrap();
    index.write().unwrap();
    let tree = index.write_tree().unwrap();
    let signature = git2::Signature::now("Fixture", "fixture@example.test").unwrap();
    repository
        .commit(
            Some("HEAD"),
            &signature,
            &signature,
            "Initial fixture",
            &repository.find_tree(tree).unwrap(),
            &[],
        )
        .unwrap();
    repository
}

fn move_session(fixture: &Fixture, repository: &git2::Repository) -> (Session, Worktree) {
    let Output::Worktree(tree) = fixture.execute(Command::CreateWorktree {
        project: fixture.session.project.unwrap(),
        path: fixture
            .directory
            .path()
            .join("next")
            .to_str()
            .unwrap()
            .into(),
        branch: "next".into(),
        commit: repository
            .head()
            .unwrap()
            .peel_to_commit()
            .unwrap()
            .id()
            .to_string(),
    }) else {
        panic!("worktree expected")
    };
    std::fs::write(
        std::path::Path::new(&tree.path).join("notes.txt"),
        "Current worktree\n",
    )
    .unwrap();
    let Output::Session(current) = fixture.execute(Command::ReadSession {
        session: fixture.session.id,
    }) else {
        panic!("session expected")
    };
    let Output::Session(moved) = fixture.execute(Command::MoveConversation {
        session: current.id,
        worktree: tree.id,
        expected_revision: current.revision,
    }) else {
        panic!("moved session expected")
    };
    (moved, tree)
}

fn submit(fixture: &Fixture, text: &str) {
    let Output::Session(session) = fixture.execute(Command::ReadSession {
        session: fixture.session.id,
    }) else {
        panic!("session expected")
    };
    fixture.execute(Command::SubmitTurn {
        session: session.id,
        expected_revision: session.revision,
        message: sailry_protocol::conversation::Input {
            text: text.into(),
            references: vec![Reference {
                target: Target::File("notes.txt".into()),
                label: "Notes".into(),
            }],
            ..Default::default()
        },
    });
    mounted::finished(fixture);
}

#[gpui::test]
fn sent_paths_keep_the_original_worktree(cx: &mut TestAppContext) {
    init(cx);
    for remote in [false, true] {
        let fixture = Fixture::with_tools(remote, vec![]);
        let repository = repository(&fixture);
        submit(&fixture, "@Notes");
        submit(&fixture, "");
        let history = mounted::finished(&fixture);
        let (moved, tree) = move_session(&fixture, &repository);
        let mut binding = fixture.binding.clone();
        binding.worktree = Some(tree.id);
        let (view, visual) = fixture::open(cx, binding, moved);
        wait(visual, |cx| {
            view.read(cx)
                .history
                .snapshot
                .as_ref()
                .is_some_and(|snapshot| snapshot.page.runs == history.page.runs)
        });
        let routes = Rc::new(RefCell::new(Vec::new()));
        visual.update(|_, cx| {
            let routes = routes.clone();
            cx.subscribe(&view, move |_, event: &Event, _| match event {
                Event::FileAt(tree, path) | Event::DirectoryAt(tree, path) => {
                    routes.borrow_mut().push((*tree, path.clone()))
                }
                Event::File(_) => panic!("sent reference lost its turn scope"),
                _ => {}
            })
            .detach();
        });
        let users = history
            .page
            .entries
            .iter()
            .filter(|entry| entry.author == "user")
            .collect::<Vec<_>>();
        assert_eq!(users.len(), 2);
        tap(visual, &format!("live-user-text-{}", users[0].turn));
        tap(visual, &format!("sent-reference-{}-0", users[1].turn));
        visual.update(|window, cx| {
            view.update(cx, |view, cx| {
                view.open_reference(
                    Reference {
                        target: Target::Directory("src".into()),
                        label: "Source".into(),
                    },
                    Some(users[0].turn),
                    window,
                    cx,
                );
            })
        });
        assert_eq!(
            *routes.borrow(),
            vec![
                (fixture.session.worktree, "notes.txt".into()),
                (fixture.session.worktree, "notes.txt".into()),
                (fixture.session.worktree, "src".into())
            ]
        );
        visual.update(|window, cx| {
            view.update(cx, |view, cx| {
                view.open_reference(
                    Reference {
                        target: Target::File("notes.txt".into()),
                        label: "Notes".into(),
                    },
                    Some(TurnId::new()),
                    window,
                    cx,
                );
            })
        });
        assert_eq!(
            routes.borrow().len(),
            3,
            "unknown turns must not fall back to the current worktree"
        );
        visual.update(|window, cx| {
            assert_eq!(
                crate::feedback::tests::summary(window, cx),
                tr("reference_unavailable")
            )
        });
        visual.update(|window, cx| {
            view.update(cx, |view, cx| {
                view.show_outgoing("@Notes".into(), cx);
                let row = view.outgoing.as_ref().unwrap().row;
                view.open_reference(
                    Reference {
                        target: Target::File("notes.txt".into()),
                        label: "Notes".into(),
                    },
                    Some(row),
                    window,
                    cx,
                );
            })
        });
        assert_eq!(routes.borrow().last(), Some(&(tree.id, "notes.txt".into())));
        assert_eq!(fixture.task_requests(), 2);
        visual.update(|window, _| window.remove_window());
        fixture.close();
    }
}

#[gpui::test]
fn opens_original_documents_after_a_move(cx: &mut TestAppContext) {
    init(cx);
    for remote in [false, true] {
        let fixture = Fixture::with_tools(false, vec![]);
        let desktop = mounted::desktop(&fixture);
        let repository = repository(&fixture);
        fixture.files_context();
        submit(&fixture, "@Notes");
        let history = mounted::finished(&fixture);
        let (moved, tree) = move_session(&fixture, &repository);
        let (shell, view, visual) = mounted::open(&fixture, &desktop, remote, moved, cx);
        let turn = history
            .page
            .entries
            .iter()
            .find(|entry| entry.author == "user")
            .unwrap()
            .turn;
        wait(visual, |cx| view.read(cx).rows.contains(&turn));
        visual.update(|_, cx| {
            shell.update(cx, |shell, cx| {
                shell.live.as_mut().unwrap().select(desktop.id(), cx)
            })
        });
        tap(visual, &format!("live-user-text-{turn}"));
        wait(visual, |cx| {
            let Some(crate::resources::SideResource::Plugin(panel)) = &shell.read(cx).side_resource
            else {
                return false;
            };
            panel.read(cx).documents.as_ref().is_some_and(|documents| {
                let documents = documents.read(cx);
                documents.scope() == (fixture.node.id(), fixture.session.worktree)
                    && documents.snapshot(cx)["documents"]
                        .as_array()
                        .is_some_and(|items| {
                            items.iter().any(|item| {
                                item["path"] == "notes.txt"
                                    && item["revision"]
                                        == blake3::hash(b"Original worktree\n").to_hex().to_string()
                            })
                        })
            })
        });
        visual.update(|_, cx| {
            let Some(crate::resources::SideResource::Plugin(panel)) = &shell.read(cx).side_resource
            else {
                panic!("document panel expected")
            };
            assert_eq!(
                panel
                    .read(cx)
                    .documents
                    .as_ref()
                    .unwrap()
                    .read(cx)
                    .session(),
                None
            );
            assert_eq!(view.read(cx).binding.worktree, Some(tree.id));
        });
        assert_eq!(fixture.task_requests(), 1);
        visual.update(|window, _| window.remove_window());
        fixture.runtime.block_on(desktop.shutdown()).unwrap();
        fixture.close();
    }
}
