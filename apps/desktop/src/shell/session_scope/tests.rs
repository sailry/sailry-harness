use super::*;
use crate::{activity::fixture::Fixture, resources::SideResource};
use core::prelude::v1::test;
use gpui_kit::component::{Root, ThemeMode, WindowExt};
use sailry_client::Client;
use sailry_protocol::{Command, Output, Session, conversation::Status};
use std::time::{Duration, Instant};

mod active;
mod browser;
mod header_activity;
mod panels;
mod recent;
mod sidebar;
mod unassigned;

fn wait(cx: &mut VisualTestContext, ready: impl Fn(&App) -> bool) {
    let deadline = Instant::now() + Duration::from_secs(15);
    loop {
        cx.run_until_parked();
        if cx.update(|window, cx| {
            window.draw(cx).clear(cx);
            ready(cx)
        }) {
            return;
        }
        assert!(Instant::now() < deadline, "session tabs deadline");
        std::thread::sleep(Duration::from_millis(10));
    }
}

fn click(shell: &Entity<Shell>, cx: &mut VisualTestContext, selector: String) {
    let key = shell.read_with(cx, |shell, _| {
        shell.session_scope.open.iter().copied().find(|key| {
            key.selector() == selector || format!("close-{}", key.selector()) == selector
        })
    });
    if let Some(key) = key {
        cx.update(|window, cx| {
            shell.update(cx, |shell, cx| {
                if selector.starts_with("close-") {
                    shell.close_session_view(key, window, cx);
                } else {
                    shell.activate_session(key, window, cx);
                }
            })
        });
        cx.run_until_parked();
        return;
    }
    cx.update(|window, cx| window.draw(cx).clear(cx));
    let selector = Box::leak(selector.into_boxed_str());
    let bounds = cx
        .debug_bounds(selector)
        .unwrap_or_else(|| panic!("missing {selector}"));
    cx.simulate_click(bounds.center(), Modifiers::default());
    cx.run_until_parked();
}

fn mount<'a>(
    cx: &'a mut TestAppContext,
    fixture: &Fixture,
) -> (Entity<Shell>, &'a mut VisualTestContext) {
    mount_with_services(cx, fixture.services())
}

fn mount_with_services(
    cx: &mut TestAppContext,
    services: crate::backend::Services,
) -> (Entity<Shell>, &mut VisualTestContext) {
    cx.executor().allow_parking();
    cx.update(|cx| {
        gpui_kit::init(cx);
        crate::theme::init(cx);
        crate::shell::init(cx);
        cx.set_global(services);
    });
    let mut entity = None;
    let (_, visual) = cx.add_window_view(|window, cx| {
        let shell = cx.new(|cx| Shell::new(window, cx));
        entity = Some(shell.clone());
        Root::new(shell, window, cx)
    });
    let shell = entity.unwrap();
    wait(visual, |cx| {
        shell
            .read(cx)
            .live
            .as_ref()
            .is_some_and(|live| live.view.connected && live.hosts.len() == 2)
    });
    (shell, visual)
}

fn open(
    shell: &Entity<Shell>,
    cx: &mut VisualTestContext,
    fixture: &Fixture,
    index: usize,
    session: &Session,
) -> Key {
    let node = fixture.nodes[index].id();
    cx.update(|window, cx| {
        shell.update(cx, |shell, cx| {
            shell.park_session_panel(window, cx);
            if shell.live.as_ref().unwrap().selected != node {
                shell.live.as_mut().unwrap().select(node, cx);
            }
        })
    });
    wait(cx, |cx| {
        shell
            .read(cx)
            .live
            .as_ref()
            .unwrap()
            .view
            .snapshot
            .as_ref()
            .is_some_and(|snapshot| {
                snapshot.node == node && snapshot.sessions.iter().any(|s| s.id == session.id)
            })
    });
    cx.update(|window, cx| {
        shell.update(cx, |shell, cx| {
            shell.reveal_session(session.clone(), window, cx)
        })
    });
    wait(cx, |cx| {
        shell
            .read(cx)
            .current_chat()
            .is_some_and(|chat| chat.read(cx).connected())
    });
    Key::Session(node, session.id)
}

fn document_panel(shell: &Shell, cx: &App) -> Entity<crate::plugins::Panel> {
    match &shell.side_resource {
        Some(SideResource::Plugin(panel)) if panel.read(cx).documents.is_some() => panel.clone(),
        _ => panic!("document panel expected"),
    }
}

fn documents(shell: &Shell, cx: &App) -> Entity<crate::plugins::documents::Controller> {
    document_panel(shell, cx)
        .read(cx)
        .documents
        .clone()
        .unwrap()
}

fn open_background_session(
    shell: &Entity<Shell>,
    cx: &mut VisualTestContext,
    fixture: &Fixture,
    index: usize,
) {
    let client = Client::new(fixture.nodes[index].local());
    let Output::Session(session) = fixture
        .runtime
        .block_on(client.execute(client.prepare(Command::CreateSession {
            project: fixture.sessions[index].project,
            worktree: Some(fixture.sessions[index].worktree),
            config: Some(fixture.sessions[index].config.clone()),
        })))
        .unwrap()
    else {
        panic!("session expected")
    };
    open(shell, cx, fixture, index, &session);
}

#[test]
fn preserves_promotion_order() {
    let mut state = State::default();
    let session = SessionId::new();
    let a = Key::Session(NodeId([1; 32]), session);
    let b = Key::Session(NodeId([2; 32]), session);
    state.open(a);
    state.open(Key::Draft);
    state.open(b);
    state.open(a);
    assert_eq!(state.open, [a, b]);
    let created = Key::Session(NodeId([1; 32]), SessionId::new());
    state.promote(created);
    assert_eq!(state.open, [a, b, created]);
    assert_eq!(state.active, a);
    state.promote(created);
    assert_eq!(state.open, [a, b, created]);
}

#[test]
fn promotes_without_a_draft_tab() {
    let mut state = State::default();
    state.open(Key::Draft);
    state.open(Key::Draft);
    state.mounted = Some(Key::Draft);
    assert!(state.open.is_empty());
    assert_eq!(state.active, Key::Draft);
    let created = Key::Session(NodeId([1; 32]), SessionId::new());
    state.promote(created);
    assert_eq!(state.open, [created]);
    assert_eq!(state.active, created);
    assert_eq!(state.mounted, Some(created));
}

#[gpui::test]
fn opens_after_first_send(cx: &mut TestAppContext) {
    let fixture = Fixture::new();
    let (shell, visual) = mount(cx, &fixture);
    for index in 0..2 {
        let existing = open(&shell, visual, &fixture, index, &fixture.sessions[index]);
        let binding = shell.read_with(visual, |shell, cx| {
            shell.current_chat().unwrap().read(cx).binding()
        });
        visual.update(|window, cx| {
            shell.update(cx, |shell, cx| {
                shell.new_live_conversation(window, cx);
                shell.close_session_view(existing, window, cx);
            });
        });
        let draft = shell.read_with(visual, |shell, _| shell.current_chat().unwrap().clone());
        visual.update(|window, cx| {
            draft.update(cx, |draft, cx| draft.retarget(binding, window, cx));
            shell.update(cx, |shell, cx| shell.restore_session_scope(cx));
        });
        wait(visual, |cx| draft.read(cx).connected());
        assert!(visual.debug_bounds("session-tabs").is_none());
        assert!(draft.read_with(visual, |draft, _| draft.session().is_none()));
        click(&shell, visual, "live-chat-input".into());
        visual.simulate_input("Create a conversation");
        assert!(visual.debug_bounds("session-tab-draft").is_none());
        click(&shell, visual, "live-chat-send".into());
        wait(visual, |cx| {
            matches!(shell.read(cx).session_scope.active, Key::Session(_, _))
                && draft.read(cx).session().is_some()
        });
        let created = shell.read_with(visual, |shell, _| shell.session_scope.active);
        assert_eq!(
            created,
            Key::Session(
                fixture.nodes[index].id(),
                draft.read_with(visual, |draft, _| draft.session().unwrap())
            )
        );
        assert_eq!(
            shell.read_with(visual, |shell, _| shell.session_scope.open.clone()),
            [created]
        );
        for mode in [ThemeMode::Light, ThemeMode::Dark] {
            for width in [760., 1600.] {
                let handle = visual.update(|window, cx| {
                    crate::theme::select(Some(mode), window, cx);
                    window.window_handle()
                });
                visual.simulate_window_resize(handle, size(px(width), px(900.)));
                visual.run_until_parked();
                visual.update(|window, cx| window.draw(cx).clear(cx));
                assert!(visual.debug_bounds("pane-header").is_none());
                let bar = visual.debug_bounds("shell-module-header").unwrap();
                let toggle = visual.debug_bounds("header-sidebar-toggle").unwrap();
                let content = visual.debug_bounds("live-conversation").unwrap();
                assert!(bar.bottom() <= content.top());
                assert!(bar.size.height <= px(crate::preview::HEADER_HEIGHT));
                let header = visual.debug_bounds("shell-module-header").unwrap();
                let details = visual.debug_bounds("toggle-details").unwrap();
                assert!(details.top() >= header.top());
                assert!(details.bottom() <= header.bottom());
                assert!(details.right() <= header.right());
                assert!(toggle.left() >= header.left());
                assert!(toggle.right() <= header.right());
                assert!(toggle.top() >= header.top());
                assert!(toggle.bottom() <= header.bottom());
                for selector in ["live-search", "live-assets"] {
                    let button = visual.debug_bounds(selector).unwrap();
                    assert!(button.top() >= header.top() && button.bottom() <= header.bottom());
                    assert!(button.right() <= details.left());
                }
                let title = visual.debug_bounds("single-pane-title").unwrap();
                assert!(title.top() >= header.top() && title.bottom() <= header.bottom());
            }
        }
        visual.update(|window, cx| {
            shell.update(cx, |shell, cx| {
                shell.close_session_view(created, window, cx)
            });
        });
        visual.update(|window, cx| window.draw(cx).clear(cx));
        assert!(visual.debug_bounds("session-tabs").is_none());
    }
    fixture.close();
}

#[gpui::test]
fn switch_preserves_tasks(cx: &mut TestAppContext) {
    let fixture = Fixture::new();
    let (shell, visual) = mount(cx, &fixture);
    let a = open(&shell, visual, &fixture, 0, &fixture.sessions[0]);
    click(&shell, visual, "live-chat-input".into());
    visual.simulate_input("Draft A");
    let chat_a = shell.read_with(visual, |shell, _| shell.current_chat().unwrap().clone());
    visual.update(|window, cx| {
        shell.update(cx, |shell, cx| {
            shell.open_destination(crate::resources::launcher::Destination::Browser, window, cx);
            shell.layout.panel_width[0] = 640.;
        })
    });
    let browser = shell.read_with(visual, |shell, cx| {
        shell
            .side_resource
            .as_ref()
            .and_then(|panel| panel.browser(cx))
            .expect("browser expected")
    });
    let b = open(&shell, visual, &fixture, 1, &fixture.sessions[1]);
    assert!(shell.read_with(visual, |shell, _| shell.side_resource.is_none()));
    click(&shell, visual, "live-chat-input".into());
    visual.simulate_input("Draft B");
    visual.update(|window, cx| {
        shell.update(cx, |shell, cx| {
            shell.open_destination(crate::resources::launcher::Destination::Files, window, cx);
            shell.layout.panel_width[0] = 480.;
        })
    });
    let files = shell.read_with(visual, |shell, cx| document_panel(shell, cx).entity_id());
    click(&shell, visual, a.selector());
    assert!(shell.read_with(visual, |shell, cx| {
        shell.current_chat() == Some(&chat_a)
            && chat_a.read(cx).draft(cx).as_ref() == "Draft A"
            && shell.live.as_ref().unwrap().selected == fixture.nodes[0].id()
            && shell.live.as_ref().unwrap().project == fixture.sessions[0].project
            && shell
                .side_resource
                .as_ref()
                .and_then(|panel| panel.browser(cx))
                .as_ref()
                == Some(&browser)
            && shell.layout.panel_width[0] == 640.
    }));
    visual.update(|window, cx| {
        shell.update(cx, |shell, cx| shell.navigate(Page::Settings, window, cx))
    });
    visual.update(|window, cx| window.draw(cx).clear(cx));
    assert!(visual.debug_bounds("session-tabs").is_none());
    visual.update(|window, cx| {
        shell.update(cx, |shell, cx| {
            shell.navigate(Page::Conversation, window, cx)
        })
    });
    click(&shell, visual, b.selector());
    assert!(shell.read_with(visual, |shell, cx| {
        shell.current_chat().unwrap().read(cx).draft(cx).as_ref() == "Draft B"
            && document_panel(shell, cx).entity_id() == files
            && shell.layout.panel_width[0] == 480.
    }));
    for index in 0..2 {
        fixture.submit(index);
    }
    wait(visual, |cx| {
        [a, b].iter().all(|key| {
            let Key::Session(node, id) = key else {
                unreachable!()
            };
            shell.read(cx).chats.views[&(*node, *id)]
                .read(cx)
                .summary()
                .is_some_and(|session| session.activity.waiting.is_some())
        })
    });
    click(&shell, visual, format!("close-{}", b.selector()));
    assert_eq!(
        shell.read_with(visual, |shell, _| shell.session_scope.active),
        a
    );
    let client = Client::new(fixture.nodes[1].local());
    let history = fixture
        .runtime
        .block_on(client.read_conversation(fixture.sessions[1].id, None, 20))
        .unwrap();
    assert_eq!(history.page.runs[0].status, Status::Running);
    fixture.answer(1);
    wait(visual, |cx| {
        shell
            .read(cx)
            .session_unread(fixture.nodes[1].id(), fixture.sessions[1].id)
    });
    assert_eq!(
        shell.read_with(visual, |shell, _| shell.session_scope.open.clone()),
        vec![a]
    );
    open(&shell, visual, &fixture, 1, &fixture.sessions[1]);
    assert_eq!(
        shell.read_with(visual, |shell, cx| document_panel(shell, cx).entity_id()),
        files
    );
    wait(visual, |cx| {
        !shell
            .read(cx)
            .session_unread(fixture.nodes[1].id(), fixture.sessions[1].id)
    });
    click(&shell, visual, "new-conversation".into());
    assert!(visual.debug_bounds("session-tab-draft").is_none());
    assert_eq!(
        shell.read_with(visual, |shell, _| shell.session_scope.open.clone()),
        vec![a, b]
    );
    let draft = shell.read_with(visual, |shell, _| shell.current_chat().unwrap().clone());
    let binding = shell.read_with(visual, |shell, cx| {
        shell.chats.views[&(fixture.nodes[1].id(), fixture.sessions[1].id)]
            .read(cx)
            .binding()
    });
    visual.update(|window, cx| {
        draft.update(cx, |draft, cx| draft.retarget(binding.clone(), window, cx));
        shell.update(cx, |shell, cx| shell.restore_session_scope(cx));
    });
    wait(visual, |cx| {
        draft.read(cx).connected() && shell.read(cx).live.as_ref().unwrap().view.connected
    });
    click(&shell, visual, "live-chat-input".into());
    visual.simulate_input("New draft");
    let binding = draft.read_with(visual, |draft, _| draft.binding());
    visual.update(|window, cx| {
        shell.update(cx, |shell, cx| {
            shell.open_destination(crate::resources::launcher::Destination::Files, window, cx);
        })
    });
    let draft_files = shell.read_with(visual, |shell, cx| document_panel(shell, cx).entity_id());
    click(&shell, visual, a.selector());
    click(&shell, visual, "new-conversation".into());
    assert_eq!(
        shell.read_with(visual, |shell, _| shell.current_chat().unwrap().clone()),
        draft
    );
    assert_eq!(
        draft.read_with(visual, |draft, cx| draft.draft(cx)),
        "New draft"
    );
    assert_eq!(
        draft.read_with(visual, |draft, _| draft.binding().client.target()),
        binding.client.target()
    );
    assert_eq!(
        draft.read_with(visual, |draft, _| draft.binding().worktree),
        binding.worktree
    );
    assert_eq!(
        shell.read_with(visual, |shell, cx| document_panel(shell, cx).entity_id()),
        draft_files
    );
    for key in [a, b] {
        visual.update(|window, cx| {
            shell.update(cx, |shell, cx| shell.close_session_view(key, window, cx))
        });
    }
    visual.update(|window, cx| window.draw(cx).clear(cx));
    assert!(visual.debug_bounds("session-tabs").is_none());
    assert!(visual.debug_bounds("live-chat-input").is_some());
    assert!(shell.read_with(visual, |shell, _| shell.session_scope.open.is_empty()));
    visual.update(|window, cx| {
        shell.update(cx, |shell, cx| {
            shell.navigate(Page::Settings, window, cx);
            shell
                .live
                .as_mut()
                .unwrap()
                .select(fixture.nodes[0].id(), cx);
        })
    });
    wait(visual, |cx| {
        shell.read(cx).live.as_ref().unwrap().view.connected
    });
    visual.update(|window, cx| {
        shell.update(cx, |shell, cx| {
            shell.navigate(Page::Conversation, window, cx)
        })
    });
    assert_eq!(
        shell.read_with(visual, |shell, _| shell.live.as_ref().unwrap().selected),
        binding.client.target()
    );
    assert_eq!(
        shell.read_with(visual, |shell, cx| document_panel(shell, cx).entity_id()),
        draft_files
    );
    wait(visual, |cx| draft.read(cx).connected());
    click(&shell, visual, "live-chat-send".into());
    wait(visual, |cx| {
        matches!(shell.read(cx).session_scope.active, Key::Session(_, _))
    });
    let created = shell.read_with(visual, |shell, _| shell.session_scope.active);
    assert_eq!(
        shell.read_with(visual, |shell, _| shell.session_scope.open.clone()),
        vec![created]
    );
    assert_eq!(
        shell.read_with(visual, |shell, _| shell.current_chat().unwrap().clone()),
        draft
    );
    fixture.close();
}

#[gpui::test]
fn isolates_session_file_drafts(cx: &mut TestAppContext) {
    let fixture = Fixture::new();
    let client = Client::new(fixture.nodes[0].local());
    let execute = |command| {
        fixture
            .runtime
            .block_on(client.execute(client.prepare(command)))
            .unwrap()
    };
    let Output::Snapshot(snapshot) = execute(Command::Snapshot) else {
        panic!("snapshot expected")
    };
    let root = std::path::Path::new(&snapshot.projects[0].path);
    std::fs::write(root.join("draft.txt"), "Original").unwrap();
    let Output::Session(second) = execute(Command::CreateSession {
        project: fixture.sessions[0].project,
        worktree: Some(fixture.sessions[0].worktree),
        config: Some(fixture.sessions[0].config.clone()),
    }) else {
        panic!("session expected")
    };
    let (shell, visual) = mount(cx, &fixture);
    let a = open(&shell, visual, &fixture, 0, &fixture.sessions[0]);
    visual.update(|window, cx| {
        shell.update(cx, |shell, cx| {
            shell.open_documents(
                (fixture.nodes[0].id(), fixture.sessions[0].worktree),
                Some(("draft.txt".into(), None)),
                window,
                cx,
            );
        })
    });
    let panel_a = shell.read_with(visual, |shell, cx| document_panel(shell, cx).entity_id());
    // Switch before the first file read completes, then open the same path in a second panel.
    let b = open(&shell, visual, &fixture, 0, &second);
    visual.update(|window, cx| {
        shell.update(cx, |shell, cx| {
            shell.open_documents(
                (fixture.nodes[0].id(), fixture.sessions[0].worktree),
                Some(("draft.txt".into(), None)),
                window,
                cx,
            );
        })
    });
    wait(visual, |cx| {
        documents(shell.read(cx), cx).read(cx).editor("draft.txt").is_some()
        && shell.read(cx).retained_panels().any(|resource|
            matches!(resource, SideResource::Plugin(panel) if panel.entity_id() == panel_a
                && panel.read(cx).documents.as_ref().is_some_and(|documents| documents.read(cx).editor("draft.txt").is_some())))
    });
    let editor_b = shell.read_with(visual, |shell, cx| {
        documents(shell, cx).read(cx).editor("draft.txt").unwrap()
    });
    visual.update(|window, cx| {
        editor_b.update(cx, |editor, cx| editor.set_value("File B", window, cx))
    });
    click(&shell, visual, a.selector());
    let editor_a = shell.read_with(visual, |shell, cx| {
        documents(shell, cx).read(cx).editor("draft.txt").unwrap()
    });
    assert_ne!(editor_a, editor_b);
    visual.update(|window, cx| {
        editor_a.update(cx, |editor, cx| editor.set_value("File A", window, cx))
    });
    click(&shell, visual, b.selector());
    assert!(!visual.update(|window, cx| window.has_active_dialog(cx)));
    assert_eq!(
        editor_b.read_with(visual, |editor, _| editor.value()),
        "File B"
    );
    assert_eq!(
        editor_a.read_with(visual, |editor, _| editor.value()),
        "File A"
    );
    assert!(shell.read_with(visual, |shell, cx| {
        documents(shell, cx).read(cx).has_unsaved(cx)
    }));
    visual
        .update(|window, cx| shell.update(cx, |shell, cx| shell.close_session_view(a, window, cx)));
    open(&shell, visual, &fixture, 0, &fixture.sessions[0]);
    assert_eq!(
        shell.read_with(visual, |shell, cx| document_panel(shell, cx).entity_id()),
        panel_a
    );
    assert_eq!(
        editor_a.read_with(visual, |editor, _| editor.value()),
        "File A"
    );
    assert_eq!(
        std::fs::read_to_string(root.join("draft.txt")).unwrap(),
        "Original"
    );
    // Saving belongs to the originating panel even after another tab is selected.
    click(&shell, visual, "files_save".into());
    click(&shell, visual, b.selector());
    wait(visual, |cx| {
        shell.read(cx).retained_panels().any(|resource|
        matches!(resource, SideResource::Plugin(panel) if panel.entity_id() == panel_a
            && panel.read(cx).documents.as_ref().is_some_and(|documents| !documents.read(cx).has_unsaved(cx))))
    });
    assert_eq!(
        std::fs::read_to_string(root.join("draft.txt")).unwrap(),
        "File A"
    );
    assert_eq!(
        editor_b.read_with(visual, |editor, _| editor.value()),
        "File B"
    );
    assert!(shell.read_with(visual, |shell, cx| {
        documents(shell, cx).read(cx).has_unsaved(cx)
    }));
    fixture.close();
}

#[gpui::test]
fn indented_title_spacing(cx: &mut TestAppContext) {
    const INPUT: &str = "\n  请在当前项目中完成一次只读检查\n  ";
    let fixture = Fixture::new();
    for index in 0..2 {
        let client = Client::new(fixture.nodes[index].local());
        fixture
            .runtime
            .block_on(client.execute(client.prepare(Command::QueueTurn {
                session: fixture.sessions[index].id,
                expected_revision: 1,
                message: INPUT.into(),
            })))
            .unwrap();
    }
    let (shell, visual) = mount(cx, &fixture);
    for index in 0..2 {
        open(&shell, visual, &fixture, index, &fixture.sessions[index]);
        wait(visual, |cx| {
            shell.read(cx).current_chat().is_some_and(|chat| {
                chat.read(cx)
                    .summary()
                    .is_some_and(|session| session.activity.title == INPUT.trim())
            })
        });
        shell.read_with(visual, |shell, cx| {
            let session = shell.current_chat().unwrap().read(cx).summary().unwrap();
            assert_eq!(session.activity.title, INPUT.trim());
            assert_eq!(crate::activity::title(session).as_ref(), INPUT.trim());
        });
        {
            let prefix = format!("live-session-{}", fixture.sessions[index].id);
            let bounds = |visual: &mut VisualTestContext, suffix: &str| {
                let selector = Box::leak(format!("{prefix}-{suffix}").into_boxed_str());
                visual.debug_bounds(selector).unwrap()
            };
            let icon = Box::leak(format!("{prefix}-icon").into_boxed_str());
            assert!(visual.debug_bounds(icon).is_none());
            let guide = bounds(visual, "guide");
            let label = bounds(visual, "label");
            assert!(
                guide.right() < label.left(),
                "guide: {guide:?}, label: {label:?}"
            );
            let loading = bounds(visual, "loading");
            assert!(loading.left() >= label.right());
            assert!((label.center().y - loading.center().y).abs() < px(1.));
            let archive = Box::leak(
                format!("session-sidebar-archive-{}", fixture.sessions[index].id).into_boxed_str(),
            );
            assert!(visual.debug_bounds(archive).is_none());
            visual.simulate_mouse_move(label.center(), None, Modifiers::default());
            wait(visual, |_| true);
            assert!(visual.debug_bounds(archive).is_none());
            assert_eq!(bounds(visual, "label").left(), label.left());
            visual.simulate_mouse_move(point(px(1.), px(1.)), None, Modifiers::default());
            wait(visual, |_| true);
            assert!(visual.debug_bounds(archive).is_none());
            assert_eq!(bounds(visual, "label").size.width, label.size.width);

            // Sidebar text uses the theme's 22.5 px line box; tabs use 14 px.
            let line_height = if prefix.starts_with("live-session-") {
                24.
            } else {
                20.
            };
            assert!(
                label.size.height <= px(line_height),
                "title must stay on one line: {prefix}: {label:?}"
            );
        }
    }
    visual.update(|window, _| window.remove_window());
    fixture.close();
}

mod attention;
mod recovery;
