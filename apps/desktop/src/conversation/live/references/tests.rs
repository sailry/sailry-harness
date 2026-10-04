mod history;
pub(super) mod inline;
mod mounted;
mod navigation;
mod scoped;
mod sessions;

use super::super::tests::{
    fixture::{self, Fixture, init, tap},
    wait,
};
use super::*;
use core::prelude::v1::test;

#[gpui::test]
fn command_menu(cx: &mut TestAppContext) {
    init(cx);
    let fixture = Fixture::with_tools(false, vec![]);
    let (view, visual) = fixture::open_session(cx, fixture.binding.clone(), None);
    wait(visual, |cx| view.read(cx).connected());
    tap(visual, "live-chat-input");
    visual.simulate_input("/");
    wait(visual, |cx| view.read(cx).references.open);
    visual.update(|window, cx| {
        let _ = window.draw(cx);
    });
    let popup = visual.debug_bounds("live-reference-picker").unwrap();
    let composer = visual.debug_bounds("composer-surface").unwrap();
    assert_eq!(popup.left(), composer.left());
    assert_eq!(popup.size.width, composer.size.width);
    assert!(popup.bottom() < composer.top());
    visual.simulate_input("review");
    visual.simulate_keystrokes("enter");
    assert_eq!(
        view.read_with(visual, |view, cx| view.input.read(cx).value()),
        tr("composer_prompt_review")
    );
    assert!(view.read_with(visual, |view, _| view.session.is_none()));
    assert!(fixture.server.requests.lock().unwrap().is_empty());
    visual.simulate_keystrokes("cmd-a");
    visual.simulate_input("/explain");
    tap(visual, "live-reference-row-0");
    assert_eq!(
        view.read_with(visual, |view, cx| view.input.read(cx).value()),
        tr("composer_prompt_explain")
    );
    visual.simulate_keystrokes("cmd-a");
    visual.simulate_input("/model");
    visual.simulate_keystrokes("enter");
    wait(visual, |cx| view.read(cx).references.page == Page::Models);
    visual.simulate_keystrokes("enter");
    wait(visual, |cx| view.read(cx).config.is_some());
    assert!(
        view.read_with(visual, |view, cx| view.input.read(cx).value().is_empty()
            && view.session.is_none())
    );
    visual.simulate_input("/");
    visual.simulate_keystrokes("backspace");
    assert!(view.read_with(visual, |view, cx| view.input.read(cx).value().is_empty()));
    visual.simulate_input("/tmp/file");
    assert!(!view.read_with(visual, |view, _| view.references.open));
    visual.simulate_keystrokes("cmd-a");
    visual.simulate_input("@");
    wait(visual, |cx| view.read(cx).references.open);
    visual.update(|window, cx| {
        let _ = window.draw(cx);
    });
    let popup = visual.debug_bounds("live-reference-picker").unwrap();
    let composer = visual.debug_bounds("composer-surface").unwrap();
    assert_eq!(popup.left(), composer.left());
    assert_eq!(popup.size.width, composer.size.width);
    assert!(popup.bottom() < composer.top());
    visual.simulate_keystrokes("escape");
    assert_eq!(
        view.read_with(visual, |view, cx| view.input.read(cx).value()),
        "@"
    );
    visual.update(|window, _| window.remove_window());
    drop(view);
    fixture.close();
}

#[gpui::test]
fn dispatches_frozen_roles(cx: &mut TestAppContext) {
    init(cx);
    for remote in [false, true] {
        let fixture = Fixture::with_tools(remote, vec![]);
        let root = fixture.directory.path().join("project");
        for index in 0..=500 {
            std::fs::write(root.join(format!("file{index:03}")), "file contents").unwrap();
        }
        let role = sailry_protocol::role::Profile {
            appearance: None,
            id: sailry_protocol::RoleId::new(),
            revision: 0,
            key: "reviewer".into(),
            name: "Reviewer".into(),
            description: "Review code".into(),
            model: None,
            max_turns: None,
            skills: vec![],
            instructions: "Original instructions".into(),
        };
        let Output::Role(mut role) = fixture.execute(Command::PutRole {
            role,
            expected_revision: 0,
        }) else {
            panic!("role expected");
        };
        let Output::Session(session) = fixture.execute(Command::SetSessionRoles {
            session: fixture.session.id,
            expected_revision: fixture.session.revision,
            roles: vec![role.reference()],
        }) else {
            panic!("session expected");
        };
        role.instructions = "Changed catalog instructions".into();
        fixture.execute(Command::PutRole {
            expected_revision: role.revision,
            role,
        });
        let (view, visual) = fixture::open(cx, fixture.binding.clone(), session);
        wait(visual, |cx| view.read(cx).connected());
        view.read_with(visual, |view, cx| {
            let Item::Reference(reference) = &view.reference_catalog(&Page::Agents, cx)[0] else {
                panic!("reference expected");
            };
            let Target::Agent(role) = &reference.target else {
                panic!("role expected");
            };
            assert_eq!(
                view.reference_roles()
                    .iter()
                    .find(|profile| profile.reference() == *role)
                    .unwrap()
                    .instructions,
                "Original instructions"
            );
        });
        tap(visual, "live-chat-input");
        visual.simulate_input("@files");
        wait(visual, |cx| view.read(cx).references.open);
        visual.simulate_keystrokes("enter");
        wait(visual, |cx| {
            !view.read(cx).references.loading && view.read(cx).references.next.is_some()
        });
        visual.simulate_input("file500");
        wait(visual, |cx| {
            view.read(cx)
                .references
                .list
                .read(cx)
                .delegate()
                .rows
                .is_empty()
        });
        tap(visual, "live-reference-more");
        wait(visual, |cx| {
            !view.read(cx).references.loading && view.read(cx).references.next.is_none()
        });
        visual.update(|window, cx| {
            view.update(cx, |view, cx| {
                let rows = &view.references.list.read(cx).delegate().rows;
                assert_eq!(rows.len(), 1);
                view.choose_reference(view.references.generation, rows[0].clone(), window, cx);
            })
        });
        assert!(view.read_with(visual, |view, _| matches!(&view.references.selected[0].target, Target::File(path) if path == "file500")));
        tap(visual, "live-chat-input");
        visual.simulate_input("Inspect @agents");
        wait(visual, |cx| view.read(cx).references.open);
        visual.simulate_keystrokes("enter enter");
        assert!(view.read_with(visual, |view, _| {
            view.references
                .selected
                .iter()
                .any(|reference| matches!(reference.target, Target::Agent(_)))
        }));
        tap(visual, "live-chat-send");
        wait(visual, |cx| {
            view.read(cx)
                .history
                .snapshot
                .as_ref()
                .is_some_and(|snapshot| {
                    snapshot
                        .page
                        .runs
                        .iter()
                        .any(|run| run.status == Status::Completed)
                })
        });
        view.read_with(visual, |view, _| {
            let children = &view.history.snapshot.as_ref().unwrap().page.children;
            assert_eq!(children.len(), 1);
            assert_eq!(children[0].run.status, Status::Completed);
        });
        let requests = fixture.server.requests.lock().unwrap().clone();
        assert_eq!(requests.len(), 2);
        assert!(requests[0].to_string().contains("Original instructions"));
        assert!(
            !requests[0]
                .to_string()
                .contains("Changed catalog instructions")
        );
        visual.update(|window, _| window.remove_window());
        drop(view);
        fixture.close();
    }
}

#[gpui::test]
fn selects_paths_and_sends_context(cx: &mut TestAppContext) {
    init(cx);
    for remote in [false, true] {
        let fixture = Fixture::with_tools(remote, vec![]);
        let directory = fixture.directory.path().join("project/src");
        std::fs::create_dir(&directory).unwrap();
        std::fs::write(directory.join("资料.rs"), "workspace contents").unwrap();
        let (view, visual) = fixture::open(cx, fixture.binding.clone(), fixture.session.clone());
        wait(visual, |cx| view.read(cx).connected());
        tap(visual, "live-chat-input");
        visual.simulate_input("Inspect @");
        wait(visual, |cx| view.read(cx).references.open);
        visual.simulate_keystrokes("enter");
        wait(visual, |cx| !view.read(cx).references.loading);
        visual.simulate_keystrokes("enter");
        wait(visual, |cx| {
            matches!(&view.read(cx).references.page, Page::Files(path) if path == "src")
                && !view.read(cx).references.loading
        });
        visual.simulate_keystrokes("down enter");
        assert_eq!(
            view.read_with(visual, |view, cx| view.input.read(cx).value()),
            "Inspect @src/资料.rs "
        );
        assert!(view.read_with(visual, |view, _| matches!(&view.references.selected[0].target, Target::File(path) if path == "src/资料.rs")));
        let opened = Arc::new(std::sync::Mutex::new(None));
        let output = opened.clone();
        let _events = visual.update(|_, cx| {
            cx.subscribe(&view, move |_, event, _| {
                if let Event::File(path) | Event::FileAt(_, path) = event {
                    *output.lock().unwrap() = Some(path.clone());
                }
            })
        });
        inline::click_token(visual, &view, "@src/资料.rs");
        assert_eq!(opened.lock().unwrap().as_deref(), Some("src/资料.rs"));
        tap(visual, "live-chat-send");
        wait(visual, |cx| {
            !view.read(cx).pending && view.read(cx).references.selected.is_empty()
        });
        assert_eq!(
            view.read_with(visual, |view, cx| view.input.read(cx).value()),
            ""
        );
        wait(visual, |cx| {
            view.read(cx)
                .history
                .snapshot
                .as_ref()
                .is_some_and(|snapshot| {
                    snapshot
                        .page
                        .runs
                        .iter()
                        .any(|run| run.status == Status::Completed)
                })
        });
        let Output::Conversation(history) = fixture.execute(Command::ReadConversation {
            session: fixture.session.id,
            before: None,
            limit: 50,
        }) else {
            panic!("conversation expected");
        };
        assert!(history.page.entries.iter().any(|entry| {
            entry.author == "user" && entry.parts.iter().any(|part|
            matches!(part, sailry_protocol::conversation::Part::Text(text) if text == "Inspect @src/资料.rs "))
        }));
        let user = history
            .page
            .entries
            .iter()
            .find(|entry| entry.author == "user")
            .unwrap();
        assert!(user.parts.iter().any(|part| matches!(part, sailry_protocol::conversation::Part::Reference(reference) if reference.target == Target::File("src/资料.rs".into()))));
        let selector: &'static str =
            Box::leak(format!("live-user-text-{}", user.turn).into_boxed_str());
        *opened.lock().unwrap() = None;
        tap(visual, selector);
        assert_eq!(opened.lock().unwrap().as_deref(), Some("src/资料.rs"));
        *opened.lock().unwrap() = None;
        let bounds = visual.debug_bounds(selector).unwrap();
        let from = point(bounds.left() + px(1.), bounds.center().y);
        let to = point(bounds.right() - px(1.), bounds.center().y);
        visual.simulate_mouse_down(from, MouseButton::Left, Modifiers::default());
        visual.simulate_mouse_move(to, Some(MouseButton::Left), Modifiers::default());
        visual.simulate_mouse_up(to, MouseButton::Left, Modifiers::default());
        visual.run_until_parked();
        assert!(
            opened.lock().unwrap().is_none(),
            "dragging must select text without opening a reference"
        );
        assert_eq!(
            visual
                .update(gpui_kit::base::TextSelection::selected_text)
                .trim_end(),
            "Inspect @src/资料.rs"
        );
        assert_eq!(fixture.server.requests.lock().unwrap().len(), 1);
        visual.update(|window, _| window.remove_window());
        drop(view);
        let (view, visual) = fixture::open(cx, fixture.binding.clone(), fixture.session.clone());
        wait(visual, |cx| {
            view.read(cx)
                .history
                .snapshot
                .as_ref()
                .is_some_and(|snapshot| snapshot.page.entries == history.page.entries)
        });
        let output = opened.clone();
        let expected_worktree = fixture.session.worktree;
        let _restored_events = visual.update(|_, cx| {
            cx.subscribe(&view, move |_, event, _| {
                if let Event::FileAt(worktree, path) = event {
                    assert_eq!(*worktree, expected_worktree);
                    *output.lock().unwrap() = Some(path.clone());
                }
            })
        });
        *opened.lock().unwrap() = None;
        tap(visual, selector);
        assert_eq!(opened.lock().unwrap().as_deref(), Some("src/资料.rs"));
        assert_eq!(fixture.server.requests.lock().unwrap().len(), 1);
        visual.update(|window, _| window.remove_window());
        drop(view);
        fixture.close();
    }
}

#[gpui::test]
fn rejects_stale_roles(cx: &mut TestAppContext) {
    init(cx);
    let fixture = Fixture::with_tools(false, vec![]);
    let (view, visual) = fixture::open(cx, fixture.binding.clone(), fixture.session.clone());
    wait(visual, |cx| view.read(cx).connected());
    tap(visual, "live-chat-input");
    visual.simulate_input("mail@example.org");
    assert!(!view.read_with(visual, |view, _| view.references.open));
    visual.simulate_input(" @");
    wait(visual, |cx| view.read(cx).references.open);
    visual.simulate_keystrokes("escape");
    assert!(!view.read_with(visual, |view, _| view.references.open));
    visual.simulate_keystrokes("backspace");
    visual.simulate_input("@");
    wait(visual, |cx| view.read(cx).references.open);
    visual.update(|window, cx| {
        view.update(cx, |view, cx| {
            let generation = view.references.generation;
            view.choose_reference(generation, Item::Page(Page::Context), window, cx);
        })
    });
    visual.simulate_keystrokes("enter");
    assert_eq!(
        view.read_with(visual, |view, _| view.references.selected.len()),
        1
    );
    visual.simulate_keystrokes("cmd-a backspace");
    assert!(view.read_with(visual, |view, cx| view.active_references(cx).is_empty()));
    visual.update(|window, cx| {
        view.update(cx, |view, cx| {
            let role = sailry_protocol::role::Profile {
                appearance: None,
                id: sailry_protocol::RoleId::new(),
                revision: 1,
                key: "reviewer".into(),
                name: "Reviewer".into(),
                description: String::new(),
                model: None,
                max_turns: None,
                skills: vec![],
                instructions: "Review".into(),
            };
            let token = view.remember_reference(Reference {
                target: Target::Agent(role.reference()),
                label: "@reviewer".into(),
            });
            view.input.update(cx, |input, cx| {
                input.set_value("", window, cx);
                input.replace_with_token(token, window, cx).unwrap();
            });
            view.send(window, cx);
        })
    });
    assert!(
        view.read_with(visual, |view, cx| view.error == Some("reference_stale")
            && !view.input.read(cx).value().is_empty()
            && view.active_references(cx).len() == 1)
    );
    assert!(fixture.server.requests.lock().unwrap().is_empty());
    visual.update(|window, _| window.remove_window());
    drop(view);
    fixture.close();
}

#[gpui::test]
fn shared_connection_mentions(cx: &mut TestAppContext) {
    use sailry_protocol::{connection::Sharing, database, ssh};
    init(cx);
    for remote in [false, true] {
        let fixture = Fixture::with_tools(remote, vec![]);
        let path = fixture.directory.path().join("data.sqlite3");
        rusqlite::Connection::open(&path)
            .unwrap()
            .execute_batch("CREATE TABLE items(id INTEGER)")
            .unwrap();
        let mut database_ids = Vec::new();
        for sharing in [
            None,
            Some(Sharing::Global),
            Some(Sharing::Projects(vec![fixture.session.project.unwrap()])),
        ] {
            let Output::DatabaseProfile(profile) = fixture.execute(Command::SaveDatabase {
                profile: database::Profile {
                    id: sailry_protocol::DatabaseId::new(),
                    revision: 0,
                    name: "Database fixture".into(),
                    connection: database::Connection::Sqlite {
                        path: path.to_str().unwrap().into(),
                    },
                    read_only: true,
                    sharing: sharing.clone(),
                },
                expected_revision: 0,
                password: None,
            }) else {
                panic!("database profile expected")
            };
            database_ids.push(profile.id);
            fixture.execute(Command::SaveSsh {
                profile: ssh::Profile {
                    id: sailry_protocol::SshId::new(),
                    revision: 0,
                    name: "SSH fixture".into(),
                    host: "localhost".into(),
                    port: 22,
                    username: "fixture".into(),
                    authentication: ssh::Authentication::Agent,
                    host_key: None,
                    sharing,
                },
                expected_revision: 0,
                credential: Some(ssh::Credential::Agent),
            });
        }
        let (view, visual) = fixture::open(cx, fixture.binding.clone(), fixture.session.clone());
        wait(visual, |cx| {
            view.read(cx)
                .node
                .snapshot
                .as_ref()
                .is_some_and(|snapshot| snapshot.ssh.len() == 3)
        });
        wait(visual, |cx| view.read(cx).connected());
        view.read_with(visual, |view, _| {
            assert_eq!(view.reference_databases().len(), 2);
            assert_eq!(view.reference_ssh().len(), 2);
            assert!(
                !view
                    .reference_databases()
                    .iter()
                    .any(|profile| profile.id == database_ids[0])
            );
        });
        tap(visual, "live-chat-input");
        visual.simulate_input("@");
        visual.update(|window, cx| {
            view.update(cx, |view, cx| {
                view.reference_page(
                    Page::Database(database_ids[1], "Database fixture".into(), None),
                    window,
                    cx,
                )
            })
        });
        wait(visual, |cx| !view.read(cx).references.loading);
        visual.update(|window, cx| {
            view.update(cx, |view, cx| {
                view.reference_page(
                    Page::Database(
                        database_ids[1],
                        "Database fixture".into(),
                        Some("main".into()),
                    ),
                    window,
                    cx,
                )
            })
        });
        wait(visual, |cx| !view.read(cx).references.loading);
        tap(visual, "live-reference-row-1");
        view.read_with(visual,|view,_| {
            assert!(view.references.selected.iter().any(|reference|matches!(&reference.target, Target::Database {connection,table:Some(table),..} if *connection==database_ids[1] && table.name=="items")));
        });
        visual.simulate_input("@");
        visual.update(|window, cx| {
            view.update(cx, |view, cx| view.reference_page(Page::Ssh, window, cx))
        });
        tap(visual, "live-reference-row-0");
        view.read_with(visual, |view, _| {
            assert!(
                view.references
                    .selected
                    .iter()
                    .any(|reference| matches!(reference.target, Target::Ssh(_)))
            )
        });
        let selected = view.read_with(visual, |view, _| view.references.selected.clone());
        for reference in selected {
            inline::click_token(visual, &view, &super::inline::marker(&reference));
            visual.update(|window, cx| {
                window.draw(cx).clear(cx);
                assert!(window.has_active_dialog(cx));
                assert!(window.notifications(cx).is_empty());
            });
            assert!(visual.debug_bounds("details-content").is_some());
            visual.update(|window, cx| window.close_dialog(cx));
        }
        assert!(fixture.server.requests.lock().unwrap().is_empty());
        visual.update(|window, _| window.remove_window());
        drop(view);
        fixture.close();
    }
}

#[gpui::test]
fn typed_attachment_mentions(cx: &mut TestAppContext) {
    init(cx);
    for sidebar in [false, true] {
        let fixture = Fixture::with_tools(false, vec![]);
        let path = fixture.directory.path().join("notes.txt");
        std::fs::write(&path, "Database notes").unwrap();
        let (view, visual) = fixture::open(cx, fixture.binding.clone(), fixture.session.clone());
        visual.update(|_, cx| {
            view.update(cx, |view, cx| {
                view.sidebar = sidebar;
                cx.notify();
            })
        });
        wait(visual, |cx| view.read(cx).connected());
        tap(visual, "live-chat-input");
        visual.simulate_input("Inspect @attachment");
        wait(visual, |cx| view.read(cx).references.open);
        assert!(visual.debug_bounds("live-reference").is_none());
        view.read_with(visual, |view, cx| {
            let rows = &view.references.list.read(cx).delegate().rows;
            assert!(
                matches!(rows.as_slice(), [Item::Attachment]),
                "attachment suggestions: {:?}",
                rows.iter().map(Item::label).collect::<Vec<_>>()
            );
        });
        tap(visual, "live-reference-row-0");
        let state = view.read_with(visual, |view, cx| {
            let input = view.input.read(cx);
            (
                input.value(),
                input.selected_range(),
                view.references
                    .trigger
                    .as_ref()
                    .map(|trigger| trigger.range.clone()),
                view.references.open,
                view.references.generation,
                view.references.list.read(cx).delegate().generation,
                view.attachments_blocked(),
            )
        });
        assert!(
            visual.did_prompt_for_paths(),
            "attachment selection state: {state:?}"
        );
        visual.simulate_path_prompt_response(|options| {
            assert!(options.files && options.multiple && !options.directories);
            Some(vec![path.clone()])
        });
        wait(visual, |cx| view.read(cx).has_attachments());
        assert_eq!(
            view.read_with(visual, |view, cx| view.input.read(cx).value().to_string()),
            "Inspect "
        );
        assert!(!view.read_with(visual, |view, _| view.references.open));
        visual.update(|window, _| window.remove_window());
        drop(view);
        fixture.close();
    }
}

#[gpui::test]
fn preview_edit_preserves_draft_and_file_context(cx: &mut TestAppContext) {
    init(cx);
    for remote in [false, true] {
        let fixture = Fixture::with_tools(remote, vec![]);
        let (view, visual) = fixture::open_session(cx, fixture.binding.clone(), None);
        wait(visual, |cx| view.read(cx).connected());
        tap(visual, "live-chat-input");
        visual.simulate_input("Keep this draft");
        visual.update(|window, cx| {
            view.update(cx, |view, cx| {
                view.request_file_edit(
                    fixture.binding.worktree,
                    "report.docx".into(),
                    2,
                    "Selected paragraph",
                    window,
                    cx,
                );
            })
        });
        let draft = view.read_with(visual, |view, cx| view.draft(cx).to_string());
        assert!(draft.starts_with("Keep this draft"));
        assert!(draft.contains("report.docx") && draft.contains("Selected paragraph"));
        assert!(
            view.read_with(visual, |view, _| view.references.selected.iter().any(
                |reference| matches!(&reference.target, Target::File(path) if path == "report.docx")
            ))
        );
        assert!(fixture.server.requests.lock().unwrap().is_empty());
        visual.update(|window, cx| {
            view.update(cx, |view, cx| {
                view.request_file_edit(
                    Some(WorktreeId::new()),
                    "wrong.docx".into(),
                    1,
                    "Wrong node",
                    window,
                    cx,
                );
            })
        });
        assert_eq!(
            view.read_with(visual, |view, cx| view.draft(cx).to_string()),
            draft
        );
        visual.update(|window, _| window.remove_window());
        drop(view);
        fixture.close();
    }
}
