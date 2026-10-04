use super::*;
use crate::{plugins::fixture::Fixture, settings::Section};
use core::prelude::v1::test;
use sailry_protocol::{Command as Request, Output, plugin::Scope as PluginScope};
use std::time::{Duration, Instant};

#[track_caller]
fn wait(cx: &mut VisualTestContext, ready: impl Fn(&App) -> bool) {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        cx.run_until_parked();
        if cx.update(|window, cx| {
            window.draw(cx).clear(cx);
            ready(cx)
        }) {
            return;
        }
        assert!(Instant::now() < deadline, "command inventory deadline");
        std::thread::sleep(Duration::from_millis(10));
    }
}

fn mount<'a>(
    cx: &'a mut TestAppContext,
    fixture: &Fixture,
    remote: bool,
) -> (Entity<Shell>, &'a mut VisualTestContext) {
    cx.executor().allow_parking();
    cx.update(|cx| {
        gpui_kit::init(cx);
        crate::theme::init(cx);
        crate::shell::init(cx);
        cx.set_global(fixture.services(remote));
    });
    let mut entity = None;
    let (_, visual) = cx.add_window_view(|window, cx| {
        let shell = cx.new(|cx| Shell::new(window, cx));
        entity = Some(shell.clone());
        Root::new(shell, window, cx)
    });
    let shell = entity.unwrap();
    wait(visual, |cx| {
        shell.read(cx).live.as_ref().unwrap().hosts.len() == 2
    });
    visual.update(|_, cx| {
        shell.update(cx, |shell, cx| {
            shell.live.as_mut().unwrap().select(fixture.node.id(), cx)
        });
    });
    wait(visual, |cx| {
        let live = shell.read(cx).live.as_ref().unwrap();
        live.view.connected
            && live
                .view
                .snapshot
                .as_ref()
                .is_some_and(|snapshot| snapshot.node == fixture.node.id())
    });
    (shell, visual)
}

fn enter(cx: &mut VisualTestContext, key: &str) {
    cx.simulate_input(&tr(key));
    confirm(cx);
}

fn confirm(cx: &mut VisualTestContext) {
    // Native List prepares its searchable row cache during rendering.
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear(cx));
    cx.simulate_keystrokes("enter");
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear(cx));
}

fn root_rows(visual: &mut VisualTestContext) -> usize {
    [
        "global-command-item-0-0",
        "global-command-item-0-1",
        "global-command-item-0-2",
        "global-command-item-0-3",
        "global-command-item-0-4",
        "global-command-item-0-5",
    ]
    .into_iter()
    .filter(|selector| visual.debug_bounds(selector).is_some())
    .count()
}

fn root_count(shell: &Entity<Shell>, cx: &VisualTestContext) -> usize {
    shell.read_with(cx, |shell, cx| {
        targets(shell, Scope::capture(shell), Folder::Root, cx).len()
    })
}

fn targets(shell: &Shell, scope: Scope, folder: Folder, cx: &App) -> Vec<Target> {
    catalog::entries(shell, scope, folder, cx)
        .into_iter()
        .flat_map(|group| group.choices)
        .map(|choice| choice.target)
        .collect()
}

mod hierarchy {
    use super::*;

    #[gpui::test]
    fn root_and_escape(cx: &mut TestAppContext) {
        let (shell, mut visual) = crate::shell::tests::setup(cx);
        visual.simulate_keystrokes("secondary-k");
        visual.run_until_parked();
        visual.update(|window, cx| {
            window.draw(cx).clear(cx);
            let entries = targets(shell.read(cx), Scope::Preview(0), Folder::Root, cx);
            assert_eq!(
                entries
                    .iter()
                    .filter(|target| matches!(target, Target::Folder(_)))
                    .count(),
                4
            );
            assert!(
                !entries
                    .iter()
                    .any(|target| matches!(target, Target::Folder(Folder::Root)))
            );
            assert!(
                entries
                    .iter()
                    .skip(4)
                    .all(|target| matches!(target, Target::Feature(_)))
            );
        });
        assert_eq!(root_rows(&mut visual), root_count(&shell, &visual).min(6));
        visual.simulate_input(" ");
        visual.simulate_keystrokes("escape");
        visual.run_until_parked();
        assert!(visual.update(|window, cx| window.has_active_dialog(cx)));
        assert!(
            visual.update(|window, cx| { window.focused_input(cx).unwrap().value(cx).is_empty() })
        );
        enter(&mut visual, "shell_sessions");
        assert!(visual.debug_bounds("command-back").is_none());
        visual.simulate_input("no matching session");
        visual.simulate_keystrokes("escape");
        visual.run_until_parked();
        assert!(visual.update(|window, cx| window.has_active_dialog(cx)));
        assert!(visual.debug_bounds("command-back").is_none());
        assert!(
            visual.update(|window, cx| { window.focused_input(cx).unwrap().value(cx).is_empty() })
        );
        visual.update(|window, cx| window.draw(cx).clear(cx));
        assert!(visual.debug_bounds("global-command-item-0-0").is_some());
        visual.simulate_keystrokes("escape");
        visual.run_until_parked();
        visual.update(|window, cx| window.draw(cx).clear(cx));
        assert!(visual.update(|window, cx| window.has_active_dialog(cx)));
        assert!(visual.debug_bounds("command-back").is_none());
        assert_eq!(root_rows(&mut visual), root_count(&shell, &visual).min(6));
        visual.simulate_keystrokes("escape");
        visual.run_until_parked();
        assert!(!visual.update(|window, cx| window.has_active_dialog(cx)));
    }

    #[gpui::test]
    fn native_list_surface_has_only_row_insets(cx: &mut TestAppContext) {
        use std::{cell::Cell, rc::Rc};

        let (shell, mut visual) = crate::shell::tests::setup(cx);
        let surfaces = Rc::new(Cell::new(0));
        visual.update({
            let surfaces = surfaces.clone();
            move |_, cx| {
                gpui_kit::component::surface::set_renderer(
                    move |child, corners, cx| {
                        surfaces.set(surfaces.get() + 1);
                        assert_eq!(
                            corners,
                            Corners::all(cx.theme().surface_radius())
                                .map(|radius| (*radius).into()),
                        );
                        div()
                            .debug_selector(|| "global-command-surface".into())
                            .child(child)
                            .into_any_element()
                    },
                    cx,
                );
            }
        });
        visual.simulate_keystrokes("secondary-k");
        visual.run_until_parked();
        visual
            .executor()
            .advance_clock(*gpui_kit::component::dialog::ANIMATION_DURATION);
        visual.run_until_parked();
        for viewport in [size(px(1280.), px(820.)), size(px(760.), px(600.))] {
            visual.simulate_resize(viewport);
            visual.run_until_parked();
            surfaces.set(0);
            visual.update(|window, cx| {
                window.refresh();
                window.draw(cx).clear(cx);
            });
            assert_eq!(
                surfaces.get(),
                1,
                "only the registered dialog owns material"
            );
            let surface = visual.debug_bounds("global-command-surface").unwrap();
            let content = visual.debug_bounds("global-command").unwrap();
            let search = visual.debug_bounds("list-search").unwrap();
            let row = visual.debug_bounds("global-command-item-0-0").unwrap();
            assert_eq!(surface.size.width, px(480.));
            assert_eq!(content.left() - surface.left(), px(1.));
            assert_eq!(surface.right() - content.right(), px(1.));
            assert_eq!(search.left(), content.left());
            assert_eq!(search.right(), content.right());
            assert_eq!(search.top(), content.top());
            assert_eq!(row.left() - content.left(), px(8.));
            assert_eq!(content.right() - row.right(), px(8.));
            assert_eq!(row.top() - search.bottom(), px(8.));
            assert_eq!(row.size.height, px(crate::command_picker::ROW_HEIGHT));
            assert_eq!(
                content.size.height,
                visual.update(|window, cx| {
                    let shell = shell.read(cx);
                    crate::command_picker::height(
                        targets(shell, Scope::capture(shell), Folder::Root, cx).len(),
                        0,
                        window,
                    )
                }),
            );
            assert!(visual.debug_bounds("command-search").is_none());
        }
        enter(&mut visual, "settings");
        let content = visual.debug_bounds("global-command").unwrap();
        assert!(visual.debug_bounds("command-back").is_none());
        let search = visual.debug_bounds("list-search").unwrap();
        assert!(content.size.height <= px(600.) * 0.6);
        assert_eq!(search.top(), content.top());
        assert_eq!(search.left(), content.left());
        assert_eq!(search.right(), content.right());
        visual.simulate_keystrokes("escape escape");
        visual.run_until_parked();
        assert!(!visual.update(|window, cx| window.has_active_dialog(cx)));
    }

    #[gpui::test]
    fn preview_resources_use_their_owner(cx: &mut TestAppContext) {
        let (shell, mut visual) = crate::shell::tests::setup(cx);
        visual.update(|window, cx| {
            shell.update(cx, |shell, cx| {
                let scope = Scope::Preview(0);
                let sessions = targets(shell, scope, Folder::Sessions, cx);
                let expected: std::collections::BTreeSet<_> = shell
                    .workspace
                    .sessions
                    .iter()
                    .filter(|(_, session)| {
                        session.owner.host == 0
                            && !session.archived
                            && shell.workspace.contains(session.owner)
                    })
                    .map(|(key, _)| *key)
                    .collect();
                let actual: std::collections::BTreeSet<_> = sessions
                    .iter()
                    .map(|target| {
                        let Target::PreviewSession(key) = target else {
                            panic!("preview session expected")
                        };
                        *key
                    })
                    .collect();
                assert_eq!(actual, expected);
                assert!(actual.contains(&(0, 0)) && actual.contains(&(0, 1)));
                let visible_count = sessions.len();
                assert!(
                    sessions
                        .iter()
                        .all(|target| matches!(target, Target::PreviewSession((0, _))))
                );
                let projects = targets(shell, scope, Folder::Projects, cx);
                assert_eq!(projects.len(), 1);
                catalog::open(
                    shell,
                    scope,
                    Folder::Projects,
                    projects[0].clone(),
                    window,
                    cx,
                );
                assert_eq!(shell.page, Page::Project);
                catalog::open(
                    shell,
                    scope,
                    Folder::Sessions,
                    Target::PreviewSession((0, 1)),
                    window,
                    cx,
                );
                assert_eq!(shell.session, 1);
                assert_eq!(
                    shell.session_scope.active,
                    session_scope::Key::Preview(0, 1)
                );
                let features = targets(shell, scope, Folder::Root, cx);
                assert!(features.iter().all(|target| !matches!(
                    target,
                    Target::Feature(feature) if matches!(feature.destination, rail::Destination::Page(
                            Page::Conversation
                                | Page::Activity
                                | Page::Host
                                | Page::Project
                                | Page::Settings
                                | Page::Terminal
                        ))
                )));
                shell.workspace.sessions.get_mut(&(0, 0)).unwrap().archived = true;
                assert_eq!(
                    targets(shell, scope, Folder::Sessions, cx).len(),
                    visible_count - 1
                );
                shell.select_host(1, window, cx);
                assert!(!catalog::available(
                    shell,
                    scope,
                    Folder::Sessions,
                    &Target::PreviewSession((0, 1)),
                    cx
                ));
            });
        });
    }

    #[gpui::test]
    fn settings_confirm_uses_original_group_coordinates(cx: &mut TestAppContext) {
        let (shell, mut visual) = crate::shell::tests::setup(cx);
        visual.simulate_keystrokes("secondary-k");
        enter(&mut visual, "settings");
        visual.simulate_input(&tr("settings_providers"));
        confirm(&mut visual);
        assert_eq!(
            shell.read_with(&visual, |shell, _| shell.page),
            Page::Settings
        );
        assert_eq!(
            shell.read_with(&visual, |shell, cx| shell.settings.read(cx).section),
            Section::Providers
        );
        assert!(!visual.update(|window, cx| window.has_active_dialog(cx)));
    }
}

mod resources {
    use super::*;

    #[gpui::test]
    fn local_and_remote_navigation(cx: &mut TestAppContext) {
        for remote in [false, true] {
            let fixture = Fixture::new(remote);
            let Output::Session(unassigned) = fixture.execute(Request::CreateSession {
                project: None,
                worktree: None,
                config: Some(fixture.session.config.clone()),
            }) else {
                panic!("session expected")
            };
            let Output::Session(unassigned) = fixture.execute(Request::RenameSession {
                session: unassigned.id,
                expected_revision: unassigned.revision,
                title: "Command unassigned".into(),
            }) else {
                panic!("session expected")
            };
            let Output::Session(archived) = fixture.execute(Request::CreateSession {
                project: fixture.session.project,
                worktree: Some(fixture.session.worktree),
                config: Some(fixture.session.config.clone()),
            }) else {
                panic!("session expected")
            };
            fixture.execute(Request::SetSessionArchived {
                session: archived.id,
                expected_revision: archived.revision,
                archived: true,
            });
            let (shell, visual) = mount(cx, &fixture, remote);
            wait(visual, |cx| {
                shell
                    .read(cx)
                    .live
                    .as_ref()
                    .unwrap()
                    .view
                    .snapshot
                    .as_ref()
                    .unwrap()
                    .sessions
                    .len()
                    == 3
            });
            wait(visual, |cx| {
                shell.read(cx).extension_entries(cx).iter().any(|entry| {
                    entry.package.name == "progress"
                        && shell.read(cx).command_navigation_available(entry, cx)
                }) && shell
                    .read(cx)
                    .extension_entries(cx)
                    .iter()
                    .any(|entry| entry.package.name == "files")
            });
            let scope = Scope::Node(fixture.node.id());
            shell.read_with(visual, |shell, cx| {
                let entry = shell
                    .extension_entries(cx)
                    .into_iter()
                    .find(|entry| entry.package.name == "files")
                    .unwrap();
                assert_eq!(
                    entry.navigation.target,
                    sailry_protocol::plugin::desktop::NavigationTarget::Worktree
                );
                assert!(entry.worktree.is_none());
                assert!(!shell.command_navigation_available(&entry, cx));
                assert!(!catalog::available(
                    shell,
                    scope,
                    Folder::Root,
                    &Target::Feature(Box::new(rail::Feature::from(entry))),
                    cx
                ));
            });
            visual.simulate_keystrokes("secondary-k");
            enter(visual, "projects");
            confirm(visual);
            assert_eq!(
                shell.read_with(visual, |shell, _| shell.page),
                Page::Project
            );
            assert_eq!(
                shell.read_with(visual, |shell, _| shell.live.as_ref().unwrap().project),
                fixture.session.project
            );
            visual.simulate_keystrokes("secondary-k");
            enter(visual, "shell_sessions");
            visual.simulate_input("Command unassigned");
            confirm(visual);
            assert!(!visual.update(|window, cx| window.has_active_dialog(cx)));
            visual.update(|_, cx| {
                shell.update(cx, |shell, cx| {
                    let sessions = targets(shell, scope, Folder::Sessions, cx);
                    assert_eq!(sessions.len(), 2);
                    assert!(sessions.iter().any(|target| matches!(target, Target::Session(id) if *id == unassigned.id)));
                    assert!(!sessions.iter().any(|target| matches!(target, Target::Session(id) if *id == archived.id)));
                    assert!(sessions.iter().all(|target| matches!(target, Target::Session(_))));
                    assert!(targets(shell, scope, Folder::Root, cx).iter().any(|target| matches!(target, Target::Feature(feature) if matches!(&feature.destination, rail::Destination::Plugin(entry) if entry.package.name == "progress"))));
                    let projects = targets(shell, scope, Folder::Projects, cx);
                    assert_eq!(projects.len(), 1);
                    assert_eq!(shell.session_scope.active, session_scope::Key::Session(fixture.node.id(), unassigned.id));
                    assert!(shell.live.as_ref().unwrap().project.is_none());
                    assert_eq!(shell.current_chat().unwrap().read(cx).session(), Some(unassigned.id));
                    crate::preferences::recent::record(fixture.node.id(), fixture.session.id, cx);
                    assert!(matches!(targets(shell, scope, Folder::Sessions, cx).first(), Some(Target::Session(id)) if *id == fixture.session.id));
                });
            });
            fixture.execute(Request::RemoveSession {
                session: unassigned.id,
                expected_revision: unassigned.revision,
            });
            wait(visual, |cx| {
                !shell
                    .read(cx)
                    .live
                    .as_ref()
                    .unwrap()
                    .view
                    .snapshot
                    .as_ref()
                    .unwrap()
                    .sessions
                    .iter()
                    .any(|session| session.id == unassigned.id)
            });
            visual.update(|window, cx| {
                shell.update(cx, |shell, cx| {
                    assert!(!catalog::available(
                        shell,
                        scope,
                        Folder::Sessions,
                        &Target::Session(unassigned.id),
                        cx
                    ));
                    shell
                        .live
                        .as_mut()
                        .unwrap()
                        .select(fixture.controller.id(), cx);
                    assert!(!catalog::available(
                        shell,
                        scope,
                        Folder::Sessions,
                        &Target::Session(fixture.session.id),
                        cx
                    ));
                    assert!(!catalog::available(
                        shell,
                        scope,
                        Folder::Hosts,
                        &Target::Host(fixture.node.id()),
                        cx
                    ));
                    assert!(targets(shell, scope, Folder::Sessions, cx).is_empty());
                    catalog::open(
                        shell,
                        scope,
                        Folder::Hosts,
                        Target::Host(fixture.node.id()),
                        window,
                        cx,
                    );
                    assert_eq!(
                        shell.live.as_ref().unwrap().selected,
                        fixture.controller.id()
                    );
                });
                window.remove_window();
            });
            let Output::Snapshot(snapshot) = fixture.execute(Request::Snapshot) else {
                panic!("snapshot expected")
            };
            assert!(snapshot.turns.is_empty());
            assert!(
                snapshot
                    .sessions
                    .iter()
                    .all(|session| session.activity.run.is_none())
            );
            drop(shell);
            fixture.close();
        }
    }
}

mod settings {
    use super::*;

    #[gpui::test]
    fn desktop_contributions_survive_host_selection(cx: &mut TestAppContext) {
        let fixture = Fixture::new(false);
        fixture.package();
        let path = fixture.directory.path().join("project/package/plugin.json");
        let mut manifest: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        let extension = &mut manifest["extensions"]["dev.sailry.platform"];
        extension["scope"] = "desktop".into();
        extension["settings_page"]["placement"] =
            serde_json::json!({ "group": "app", "order": 150 });
        extension["desktop"]["navigation"] = serde_json::json!({
            "label": "Command workspace",
            "icon": "reicon:files/file-text"
        });
        extension["desktop"]["navigation_options"] = serde_json::json!({
            "target": "node"
        });
        std::fs::write(&path, manifest.to_string()).unwrap();
        let package = fixture.install(0);
        assert!(package.issues.is_empty(), "{:?}", package.issues);
        assert_eq!(
            package.extension.as_ref().unwrap().scope,
            PluginScope::Desktop
        );
        let (shell, visual) = mount(cx, &fixture, false);
        wait(visual, |cx| {
            let entries = shell.read(cx).command_settings_entries(cx);
            entries
                .iter()
                .any(|entry| entry.package == package.summary.reference())
                && entries
                    .iter()
                    .any(|entry| entry.node == fixture.node.id() && entry.package.name == "browser")
        });
        let desktop = shell.read_with(visual, |shell, cx| {
            shell
                .command_settings_entries(cx)
                .into_iter()
                .find(|entry| entry.package == package.summary.reference())
                .unwrap()
        });
        let host = shell.read_with(visual, |shell, cx| {
            shell
                .command_settings_entries(cx)
                .into_iter()
                .find(|entry| entry.node == fixture.node.id() && entry.package.name == "browser")
                .unwrap()
        });
        let feature = shell.read_with(visual, |shell, cx| {
            let entry = shell
                .extension_entries(cx)
                .into_iter()
                .find(|entry| entry.package == package.summary.reference())
                .unwrap();
            assert!(shell.command_navigation_available(&entry, cx));
            Target::Feature(Box::new(rail::Feature::from(entry)))
        });
        visual.update(|_, cx| {
            shell.update(cx, |shell, cx| {
                shell
                    .live
                    .as_mut()
                    .unwrap()
                    .select(fixture.controller.id(), cx);
                let entries = shell.command_settings_entries(cx);
                assert!(entries.contains(&desktop));
                assert!(!entries.contains(&host));
            });
        });
        wait(visual, |cx| {
            shell.read(cx).live.as_ref().unwrap().view.connected
                && shell
                    .read(cx)
                    .command_settings_entries(cx)
                    .iter()
                    .any(|entry| entry.package == package.summary.reference())
        });
        visual.update(|window, cx| {
            shell.update(cx, |shell, cx| {
                let scope = Scope::Node(fixture.controller.id());
                assert!(catalog::available(shell, scope, Folder::Root, &feature, cx));
                let groups = catalog::entries(shell, scope, Folder::Settings, cx);
                assert_eq!(groups.len(), Section::GROUPS.len());
                assert!(matches!(groups[0].choices[0].target, Target::Setting(Section::General)));
                assert!(matches!(&groups[0].choices[1].target, Target::PluginSetting(entry) if entry == &desktop));
                assert!(matches!(groups[0].choices[2].target, Target::Setting(Section::Appearance)));
                assert!(!catalog::available(shell, scope, Folder::Settings, &Target::PluginSetting(host.clone()), cx));
                catalog::open(shell, scope, Folder::Settings, Target::PluginSetting(desktop.clone()), window, cx);
                assert_eq!(shell.settings_target, Some(fixture.node.id()));
                assert_eq!(shell.settings.read(cx).selected_plugin_settings(), Some(package.summary.name.as_str()));
                assert_eq!(shell.live.as_ref().unwrap().selected, fixture.controller.id());
            });
        });
        fixture.execute(Request::SetPluginEnabled {
            name: package.summary.name.clone(),
            expected_revision: package.summary.revision,
            enabled: false,
        });
        wait(visual, |cx| {
            let shell = shell.read(cx);
            shell
                .command_settings_entries(cx)
                .iter()
                .any(|entry| entry.package.name == package.summary.name)
                && !shell
                    .extension_entries(cx)
                    .iter()
                    .any(|entry| entry.package.name == package.summary.name)
        });
        visual.update(|window, cx| {
            shell.update(cx, |shell, cx| {
                assert!(catalog::available(
                    shell,
                    Scope::Node(fixture.controller.id()),
                    Folder::Settings,
                    &Target::PluginSetting(desktop.clone()),
                    cx
                ));
                assert!(!catalog::available(
                    shell,
                    Scope::Node(fixture.controller.id()),
                    Folder::Root,
                    &feature,
                    cx
                ));
            });
            window.remove_window();
        });
        drop(shell);
        fixture.close();
    }
}
