use super::*;
use crate::preview::Page;
use sailry_client::Client;
use sailry_protocol::{
    Output,
    plugin::{Scope, desktop::ResourceKind},
};

fn execute_on(fixture: &Fixture, client: &Client, command: Command) -> Output {
    fixture
        .runtime
        .block_on(client.execute(client.prepare(command)))
        .unwrap()
}

fn set_on(fixture: &Fixture, client: &Client, name: &str, enabled: bool) {
    let Output::Plugin(info) =
        execute_on(fixture, client, Command::ReadPlugin { name: name.into() })
    else {
        panic!("plugin expected")
    };
    let Output::Plugin(updated) = execute_on(
        fixture,
        client,
        Command::SetPluginEnabled {
            name: name.into(),
            expected_revision: info.summary.revision,
            enabled,
        },
    ) else {
        panic!("plugin expected")
    };
    assert_eq!(updated.summary.enabled, enabled);
}

fn set_name(fixture: &Fixture, name: &str, enabled: bool) {
    let Output::Snapshot(snapshot) = fixture.execute(Command::Snapshot) else {
        panic!("snapshot expected")
    };
    let entry = snapshot
        .plugins
        .iter()
        .find(|entry| entry.name == name)
        .unwrap();
    fixture.execute(Command::SetPluginEnabled {
        name: entry.name.clone(),
        expected_revision: entry.revision,
        enabled,
    });
}

fn reminder(fixture: &Fixture, client: &Client, title: &str, project: sailry_protocol::ProjectId) {
    let Output::Plugin(info) = execute_on(
        fixture,
        client,
        Command::ReadPlugin {
            name: "reminders".into(),
        },
    ) else {
        panic!("plugin expected");
    };
    let request = client.prepare(Command::CallPlugin {
        handler: "save".into(),
        input: serde_json::json!({"revision":"0","title":title,"message":"","project":project,"completed":false,"due_ms":null}),
    }).with_plugin(sailry_protocol::plugin::Context {
        invocation: None, turn: None, surface: Default::default(),
        package: info.summary.reference(), worktree: None, session: None,
    });
    let Output::PluginResult(result) = fixture.runtime.block_on(client.execute(request)).unwrap()
    else {
        panic!("reminder result expected");
    };
    assert!(result.get("Ok").is_some(), "{result}");
}

#[gpui::test]
fn routes_entries_and_keeps_host_scope(cx: &mut TestAppContext) {
    init(cx);
    for remote in [false, true] {
        let fixture = Fixture::new(remote);
        fixture.package();
        fixture.install(0);
        let local = Client::new(fixture.services(remote).local);
        reminder(
            &fixture,
            &fixture.binding.client,
            "Selected-node reminder",
            fixture.binding.project.unwrap(),
        );
        if remote {
            let path = fixture.directory.path().join("controller-project");
            std::fs::create_dir(&path).unwrap();
            let Output::Project(project) = execute_on(
                &fixture,
                &local,
                Command::RegisterProject {
                    name: "Controller project".into(),
                    path: path.to_str().unwrap().into(),
                },
            ) else {
                panic!("project expected");
            };
            reminder(&fixture, &local, "Controller reminder", project.id);
        }
        cx.update(|cx| cx.set_global(fixture.services(remote)));
        let mut owner = None;
        let (_, visual) = cx.add_window_view(|window, cx| {
            let shell = cx.new(|cx| Shell::new(window, cx));
            owner = Some(shell.clone());
            Root::new(shell, window, cx)
        });
        let shell = owner.unwrap();
        wait(visual, |cx| {
            shell
                .read(cx)
                .live
                .as_ref()
                .unwrap()
                .hosts
                .contains_key(&fixture.node.id())
        });
        visual.update(|_, cx| {
            shell.update(cx, |shell, cx| {
                shell.live.as_mut().unwrap().select(fixture.node.id(), cx)
            })
        });
        wait(visual, |cx| {
            shell
                .read(cx)
                .extensions
                .as_ref()
                .is_some_and(|state| state.metadata.read(cx).settled())
                && ["databases", "progress"].into_iter().all(|name| {
                    shell
                        .read(cx)
                        .extension_entries(cx)
                        .iter()
                        .any(|entry| entry.package.name == name && entry.node == fixture.node.id())
                })
        });
        let database_selector = shell.read_with(visual, |shell, cx| {
            shell
                .extension_entries(cx)
                .into_iter()
                .find(|entry| entry.package.name == "databases")
                .unwrap()
                .selector()
        });
        let database_selector = Box::leak(database_selector.into_boxed_str());
        let ssh_selector = shell.read_with(visual, |shell, cx| {
            shell
                .extension_entries(cx)
                .into_iter()
                .find(|entry| entry.package.name == "ssh")
                .unwrap()
                .selector()
        });
        let ssh_selector = Box::leak(ssh_selector.into_boxed_str());
        let activity_selector = shell.read_with(visual, |shell, cx| {
            shell
                .extension_entries(cx)
                .into_iter()
                .find(|entry| entry.package.name == "progress")
                .unwrap()
                .selector()
        });
        let activity_selector = Box::leak(activity_selector.into_boxed_str());
        assert!(visual.debug_bounds(database_selector).is_some());
        assert!(visual.debug_bounds(activity_selector).is_some());
        wait(visual, |cx| {
            shell
                .read(cx)
                .extension_entries(cx)
                .iter()
                .any(|entry| entry.package.name == "reminders")
                && shell
                    .read(cx)
                    .extension_entries(cx)
                    .iter()
                    .any(|entry| entry.package.name == "scheduled-tasks")
        });
        let reminders = shell.read_with(visual, |shell, cx| {
            shell
                .extension_entries(cx)
                .into_iter()
                .find(|entry| entry.package.name == "reminders")
                .unwrap()
        });
        assert_eq!(reminders.node, fixture.node.id());
        assert_eq!(reminders.scope, Scope::Host);
        assert!(reminders.navigation.pinned);
        let selector = Box::leak(reminders.selector().into_boxed_str());
        assert!(visual.debug_bounds(selector).is_some());
        assert!(shell.read_with(visual, |shell, _| {
            shell.extensions.as_ref().unwrap().panel.is_none()
        }));
        assert!(shell.read_with(visual, |shell, cx| {
            shell
                .extension_entries(cx)
                .iter()
                .all(|entry| !entry.package.name.starts_with("sailry.builtin."))
        }));
        let tasks = shell.read_with(visual, |shell, cx| {
            shell
                .extension_entries(cx)
                .into_iter()
                .find(|entry| entry.package.name == "scheduled-tasks")
                .unwrap()
        });
        assert_eq!(tasks.node, fixture.node.id());
        assert_eq!(tasks.scope, Scope::Host);
        let tasks_selector = Box::leak(tasks.selector().into_boxed_str());
        click(visual, tasks_selector);
        wait(visual, |cx| {
            shell
                .read(cx)
                .extensions
                .as_ref()
                .and_then(|state| state.panel.as_ref())
                .is_some_and(|panel| snapshot(panel, cx).contains("scheduled-tasks-page"))
        });
        assert!(visual.debug_bounds("activity-host-filter").is_none());
        assert_eq!(
            shell.read_with(visual, |shell, cx| shell
                .extensions
                .as_ref()
                .unwrap()
                .panel
                .as_ref()
                .unwrap()
                .read(cx)
                .binding
                .client
                .target()),
            fixture.node.id()
        );
        click(visual, database_selector);
        click(visual, selector);
        wait(visual, |cx| {
            shell
                .read(cx)
                .extensions
                .as_ref()
                .and_then(|state| state.panel.as_ref())
                .is_some_and(|panel| {
                    snapshot(panel, cx).contains("Selected-node reminder")
                        && snapshot(panel, cx).contains("Plugin fixture")
                })
        });
        assert!(visual.debug_bounds("activity-host-filter").is_none());
        if remote {
            assert!(shell.read_with(visual, |shell, cx| {
                !snapshot(
                    shell.extensions.as_ref().unwrap().panel.as_ref().unwrap(),
                    cx,
                )
                .contains("Controller reminder")
            }));
        }
        let Output::Plugin(info) = execute_on(
            &fixture,
            &local,
            Command::ReadPlugin {
                name: "reminders".into(),
            },
        ) else {
            panic!("plugin expected")
        };
        let Output::Notification(notice) = execute_on(
            &fixture,
            &local,
            Command::PublishNotification {
                package: info.summary.reference(),
                content: sailry_protocol::notification::Draft {
                    title: "Reminder route".into(),
                    message: String::new(),
                    kind: sailry_protocol::notification::Kind::Info,
                    session: None,
                },
            },
        ) else {
            panic!("notice expected")
        };
        wait(visual, |cx| {
            shell
                .read(cx)
                .activity_snapshot(local.target())
                .is_some_and(|snapshot| {
                    snapshot
                        .notifications
                        .iter()
                        .any(|entry| entry.id == notice.id)
                })
        });
        click(visual, database_selector);
        visual.update(|window, cx| {
            shell.update(cx, |shell, cx| {
                shell.live.as_mut().unwrap().select(
                    if local.target() == fixture.node.id() {
                        fixture.controller.id()
                    } else {
                        fixture.node.id()
                    },
                    cx,
                );
                shell.open_activity(
                    local.target(),
                    sailry_client::activity::Target::Notification(notice.id),
                    window,
                    cx,
                );
            })
        });
        wait(visual, |cx| {
            shell.read(cx).page == Page::Plugin
                && shell
                    .read(cx)
                    .extensions
                    .as_ref()
                    .and_then(|state| state.selected.as_ref())
                    .is_some_and(|entry| entry.package.name == "reminders")
                && shell.read(cx).live.as_ref().unwrap().selected == local.target()
        });
        wait(visual, |cx| {
            shell
                .read(cx)
                .extensions
                .as_ref()
                .and_then(|state| state.panel.as_ref())
                .is_some_and(|panel| {
                    snapshot(panel, cx).contains(if remote {
                        "Controller reminder"
                    } else {
                        "Selected-node reminder"
                    })
                })
        });
        assert!(visual.debug_bounds("activity-host-filter").is_none());
        if remote {
            assert!(shell.read_with(visual, |shell, cx| {
                let panel = shell.extensions.as_ref().unwrap().panel.as_ref().unwrap();
                panel.read(cx).binding.client.target() == local.target()
                    && snapshot(panel, cx).contains("Controller project")
                    && !snapshot(panel, cx).contains("Selected-node reminder")
            }));
        }
        visual.update(|_, cx| {
            shell.update(cx, |shell, cx| {
                shell.live.as_mut().unwrap().select(fixture.node.id(), cx)
            })
        });
        wait(visual, |cx| {
            shell.read(cx).live.as_ref().unwrap().view.connected
                && shell
                    .read(cx)
                    .extensions
                    .as_ref()
                    .is_some_and(|state| state.metadata.read(cx).settled())
                && shell.read(cx).extension_entries(cx).iter().any(|entry| {
                    entry.package.name == "databases" && entry.node == fixture.node.id()
                })
        });
        click(visual, database_selector);
        wait(visual, |cx| {
            shell
                .read(cx)
                .extensions
                .as_ref()
                .and_then(|state| state.panel.as_ref())
                .is_some_and(|panel| snapshot(panel, cx).contains("db-landing"))
        });
        if remote {
            let Output::Plugin(info) = fixture.execute(Command::ReadPlugin {
                name: "reminders".into(),
            }) else {
                panic!("plugin expected")
            };
            let Output::Notification(notice) = fixture.execute(Command::PublishNotification {
                package: info.summary.reference(),
                content: sailry_protocol::notification::Draft {
                    title: "Remote reminder route".into(),
                    message: String::new(),
                    kind: sailry_protocol::notification::Kind::Info,
                    session: None,
                },
            }) else {
                panic!("notice expected")
            };
            wait(visual, |cx| {
                shell
                    .read(cx)
                    .activity_snapshot(fixture.node.id())
                    .is_some_and(|snapshot| {
                        snapshot
                            .notifications
                            .iter()
                            .any(|entry| entry.id == notice.id)
                    })
            });
            visual.update(|window, cx| {
                shell.update(cx, |shell, cx| {
                    shell.open_activity(
                        fixture.node.id(),
                        sailry_client::activity::Target::Notification(notice.id),
                        window,
                        cx,
                    )
                })
            });
            wait(visual, |cx| {
                shell
                    .read(cx)
                    .extensions
                    .as_ref()
                    .unwrap()
                    .metadata
                    .read(cx)
                    .settled()
            });
            assert!(
                shell.read_with(visual, |shell, cx| {
                    let state = shell.extensions.as_ref().unwrap();
                    state.selected.as_ref().is_some_and(|entry| {
                        entry.package.name == "reminders" && entry.node == fixture.node.id()
                    }) && state
                        .panel
                        .as_ref()
                        .unwrap()
                        .read(cx)
                        .binding
                        .client
                        .target()
                        == fixture.node.id()
                }),
                "a remote notice must open only the originating Node's reminder records"
            );
            wait(visual, |cx| {
                shell
                    .read(cx)
                    .extensions
                    .as_ref()
                    .and_then(|state| state.panel.as_ref())
                    .is_some_and(|panel| {
                        snapshot(panel, cx).contains("Selected-node reminder")
                            && snapshot(panel, cx).contains("Plugin fixture")
                            && !snapshot(panel, cx).contains("Controller reminder")
                    })
            });
            for enabled in [false, true] {
                for name in ["reminders", "scheduled-tasks"] {
                    set_name(&fixture, name, enabled);
                }
                wait(visual, |cx| {
                    shell
                        .read(cx)
                        .live
                        .as_ref()
                        .unwrap()
                        .view
                        .snapshot
                        .as_ref()
                        .is_some_and(|snapshot| {
                            ["reminders", "scheduled-tasks"].into_iter().all(|name| {
                                snapshot
                                    .plugins
                                    .iter()
                                    .any(|entry| entry.name == name && entry.enabled == enabled)
                            })
                        })
                        && shell
                            .read(cx)
                            .extensions
                            .as_ref()
                            .is_some_and(|state| state.metadata.read(cx).settled())
                });
                assert!(shell.read_with(visual, |shell, cx| {
                    let entries = shell.extension_entries(cx);
                    ["reminders", "scheduled-tasks"].into_iter().all(|name| {
                        entries.iter().any(|entry| {
                            entry.package.name == name && entry.node == fixture.node.id()
                        }) == enabled
                    })
                }));
            }
        }
        let panel = visual.update(|window, cx| {
            let source = cx.new(|cx| {
                crate::conversation::live::View::new(
                    fixture.binding.clone(),
                    Some(fixture.session.clone()),
                    window,
                    cx,
                )
            });
            cx.new(|cx| Panel::new(source, cx))
        });
        wait(visual, |cx| {
            panel
                .read(cx)
                .renderer_entry(ResourceKind::Git, cx)
                .is_some()
        });
        set_name(&fixture, "databases", false);
        set_name(&fixture, "git", false);
        set_on(&fixture, &local, "reminders", false);
        set_on(&fixture, &local, "scheduled-tasks", false);
        if remote {
            assert!(shell.read_with(visual, |shell, cx| {
                let entries = shell.extension_entries(cx);
                ["reminders", "scheduled-tasks"].into_iter().all(|name| {
                    entries
                        .iter()
                        .any(|entry| entry.package.name == name && entry.node == fixture.node.id())
                })
            }));
            for name in ["reminders", "scheduled-tasks"] {
                set_name(&fixture, name, false);
            }
        }
        wait(visual, |cx| {
            !shell
                .read(cx)
                .extension_entries(cx)
                .iter()
                .any(|entry| entry.package.name == "databases")
                && !shell
                    .read(cx)
                    .extension_entries(cx)
                    .iter()
                    .any(|entry| entry.package.name == "scheduled-tasks")
                && !shell
                    .read(cx)
                    .extension_entries(cx)
                    .iter()
                    .any(|entry| entry.package.name == "reminders")
                && !panel
                    .read(cx)
                    .renderer_entry(ResourceKind::Git, cx)
                    .is_some()
        });
        assert!(visual.debug_bounds(database_selector).is_none());
        assert!(visual.debug_bounds(activity_selector).is_some());
        assert!(visual.debug_bounds(tasks_selector).is_none());
        assert!(visual.debug_bounds(selector).is_none());
        assert!(visual.debug_bounds(ssh_selector).is_some());
        visual.update(|_, cx| {
            shell.update(cx, |shell, cx| {
                shell
                    .live
                    .as_mut()
                    .unwrap()
                    .select(fixture.controller.id(), cx)
            })
        });
        wait(visual, |cx| {
            shell
                .read(cx)
                .extension_entries(cx)
                .iter()
                .any(|entry| entry.package.name == "databases")
        });
        assert!(!panel.read_with(visual, |panel, cx| {
            panel.renderer_entry(ResourceKind::Git, cx).is_some()
        }));
        set_name(&fixture, "git", true);
        wait(visual, |cx| {
            panel
                .read(cx)
                .renderer_entry(ResourceKind::Git, cx)
                .is_some()
        });
        assert_eq!(
            panel.read_with(visual, |panel, _| panel.binding.client.target()),
            fixture.node.id()
        );
        visual.update(|window, _| window.remove_window());
        drop(panel);
        drop(shell);
        fixture.close();
    }
}

#[gpui::test]
fn blocks_disabled(cx: &mut TestAppContext) {
    init(cx);
    for remote in [false, true] {
        let fixture = Fixture::new(remote);
        cx.update(|cx| cx.set_global(fixture.services(remote)));
        let mut owner = None;
        let (_, visual) = cx.add_window_view(|window, cx| {
            let shell = cx.new(|cx| Shell::new(window, cx));
            owner = Some(shell.clone());
            Root::new(shell, window, cx)
        });
        let shell = owner.unwrap();
        wait(visual, |cx| {
            shell
                .read(cx)
                .live
                .as_ref()
                .unwrap()
                .hosts
                .contains_key(&fixture.node.id())
        });
        visual.update(|_, cx| {
            shell.update(cx, |shell, cx| {
                shell.live.as_mut().unwrap().select(fixture.node.id(), cx)
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
                .is_some()
        });
        visual.update(|window, cx| {
            shell.update(cx, |shell, cx| {
                shell.reveal_session(fixture.session.clone(), window, cx)
            })
        });
        wait(visual, |cx| {
            shell
                .read(cx)
                .current_chat()
                .is_some_and(|chat| chat.read(cx).connected())
        });
        set_name(&fixture, "files", false);
        set_name(&fixture, "git", false);
        wait(visual, |cx| {
            shell.read(cx).current_chat().is_some_and(|chat| {
                chat.read(cx).renderer_unavailable(ResourceKind::Git, cx)
                    && chat
                        .read(cx)
                        .renderer_unavailable(ResourceKind::Documents, cx)
            }) && shell
                .read(cx)
                .renderer_navigation(
                    sailry_protocol::plugin::desktop::ResourceKind::Documents,
                    cx,
                )
                .is_none()
        });
        visual.update(|_, cx| {
            shell.update(cx, |shell, cx| {
                shell
                    .live
                    .as_mut()
                    .unwrap()
                    .select(fixture.controller.id(), cx)
            })
        });
        wait(visual, |cx| {
            shell
                .read(cx)
                .renderer_navigation(ResourceKind::Documents, cx)
                .is_some()
        });
        visual.update(|window, cx| {
            shell.update(cx, |shell, cx| {
                assert_eq!(shell.page, Page::Conversation);
                shell.open_destination(crate::resources::launcher::Destination::Review, window, cx);
                assert!(shell.side_resource.is_none());
                shell.open_resource_panel(Page::Files, window, cx);
                assert!(shell.side_resource.is_none());
            })
        });
        for event in [
            crate::conversation::live::Event::FileAt(fixture.session.worktree, "notes.txt".into()),
            crate::conversation::live::Event::GitFile(
                fixture.session.worktree,
                Some("notes.txt".into()),
            ),
        ] {
            visual.update(|_, cx| {
                let chat = shell.read(cx).current_chat().unwrap().clone();
                chat.update(cx, |_, cx| cx.emit(event));
            });
            visual.run_until_parked();
            assert!(shell.read_with(visual, |shell, _| shell.side_resource.is_none()));
        }
        set_name(&fixture, "files", true);
        wait(visual, |cx| {
            shell.read(cx).current_chat().is_some_and(|chat| {
                chat.read(cx)
                    .renderer_available(ResourceKind::Documents, cx)
            })
        });
        visual.update(|window, cx| {
            shell.update(cx, |shell, cx| {
                shell.open_resource_panel(Page::Files, window, cx)
            })
        });
        wait(visual, |cx| {
            matches!(&shell.read(cx).side_resource, Some(crate::resources::SideResource::Plugin(panel))
            if panel.read(cx).resource_active()
                && panel.read(cx).binding.client.target() == fixture.node.id()
                && panel.read(cx).binding.worktree == Some(fixture.session.worktree))
        });
        visual.update(|window, _| window.remove_window());
        drop(shell);
        fixture.close();
    }
}

#[gpui::test]
fn preserves_disabled_panel(cx: &mut TestAppContext) {
    init(cx);
    for remote in [false, true] {
        let fixture = Fixture::new(remote);
        cx.update(|cx| cx.set_global(fixture.services(remote)));
        let mut owner = None;
        let (_, visual) = cx.add_window_view(|window, cx| {
            let shell = cx.new(|cx| Shell::new(window, cx));
            owner = Some(shell.clone());
            Root::new(shell, window, cx)
        });
        let shell = owner.unwrap();
        let handle = visual.update(|window, _| window.window_handle());
        visual.simulate_window_resize(handle, size(px(1280.), px(820.)));
        wait(visual, |cx| {
            shell
                .read(cx)
                .live
                .as_ref()
                .unwrap()
                .hosts
                .contains_key(&fixture.node.id())
        });
        visual.update(|_, cx| {
            shell.update(cx, |shell, cx| {
                shell.live.as_mut().unwrap().select(fixture.node.id(), cx)
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
                .is_some()
        });
        visual.update(|window, cx| {
            shell.update(cx, |shell, cx| {
                shell.reveal_session(fixture.session.clone(), window, cx)
            })
        });
        wait(visual, |cx| {
            shell
                .read(cx)
                .current_chat()
                .is_some_and(|chat| chat.read(cx).connected())
        });
        visual.update(|window, cx| {
            shell.update(cx, |shell, cx| {
                shell.open_documents(
                    (fixture.node.id(), fixture.session.worktree),
                    Some(("notes.txt".into(), None)),
                    window,
                    cx,
                );
            })
        });
        let panel = shell.read_with(visual, |shell, _| match &shell.side_resource {
            Some(crate::resources::SideResource::Plugin(panel)) => panel.clone(),
            _ => panic!("document renderer expected"),
        });
        let documents = panel.read_with(visual, |panel, _| panel.documents.clone().unwrap());
        wait(visual, |cx| {
            documents.read(cx).editor("notes.txt").is_some()
        });
        let editor = documents.read_with(visual, |documents, _| {
            documents.editor("notes.txt").unwrap()
        });
        visual.update(|window, cx| {
            editor.update(cx, |editor, cx| {
                editor.set_value("Unsaved draft\n", window, cx);
                editor.focus(window, cx);
            })
        });
        wait(visual, |cx| documents.read(cx).has_unsaved(cx));
        #[cfg(target_os = "macos")]
        {
            visual.dispatch_action(crate::app_menu::CloseWindow);
            visual.run_until_parked();
            assert!(visual.has_pending_prompt());
            visual.simulate_prompt_answer(&crate::tr("settings_cancel"));
            visual.run_until_parked();
            assert!(!visual.has_pending_prompt());
            assert!(visual.update(|window, cx| cx.windows().contains(&window.window_handle())));
            assert_eq!(
                editor.read_with(visual, |editor, _| editor.value().to_string()),
                "Unsaved draft\n"
            );
        }
        set_name(&fixture, "files", false);
        wait(visual, |cx| {
            panel.read(cx).mounted.is_none() && !panel.read(cx).resource_active()
        });
        assert!(!panel.read_with(visual, |panel, _| panel.resource_active()));
        assert!(visual.debug_bounds("files_save").is_none());
        visual.simulate_keystrokes("secondary-s");
        visual.run_until_parked();
        assert_eq!(
            std::fs::read_to_string(fixture.directory.path().join("project/notes.txt")).unwrap(),
            "Complete 中文 🙂\n"
        );
        assert_eq!(
            editor.read_with(visual, |editor, _| editor.value().to_string()),
            "Unsaved draft\n"
        );
        assert!(documents.read_with(visual, |documents, cx| documents.has_unsaved(cx)));
        set_name(&fixture, "files", true);
        wait(visual, |cx| {
            panel.read(cx).resource_active() && snapshot(&panel, cx).contains("file-explorer")
        });
        assert!(panel.read_with(visual, |panel, _| panel.resource_active()));
        assert_eq!(
            documents.read_with(visual, |documents, _| documents
                .editor("notes.txt")
                .unwrap()
                .entity_id()),
            editor.entity_id()
        );
        click(visual, "files_save");
        wait(visual, |cx| !documents.read(cx).has_unsaved(cx));
        assert_eq!(
            std::fs::read_to_string(fixture.directory.path().join("project/notes.txt")).unwrap(),
            "Unsaved draft\n"
        );
        visual.update(|window, _| window.remove_window());
        drop(shell);
        fixture.close();
    }
}
