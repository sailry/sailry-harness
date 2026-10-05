use super::*;
use core::prelude::v1::test;
use sailry_node_runtime::Node;
use sailry_protocol::{
    Effort, ProviderId, SessionConfig,
    conversation::{Model, ModelApi, Provider},
};
use std::time::{Duration, Instant};

mod draft;

#[track_caller]
pub(in crate::live) fn wait(cx: &mut VisualTestContext, predicate: impl Fn(&App) -> bool) {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        cx.run_until_parked();
        if cx.update(|window, cx| {
            let _ = window.draw(cx);
            predicate(cx)
        }) {
            return;
        }
        assert!(Instant::now() < deadline, "session navigation deadline");
        std::thread::sleep(Duration::from_millis(10));
    }
}

pub(super) fn click(cx: &mut VisualTestContext, selector: &'static str) {
    if cx.update(|window, cx| window.has_active_dialog(cx)) {
        // Kit dialog geometry follows wall-clock animation, not the test clock.
        std::thread::sleep(*gpui_kit::component::dialog::ANIMATION_DURATION);
        cx.run_until_parked();
    }
    cx.update(|window, cx| {
        let _ = window.draw(cx);
    });
    let bounds = cx
        .debug_bounds(selector)
        .unwrap_or_else(|| panic!("missing {selector}"));
    cx.simulate_click(bounds.center(), Modifiers::default());
    cx.run_until_parked();
}

#[gpui::test]
fn preserves_drafts(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    let directory = tempfile::tempdir().unwrap();
    let runtime = Arc::new(tokio::runtime::Runtime::new().unwrap());
    let local = runtime
        .block_on(Node::start(directory.path().join("local")))
        .unwrap();
    let remote = runtime
        .block_on(Node::start(directory.path().join("remote")))
        .unwrap();
    runtime
        .block_on(local.link().pair(remote.link().invite().unwrap().ticket()))
        .unwrap();
    cx.update(|cx| {
        gpui_kit::init(cx);
        crate::theme::init(cx);
        crate::shell::init(cx);
        cx.set_global(Services {
            runtime: runtime.clone(),
            local: local.local(),
            link: local.link(),
            relay_enabled: false,
        });
    });
    for node in [&local, &remote] {
        let client = Client::new(node.local());
        let provider = Provider {
            options: None,
            oauth: None,
            id: ProviderId::new(),
            revision: 0,
            name: "Navigation fixture".into(),
            api: ModelApi::ChatCompletions,
            authentication: sailry_protocol::Authentication::ApiKey,
            endpoint: "http://127.0.0.1:12345/v1".into(),
            enabled: true,
            credential: None,
            default_model: "fixture-model".into(),
            models: vec![Model {
                id: "fixture-model".into(),
                context: 4096,
                output: 128,
                vision: false,
                tools: false,
                reasoning: false,
                web_search: false,
                generates: vec![],
                efforts: Vec::new(),
                custom_efforts: false,
                default_effort: sailry_protocol::Effort::Default,
            }],
        };
        runtime
            .block_on(client.execute(client.prepare(Command::PutProvider {
                provider: provider.clone(),
                expected_revision: 0,
            })))
            .unwrap();
        let Output::Project(project) = runtime
            .block_on(client.execute(client.prepare(Command::RegisterProject {
                name: "Navigation fixture".into(),
                path: directory.path().to_str().unwrap().into(),
            })))
            .unwrap()
        else {
            panic!("project expected")
        };
        let Output::Session(session) = runtime
            .block_on(client.execute(client.prepare(Command::CreateSession {
                project: Some(project.id),
                worktree: None,
                config: Some(SessionConfig {
                    assistant: None,
                    resource: None,
                    provider: provider.id,
                    model: "fixture-model".into(),
                    effort: Effort::Medium,
                    mode: sailry_protocol::WorkMode::Code,
                    permission: sailry_protocol::Permission::Ask,
                    credential: None,
                }),
            })))
            .unwrap()
        else {
            panic!("session expected")
        };
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
                .unwrap()
                .hosts
                .contains_key(&node.id())
        });
        visual.update(|_, cx| {
            shell.update(cx, |shell, cx| {
                shell.live.as_mut().unwrap().select(node.id(), cx)
            })
        });
        wait(visual, |cx| {
            shell
                .read(cx)
                .live
                .as_ref()
                .unwrap()
                .view
                .snapshot
                .as_ref()
                .is_some_and(|snapshot| snapshot.sessions.iter().any(|item| item.id == session.id))
        });
        visual.update(|window, cx| {
            shell.update(cx, |shell, cx| {
                shell.live.as_mut().unwrap().project = Some(project.id);
                shell.new_live_conversation(window, cx);
            })
        });
        wait(visual, |cx| {
            shell
                .read(cx)
                .current_chat()
                .is_some_and(|view| view.read(cx).connected())
        });
        let draft = shell.read_with(visual, |shell, _| shell.current_chat().unwrap().entity_id());
        assert!(visual.debug_bounds("session-tabs").is_none());
        click(visual, "live-chat-input");
        visual.simulate_input("unsent draft");
        // The pinned GPUI test API requires static selectors.
        visual.update(|window, cx| {
            shell.update(cx, |shell, cx| {
                shell.select_live_project(project.id, window, cx)
            })
        });
        let selector = Box::leak(format!("live-session-{}", session.id).into_boxed_str());
        let record = Box::leak(format!("project-record-{}", session.id).into_boxed_str());
        click(visual, record);
        wait(visual, |cx| {
            shell.read(cx).current_chat().is_some_and(|view| {
                view.read(cx).session() == Some(session.id) && view.read(cx).connected()
            })
        });
        let conversation =
            shell.read_with(visual, |shell, _| shell.current_chat().unwrap().entity_id());
        let chat = shell.read_with(visual, |shell, _| shell.current_chat().unwrap().clone());
        for event in [
            Event::HostPage(node.id()),
            Event::ProjectPage(node.id(), project.id),
        ] {
            visual.update(|_, cx| {
                shell.update(cx, |shell, cx| {
                    let other = if node.id() == local.id() {
                        remote.id()
                    } else {
                        local.id()
                    };
                    shell.live.as_mut().unwrap().select(other, cx);
                });
                chat.update(cx, |_, cx| cx.emit(event.clone()));
            });
            visual.run_until_parked();
            shell.read_with(visual, |shell, cx| {
                let live = shell.live.as_ref().unwrap();
                assert_eq!(live.selected, node.id());
                if matches!(event, Event::HostPage(_)) {
                    assert_eq!(shell.page, Page::Host);
                } else {
                    assert_eq!(shell.page, Page::Project);
                    assert_eq!(live.project, Some(project.id));
                }
                assert_eq!(chat.read(cx).binding().client.target(), node.id());
                assert_eq!(chat.read(cx).session(), Some(session.id));
            });
        }
        visual.update(|window, cx| {
            shell.update(cx, |shell, cx| {
                shell.navigate(Page::Settings, window, cx);
                shell.navigate(Page::Conversation, window, cx);
            })
        });
        assert_eq!(
            shell.read_with(visual, |shell, _| shell.current_chat().unwrap().entity_id()),
            conversation
        );
        #[cfg(target_os = "macos")]
        {
            visual.dispatch_action(crate::app_menu::NewConversation);
            visual.run_until_parked();
        }
        #[cfg(not(target_os = "macos"))]
        visual.update(|window, cx| {
            shell.update(cx, |shell, cx| shell.start_conversation(window, cx))
        });
        assert_eq!(
            shell.read_with(visual, |shell, _| shell.current_chat().unwrap().entity_id()),
            draft
        );
        assert!(shell.read_with(visual, |shell, _| shell.conversations.is_empty()));
        let Output::Snapshot(snapshot) = runtime
            .block_on(client.execute(client.prepare(Command::Snapshot)))
            .unwrap()
        else {
            panic!("snapshot expected")
        };
        assert_eq!(snapshot.sessions.len(), 1);
        assert!(snapshot.turns.is_empty());
        let Output::Session(created) = runtime
            .block_on(client.execute(client.prepare(Command::CreateSession {
                project: Some(project.id),
                worktree: Some(session.worktree),
                config: Some(session.config.clone()),
            })))
            .unwrap()
        else {
            panic!("session expected")
        };
        let draft = shell.read_with(visual, |shell, _| shell.current_chat().unwrap().clone());
        // Wait for the shared list to reorder before clicking the existing session.
        wait(visual, |cx| {
            shell
                .read(cx)
                .live
                .as_ref()
                .unwrap()
                .view
                .snapshot
                .as_ref()
                .is_some_and(|snapshot| snapshot.sessions.iter().any(|item| item.id == created.id))
        });
        click(visual, selector);
        wait(visual, |cx| {
            shell.read(cx).current_chat().is_some_and(|view| {
                view.read(cx).session() == Some(session.id) && view.read(cx).connected()
            })
        });
        visual.update(|_, cx| {
            draft.update(cx, |_, cx| {
                cx.emit(Event::Created(Box::new(created.clone())))
            });
        });
        visual.run_until_parked();
        assert!(shell.read_with(visual, |shell, _| {
            shell
                .session_scope
                .open
                .contains(&Key::Session(node.id(), created.id))
                && !shell.session_scope.open.contains(&Key::Draft)
        }));
        assert_eq!(
            shell.read_with(visual, |shell, cx| shell
                .current_chat()
                .unwrap()
                .read(cx)
                .session()),
            Some(session.id)
        );
        assert_eq!(
            shell.read_with(visual, |shell, _| shell.chats.views
                [&(node.id(), created.id)]
                .clone()),
            draft
        );
        visual.update(|window, cx| {
            shell.update(cx, |shell, cx| {
                shell.select_live_project(project.id, window, cx)
            });
            window.draw(cx).clear(cx);
        });
        let newest_record = Box::leak(format!("project-record-{}", created.id).into_boxed_str());
        let newest_sidebar = Box::leak(format!("live-session-{}", created.id).into_boxed_str());
        assert!(
            visual.debug_bounds(newest_record).unwrap().top()
                < visual.debug_bounds(record).unwrap().top()
        );
        assert!(
            visual.debug_bounds(newest_sidebar).unwrap().top()
                < visual.debug_bounds(selector).unwrap().top()
        );
        let other = if node.id() == local.id() {
            &remote
        } else {
            &local
        };
        let other_client = Client::new(other.local());
        let Output::Snapshot(before) = runtime
            .block_on(other_client.execute(other_client.prepare(Command::Snapshot)))
            .unwrap()
        else {
            panic!("snapshot expected")
        };
        visual.update(|_, cx| {
            shell.update(cx, |shell, cx| {
                shell.live.as_mut().unwrap().select(other.id(), cx)
            });
        });
        // A session action captured on another Node must not affect either selection.
        visual.update(|window, cx| {
            shell.update(cx, |shell, cx| {
                shell.live_resource_action(
                    &crate::live::menus::Dispatch {
                        node: node.id(),
                        target: crate::live::menus::Target::Session(session.id),
                        command: crate::live::menus::Command::Archive,
                    },
                    window,
                    cx,
                );
            });
        });
        let Output::Snapshot(original) = runtime
            .block_on(client.execute(client.prepare(Command::Snapshot)))
            .unwrap()
        else {
            panic!("snapshot expected")
        };
        let unchanged = original
            .sessions
            .iter()
            .find(|entry| entry.id == session.id)
            .unwrap();
        assert_eq!(unchanged.revision, 1);
        assert!(!unchanged.archived);
        let Output::Snapshot(after) = runtime
            .block_on(other_client.execute(other_client.prepare(Command::Snapshot)))
            .unwrap()
        else {
            panic!("snapshot expected")
        };
        assert_eq!(before.sessions, after.sessions);
        visual.update(|window, _| window.remove_window());
    }
    runtime.block_on(local.shutdown()).unwrap();
    runtime.block_on(remote.shutdown()).unwrap();
}
