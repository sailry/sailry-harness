use super::*;
use sailry_protocol::plugin;

fn mount<'a>(
    fixture: &Fixture,
    cx: &'a mut TestAppContext,
) -> (Entity<Panel>, &'a mut VisualTestContext) {
    let mut binding = fixture.binding.clone();
    binding.project = None;
    binding.worktree = None;
    let mut panel = None;
    let (_, visual) = cx.add_window_view(|window, cx| {
        let view = cx.new(|cx| Panel::standalone(binding, cx));
        panel = Some(view.clone());
        let content = cx.new(|cx| {
            cx.observe(&view, |_, _, cx| cx.notify()).detach();
            Harness(view)
        });
        Root::new(content, window, cx)
    });
    let panel = panel.unwrap();
    wait(visual, |cx| {
        panel.read(cx).ready_for(&reference(fixture), cx)
    });
    (panel, visual)
}

fn reference(fixture: &Fixture) -> plugin::Reference {
    let Output::Snapshot(snapshot) = fixture.execute(Command::Snapshot) else {
        panic!("snapshot expected")
    };
    snapshot
        .plugins
        .iter()
        .find(|entry| entry.name == "task-notes")
        .unwrap()
        .reference()
}

fn chat(panel: &Entity<Panel>, cx: &App) -> Option<Entity<crate::conversation::live::View>> {
    panel.read(cx).mounted.as_ref()?.conversations.first()
}

#[gpui::test]
fn restores(cx: &mut TestAppContext) {
    init(cx);
    for remote in [false, true] {
        let fixture = Fixture::new(remote);
        let package = package(&fixture, true);
        let server = fixture
            .runtime
            .block_on(crate::agent_fixture::Server::tools(Vec::new()));
        let Output::Snapshot(snapshot) = fixture.execute(Command::Snapshot) else {
            panic!("snapshot expected")
        };
        let mut provider = snapshot.providers[0].clone();
        provider.endpoint = server.endpoint.clone();
        fixture.execute(Command::PutProvider {
            expected_revision: provider.revision,
            provider,
        });
        let (panel, visual) = mount(&fixture, cx);
        visual.update(|window, cx| {
            panel.update(cx, |panel, cx| {
                panel.open(package.summary.reference(), window, cx)
            })
        });
        wait(visual, |cx| {
            chat(&panel, cx).is_some_and(|chat| chat.read(cx).connected())
        });
        assert!(server.requests.lock().unwrap().is_empty());
        let Output::Snapshot(snapshot) = fixture.execute(Command::Snapshot) else {
            panic!("snapshot expected")
        };
        assert_eq!(
            snapshot.sessions.len(),
            1,
            "mounting must not create a session"
        );
        click(visual, "live-chat-input");
        visual.simulate_input("Organize my notes");
        visual.simulate_keystrokes("enter");
        wait(visual, |cx| {
            chat(&panel, cx).is_some_and(|chat| chat.read(cx).session().is_some())
        });
        let id = panel.read_with(visual, |panel, cx| {
            panel
                .mounted
                .as_ref()
                .unwrap()
                .conversations
                .first()
                .unwrap()
                .read(cx)
                .session()
                .unwrap()
        });
        wait(visual, |_| {
            let Output::Conversation(history) = fixture.execute(Command::ReadConversation {
                session: id,
                before: None,
                limit: 100,
            }) else {
                return false;
            };
            history
                .page
                .runs
                .iter()
                .any(|run| run.status == Status::Completed)
        });
        let Output::Snapshot(snapshot) = fixture.execute(Command::Snapshot) else {
            panic!("snapshot expected")
        };
        let created = snapshot
            .sessions
            .iter()
            .find(|session| session.id == id)
            .unwrap();
        assert!(created.project.is_none());
        assert_ne!(created.worktree, fixture.session.worktree);
        assert_eq!(
            created.config.assistant,
            Some(plugin::conversation::Binding {
                package: package.summary.reference(),
                id: "notes".into()
            })
        );
        assert!(
            snapshot
                .turns
                .iter()
                .filter(|turn| turn.session == id)
                .any(|turn| turn.plugins.contains(&package.summary.reference()))
        );
        assert!(
            server.requests.lock().unwrap()[0]
                .to_string()
                .contains("Help the user organize notes")
        );
        let Output::Snapshot(controller) = fixture.runtime.block_on(async {
            let client = sailry_client::Client::new(fixture.controller.local());
            client
                .execute(client.prepare(Command::Snapshot))
                .await
                .unwrap()
        }) else {
            panic!("snapshot expected")
        };
        assert!(controller.sessions.is_empty());
        wait(visual, |cx| {
            panel
                .read(cx)
                .snapshot
                .borrow()
                .as_ref()
                .is_some_and(|snapshot| snapshot.sessions.iter().any(|session| session.id == id))
        });
        let weak = panel.read_with(visual, |_, cx| chat(&panel, cx).unwrap().downgrade());
        visual.update(|_, cx| panel.update(cx, |panel, cx| panel.back(cx)));
        wait(visual, |_| weak.upgrade().is_none());
        visual.update(|window, cx| {
            panel.update(cx, |panel, cx| {
                panel.open(package.summary.reference(), window, cx)
            })
        });
        wait(visual, |cx| {
            chat(&panel, cx).is_some_and(|chat| {
                chat.read(cx).session() == Some(id) && chat.read(cx).connected()
            })
        });
        click(visual, "connection-chat-new");
        wait(visual, |cx| {
            chat(&panel, cx).is_some_and(|chat| chat.read(cx).session().is_none())
        });
        let Output::Snapshot(snapshot) = fixture.execute(Command::Snapshot) else {
            panic!("snapshot expected")
        };
        assert_eq!(
            snapshot.sessions.len(),
            2,
            "new draft must not create a session"
        );
        visual.update(|window, _| window.remove_window());
        drop(panel);
        fixture.close();
    }
}

struct Routed {
    panel: Entity<Panel>,
    shell: Entity<crate::shell::Shell>,
}

impl Render for Routed {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if self.shell.read(cx).page == crate::preview::Page::Conversation {
            self.shell.clone().into_any_element()
        } else {
            self.panel.clone().into_any_element()
        }
    }
}

#[gpui::test]
fn opens_file(cx: &mut TestAppContext) {
    use crate::{preview::Page, resources::SideResource, shell::Shell};
    init(cx);
    for remote in [false, true] {
        let fixture = Fixture::new(remote);
        let original = package(&fixture, true);
        let path = fixture.directory.path().join("project/package/plugin.json");
        let mut manifest: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        manifest["extensions"]["dev.sailry.platform"]["desktop"]["conversations"][0]["tools"] =
            serde_json::json!([{ "kind": "package", "package":"files", "name": "read_file" }]);
        std::fs::write(path, manifest.to_string()).unwrap();
        let Output::Plugin(package) = fixture.execute(Command::InstallPlugin {
            worktree: fixture.session.worktree,
            path: "package".into(),
            name: "task-notes".into(),
            expected_revision: original.summary.revision,
        }) else {
            panic!("plugin expected")
        };
        let server = fixture
            .runtime
            .block_on(crate::agent_fixture::Server::tools(vec![(
                crate::agent_fixture::plugin_tool("files", "read_file"),
                serde_json::json!({"path":"origin.txt"}),
            )]));
        let Output::Snapshot(snapshot) = fixture.execute(Command::Snapshot) else {
            panic!("snapshot expected")
        };
        let mut provider = snapshot.providers[0].clone();
        provider.endpoint = server.endpoint.clone();
        fixture.execute(Command::PutProvider {
            expected_revision: provider.revision,
            provider,
        });
        let mut config = fixture.session.config.clone();
        config.assistant = Some(plugin::conversation::Binding {
            package: package.summary.reference(),
            id: "notes".into(),
        });
        let Output::Session(session) = fixture.execute(Command::CreateSession {
            project: None,
            worktree: None,
            config: Some(config),
        }) else {
            panic!("session expected")
        };
        fixture.execute(Command::WriteFile {
            worktree: session.worktree,
            path: "origin.txt".into(),
            text: "Captured Node file".into(),
            expected_revision: None,
        });
        let Output::QueuedTurn(turn) = fixture.execute(Command::SubmitTurn {
            session: session.id,
            expected_revision: session.revision,
            message: "Read the file".into(),
        }) else {
            panic!("turn expected")
        };
        cx.update(|cx| cx.set_global(fixture.services(remote)));
        let mut binding = fixture.binding.clone();
        binding.project = None;
        binding.worktree = None;
        let mut mounted = None;
        let (_, visual) = cx.add_window_view(|window, cx| {
            let panel = cx.new(|cx| Panel::standalone(binding, cx));
            let shell = cx.new(|cx| {
                let mut shell = Shell::new(window, cx);
                shell.page = Page::Plugin;
                Shell::observe_plugin_conversations(&panel, window, cx);
                shell
            });
            mounted = Some((panel.clone(), shell.clone()));
            let content = cx.new(|cx| {
                cx.observe(&panel, |_, _, cx| cx.notify()).detach();
                cx.observe(&shell, |_, _, cx| cx.notify()).detach();
                Routed { panel, shell }
            });
            Root::new(content, window, cx)
        });
        let (panel, shell) = mounted.unwrap();
        wait(visual, |cx| {
            let Output::Conversation(history) = fixture.execute(Command::ReadConversation {
                session: session.id,
                before: None,
                limit: 100,
            }) else {
                return false;
            };
            history
                .page
                .runs
                .iter()
                .any(|run| run.status == Status::Completed)
                && panel.read(cx).ready_for(&package.summary.reference(), cx)
                && shell
                    .read(cx)
                    .live
                    .as_ref()
                    .unwrap()
                    .hosts
                    .contains_key(&fixture.node.id())
        });
        visual.update(|window, cx| {
            shell.update(cx, |shell, cx| {
                shell
                    .live
                    .as_mut()
                    .unwrap()
                    .select(fixture.controller.id(), cx)
            });
            panel.update(cx, |panel, cx| {
                panel.open(package.summary.reference(), window, cx)
            });
        });
        wait(visual, |cx| {
            let Output::Conversation(history) = fixture.execute(Command::ReadConversation {
                session: session.id,
                before: None,
                limit: 100,
            }) else {
                return false;
            };
            history
                .page
                .runs
                .iter()
                .any(|run| run.status == Status::Completed)
                && chat(&panel, cx).is_some_and(|chat| chat.read(cx).connected())
        });
        let Output::Conversation(history) = fixture.execute(Command::ReadConversation {
            session: session.id,
            before: None,
            limit: 100,
        }) else {
            panic!("history expected")
        };
        let source = history.page.entries.iter().find_map(|entry| {
            entry.parts.iter().enumerate().find_map(|(index, part)| {
                matches!(part, sailry_protocol::conversation::Part::ToolCall { name, .. } if name == &crate::agent_fixture::plugin_tool("files", "read_file"))
                    .then(|| format!("{}-{index}", entry.id))
            })
        }).unwrap();
        let turn_selector = Box::leak(format!("live-turn-work-{}", turn.id).into_boxed_str());
        click(visual, turn_selector);
        click(
            visual,
            Box::leak(format!("live-{}-tool-group-{source}", turn.id).into_boxed_str()),
        );
        let file = Box::leak(format!("live-tool-file-{}-{source}-0", turn.id).into_boxed_str());
        for enabled in [false, true] {
            let Output::Snapshot(snapshot) = fixture.execute(Command::Snapshot) else {
                panic!("snapshot expected")
            };
            let plugin = snapshot
                .plugins
                .iter()
                .find(|plugin| plugin.name == "files")
                .unwrap();
            fixture.execute(Command::SetPluginEnabled {
                name: plugin.name.clone(),
                enabled,
                expected_revision: plugin.revision,
            });
            wait(visual, |cx| {
                panel
                    .read(cx)
                    .snapshot
                    .borrow()
                    .as_ref()
                    .is_some_and(|snapshot| {
                        snapshot
                            .plugins
                            .iter()
                            .any(|plugin| plugin.name == "files" && plugin.enabled == enabled)
                    })
            });
            wait(visual, |cx| {
                let chat = if shell.read(cx).page == Page::Conversation {
                    shell.read(cx).current_chat().cloned()
                } else {
                    chat(&panel, cx)
                };
                chat.is_some_and(|chat| {
                    let chat = chat.read(cx);
                    let resource = sailry_protocol::plugin::desktop::ResourceKind::Documents;
                    if enabled {
                        chat.renderer_available(resource, cx)
                    } else {
                        chat.renderer_unavailable(resource, cx)
                    }
                })
            });
            if visual.debug_bounds(file).is_none() {
                click(visual, turn_selector);
                click(
                    visual,
                    Box::leak(format!("live-{}-tool-group-{source}", turn.id).into_boxed_str()),
                );
            }
            click(visual, file);
            if !enabled {
                assert_eq!(
                    shell.read_with(visual, |shell, _| shell.page),
                    Page::Conversation
                );
                assert!(shell.read_with(visual, |shell, _| shell.side_resource.is_none()));
            }
        }
        wait(visual, |cx| {
            matches!(&shell.read(cx).side_resource, Some(SideResource::Plugin(panel))
                if panel.read(cx).document_scope(cx) == Some((fixture.node.id(), session.worktree))
                && panel.read(cx).documents.as_ref().and_then(|documents| documents.read(cx).editor("origin.txt"))
                    .is_some_and(|editor| editor.read(cx).value() == "Captured Node file"))
        });
        assert!(shell.read_with(visual, |shell, cx| {
            shell.current_chat().is_some_and(|chat| {
                chat.read(cx).session() == Some(session.id)
                    && chat.read(cx).binding().client.target() == fixture.node.id()
            })
        }));
        visual.update(|window, _| window.remove_window());
        drop(panel);
        drop(shell);
        fixture.close();
    }
}
