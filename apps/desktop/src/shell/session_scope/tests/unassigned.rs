use super::*;

#[gpui::test]
fn navigation_and_resources(cx: &mut TestAppContext) {
    let fixture = Fixture::new();
    let (shell, visual) = mount(cx, &fixture);
    let handle = visual.update(|window, _| window.window_handle());
    visual.simulate_window_resize(handle, size(px(1280.), px(820.)));
    for index in 0..2 {
        let transport = if index == 0 {
            fixture.nodes[0].local()
        } else {
            fixture.nodes[0]
                .link()
                .remote(fixture.nodes[1].link().address())
        };
        let client = Client::new(transport);
        let Output::Session(session) = fixture
            .runtime
            .block_on(client.execute(client.prepare(Command::CreateSession {
                project: None,
                worktree: None,
                config: Some(fixture.sessions[index].config.clone()),
            })))
            .unwrap()
        else {
            panic!("session expected")
        };
        open(&shell, visual, &fixture, index, &session);
        wait(visual, |cx| {
            shell
                .read(cx)
                .live
                .as_ref()
                .unwrap()
                .selected_worktree()
                .is_some_and(|tree| tree.id == session.worktree)
        });
        visual.update(|window, cx| window.draw(cx).clear(cx));
        let row = format!("live-session-{}", session.id);
        let group = visual.debug_bounds("unassigned-sessions").unwrap();
        let projects = visual.debug_bounds("projects-heading").unwrap();
        let active = visual.debug_bounds("active-toggle").unwrap();
        assert_eq!(group.size.height, projects.size.height);
        assert_eq!(group.left(), projects.left());
        assert_eq!(group.left(), active.left());
        let session_row = visual
            .debug_bounds(Box::leak(row.clone().into_boxed_str()))
            .unwrap();
        assert!(session_row.top() - group.bottom() >= px(1.));
        for height in [560., 820.] {
            visual.simulate_window_resize(handle, size(px(1280.), px(height)));
            visual.run_until_parked();
            visual.update(|window, cx| window.draw(cx).clear(cx));
            let active = visual.debug_bounds("sidebar-active").unwrap();
            let group = visual.debug_bounds("unassigned-sessions").unwrap();
            let unassigned = visual.debug_bounds("sidebar-unassigned-viewport").unwrap();
            let projects = visual.debug_bounds("projects-heading").unwrap();
            let resource = visual
                .debug_bounds(Box::leak(row.clone().into_boxed_str()))
                .unwrap();
            assert!(active.bottom() <= group.top());
            assert!(group.top() - active.bottom() <= px(8.));
            assert!(resource.top() >= group.bottom());
            assert!(resource.bottom() <= unassigned.bottom());
            assert!(unassigned.bottom() - resource.bottom() <= px(4.));
            assert!(unassigned.bottom() <= projects.top());
            assert!(projects.top() - unassigned.bottom() <= px(8.));
            assert_eq!(group.left(), projects.left());
            assert!(
                (unassigned.size.height - resource.size.height).abs() <= px(4.),
                "one unassigned row must not reserve unused sidebar height"
            );
        }
        let project_row = Box::leak(
            format!("live-project-{}", fixture.sessions[index].project.unwrap()).into_boxed_str(),
        );
        let assigned_row =
            Box::leak(format!("live-session-{}", fixture.sessions[index].id).into_boxed_str());
        assert!(
            visual.debug_bounds(assigned_row).unwrap().top()
                - visual.debug_bounds(project_row).unwrap().bottom()
                >= px(1.)
        );

        // Unassigned remains a peer group when Projects is collapsed.
        click(&shell, visual, "projects-toggle".into());
        visual.update(|window, cx| window.draw(cx).clear(cx));
        assert!(visual.debug_bounds(project_row).is_none());
        assert!(visual.debug_bounds("sidebar-unassigned-viewport").is_some());
        assert!(
            (visual
                .debug_bounds("sidebar-unassigned-viewport")
                .unwrap()
                .size
                .height
                - session_row.size.height)
                .abs()
                <= px(4.),
            "collapsing Projects must not expand one unassigned row"
        );
        assert!(
            visual
                .debug_bounds(Box::leak(row.clone().into_boxed_str()))
                .is_some()
        );
        click(&shell, visual, "projects-toggle".into());
        visual.update(|window, cx| window.draw(cx).clear(cx));
        assert!(visual.debug_bounds("unassigned-sessions").is_some());
        assert!(
            visual
                .debug_bounds(Box::leak(row.clone().into_boxed_str()))
                .is_some()
        );
        assert!(
            visual
                .debug_bounds(Box::leak(
                    format!("live-session-{}-worktree", session.id).into_boxed_str()
                ))
                .is_none()
        );
        click(&shell, visual, "unassigned-sessions".into());
        visual.update(|window, cx| window.draw(cx).clear(cx));
        assert!(
            visual
                .debug_bounds(Box::leak(row.clone().into_boxed_str()))
                .is_none()
        );
        assert!(visual.debug_bounds(project_row).is_some());
        assert!(visual.debug_bounds(assigned_row).is_some());
        click(&shell, visual, "unassigned-sessions".into());
        click(&shell, visual, row);
        wait(visual, |cx| {
            shell.read(cx).features(cx).iter().any(|feature| {
                matches!(&feature.destination, crate::shell::rail::Destination::Plugin(entry)
                    if entry.package.name == "progress" && entry.node == fixture.nodes[index].id())
            })
        });
        let handle = visual.update(|window, _| window.window_handle());
        visual.simulate_window_resize(handle, size(px(1440.), px(820.)));
        visual.update(|window, cx| {
            shell.update(cx, |shell, cx| {
                let feature = shell
                    .features(cx)
                    .into_iter()
                    .find(|feature| {
                        matches!(&feature.destination, crate::shell::rail::Destination::Plugin(entry)
                            if entry.package.name == "progress" && entry.node == fixture.nodes[index].id())
                    })
                    .unwrap();
                feature.open(shell, window, cx);
            })
        });
        let card = format!("activity-{}", session.id);
        wait(visual, |cx| {
            let shell = shell.read(cx);
            shell.page == crate::preview::Page::Plugin
                && shell.extensions.as_ref().is_some_and(|state| {
                    state.selected.as_ref().is_some_and(|entry| {
                        entry.package.name == "progress" && entry.node == fixture.nodes[index].id()
                    }) && state
                        .panel
                        .as_ref()
                        .is_some_and(|panel| crate::plugins::diagnostics(panel, cx).contains(&card))
                })
        });
        assert!(
            visual
                .debug_bounds(Box::leak(card.clone().into_boxed_str()))
                .is_some()
        );
        assert!(
            visual
                .debug_bounds(Box::leak(format!("{card}-worktree-icon").into_boxed_str()))
                .is_none()
        );
        assert!(shell.read_with(visual, |shell, cx| {
            let panel = shell.extensions.as_ref().unwrap().panel.as_ref().unwrap();
            !crate::plugins::diagnostics(panel, cx).contains(&format!("{card}-worktree-icon"))
        }));
        click(&shell, visual, card);
        assert_eq!(
            shell.read_with(visual, |shell, _| shell.session_scope.active),
            Key::Session(fixture.nodes[index].id(), session.id)
        );
        let mut navigation_panels = std::collections::BTreeMap::new();
        for _ in 0..2 {
            for name in ["files", "git"] {
                visual.update(|window, cx| {
                    shell.update(cx, |shell, cx| {
                        let feature = shell
                            .features(cx)
                            .into_iter()
                            .find(|feature| {
                                matches!(&feature.destination, crate::shell::rail::Destination::Plugin(entry)
                                    if entry.package.name == name && entry.node == fixture.nodes[index].id())
                            })
                            .unwrap();
                        feature.open(shell, window, cx);
                    });
                });
                wait(visual, |cx| {
                    let shell = shell.read(cx);
                    shell.page == crate::preview::Page::Plugin
                        && !shell.needs_project()
                        && shell.extensions.as_ref().is_some_and(|state| {
                            state.selected.as_ref().is_some_and(|entry| {
                                entry.package.name == name
                                    && entry.node == fixture.nodes[index].id()
                                    && entry.worktree == Some(session.worktree)
                            }) && state.panel.as_ref().is_some_and(|panel| {
                                panel.read(cx).resource_active()
                                    && panel.read(cx).document_scope(cx)
                                        == Some((fixture.nodes[index].id(), session.worktree))
                                    && crate::plugins::diagnostics(panel, cx).contains(
                                        if name == "git" {
                                            "git_directory_title"
                                        } else {
                                            "file-explorer"
                                        },
                                    )
                            })
                        })
                });
                let panel = shell.read_with(visual, |shell, _| {
                    shell.extensions.as_ref().unwrap().panel.clone().unwrap()
                });
                if let Some(previous) = navigation_panels.insert(name, panel.entity_id()) {
                    assert_eq!(panel.entity_id(), previous);
                }
                if name == "git" {
                    assert!(visual.debug_bounds("git-initialize").is_some());
                }
                visual.update(|window, cx| {
                    assert!(window.notifications(cx).is_empty());
                    for key in [
                        "git_read_failed",
                        "files_read_failed",
                        "plugins_view_load_failed",
                    ] {
                        assert_eq!(
                            crate::feedback::tests::count(window, &crate::tr(key), cx),
                            0
                        );
                    }
                });
            }
            click(&shell, visual, "navigation-conversation".into());
            assert!(shell.read_with(visual, |shell, cx| {
                shell.page == crate::preview::Page::Conversation
                    && shell.session_scope.active
                        == Key::Session(fixture.nodes[index].id(), session.id)
                    && shell.current_chat().is_some_and(|chat| {
                        chat.read(cx).binding().worktree == Some(session.worktree)
                    })
                    && shell
                        .live
                        .as_ref()
                        .unwrap()
                        .selected_worktree()
                        .is_some_and(|tree| tree.id == session.worktree && tree.project.is_none())
            }));
        }
        visual.update(|window, cx| {
            shell.update(cx, |shell, cx| {
                shell.open_destination(crate::resources::launcher::Destination::Files, window, cx);
            })
        });
        wait(
            visual,
            |cx| matches!(&shell.read(cx).side_resource, Some(SideResource::Plugin(panel)) if panel.read(cx).document_scope(cx) == Some((fixture.nodes[index].id(), session.worktree))),
        );
        visual.update(|window, cx| {
            shell.update(cx, |shell, cx| {
                shell.close_resource_panel(cx);
                shell.terminal_action(None, window, cx);
            })
        });
        wait(visual, |cx| {
            let shell = shell.read(cx);
            shell.page == crate::preview::Page::Terminal
                && shell
                    .live
                    .as_ref()
                    .unwrap()
                    .selected_worktree()
                    .is_some_and(|tree| tree.id == session.worktree)
                && shell
                    .live
                    .as_ref()
                    .unwrap()
                    .view
                    .snapshot
                    .as_ref()
                    .unwrap()
                    .terminals
                    .iter()
                    .any(|info| info.worktree == Some(session.worktree))
        });
        assert!(shell.read_with(visual, |shell, _| {
            shell.live.as_ref().unwrap().project.is_none()
        }));
    }
}

#[gpui::test]
fn overflowing_sessions_scroll_without_moving_peer_groups(cx: &mut TestAppContext) {
    cx.update(|cx| crate::preferences::update(cx, |data| data.sidebar_metrics = true));
    let fixture = Fixture::new();
    let (shell, visual) = mount(cx, &fixture);
    let handle = visual.update(|window, _| window.window_handle());
    for index in 0..2 {
        let transport = if index == 0 {
            fixture.nodes[0].local()
        } else {
            fixture.nodes[0]
                .link()
                .remote(fixture.nodes[1].link().address())
        };
        let client = Client::new(transport);
        let mut sessions = Vec::new();
        for _ in 0..32 {
            let Output::Session(session) = fixture
                .runtime
                .block_on(client.execute(client.prepare(Command::CreateSession {
                    project: None,
                    worktree: None,
                    config: Some(fixture.sessions[index].config.clone()),
                })))
                .unwrap()
            else {
                panic!("session expected")
            };
            sessions.push(session);
        }
        open(&shell, visual, &fixture, index, &sessions[0]);
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
                .iter()
                .filter(|session| session.project.is_none())
                .count()
                == sessions.len()
        });
        for height in [560., 820.] {
            visual.simulate_window_resize(handle, size(px(1280.), px(height)));
            visual.run_until_parked();
            visual.update(|window, cx| window.draw(cx).clear(cx));
            let viewport = visual.debug_bounds("sidebar-unassigned-viewport").unwrap();
            visual.simulate_event(ScrollWheelEvent {
                position: viewport.center(),
                delta: ScrollDelta::Pixels(point(px(0.), px(5000.))),
                ..Default::default()
            });
            visual.run_until_parked();
            visual.update(|window, cx| window.draw(cx).clear(cx));
            let viewport = visual.debug_bounds("sidebar-unassigned-viewport").unwrap();
            let projects = visual.debug_bounds("projects-heading").unwrap();
            let project_viewport = visual.debug_bounds("sidebar-projects-viewport").unwrap();
            let recent = visual.debug_bounds("recent-toggle").unwrap();
            let footer = visual.debug_bounds("sidebar-footer").unwrap();
            assert!(viewport.size.height > px(0.));
            assert!(visual.debug_bounds("unassigned-sessions").unwrap().bottom() <= viewport.top());
            assert!(viewport.bottom() <= projects.top());
            assert!(project_viewport.bottom() <= recent.top());
            assert!(recent.bottom() <= footer.top());
            let assigned: &'static str =
                Box::leak(format!("live-session-{}", fixture.sessions[index].id).into_boxed_str());
            let fixed = [
                "new-conversation",
                "hosts-heading",
                "active-toggle",
                "unassigned-sessions",
                "projects-heading",
                "sidebar-projects-viewport",
                assigned,
                "recent-toggle",
                "sidebar-footer",
            ]
            .map(|selector| (selector, visual.debug_bounds(selector).unwrap()));
            let (last, before) = sessions
                .iter()
                .map(|session| {
                    let selector: &'static str =
                        Box::leak(format!("live-session-{}", session.id).into_boxed_str());
                    (session, visual.debug_bounds(selector).unwrap())
                })
                .max_by(|(_, a), (_, b)| a.bottom().partial_cmp(&b.bottom()).unwrap())
                .unwrap();
            assert!(
                before.bottom() > viewport.bottom(),
                "large content must overflow its own viewport"
            );
            visual.simulate_event(ScrollWheelEvent {
                position: viewport.center(),
                delta: ScrollDelta::Pixels(point(px(0.), px(-5000.))),
                ..Default::default()
            });
            visual.run_until_parked();
            visual.update(|window, cx| window.draw(cx).clear(cx));
            let selector: &'static str =
                Box::leak(format!("live-session-{}", last.id).into_boxed_str());
            let after = visual.debug_bounds(selector).unwrap();
            assert!(after.top() < before.top());
            assert!(after.top() >= viewport.top() && after.bottom() <= viewport.bottom());
            for (selector, bounds) in fixed {
                assert_eq!(visual.debug_bounds(selector).unwrap(), bounds);
            }
            click(&shell, visual, selector.into());
            assert_eq!(
                shell.read_with(visual, |shell, _| shell.session_scope.active),
                Key::Session(fixture.nodes[index].id(), last.id)
            );
        }
        let Output::Snapshot(snapshot) = fixture
            .runtime
            .block_on(client.execute(client.prepare(Command::Snapshot)))
            .unwrap()
        else {
            panic!("snapshot expected")
        };
        assert!(snapshot.turns.is_empty());
        assert!(
            snapshot
                .sessions
                .iter()
                .all(|session| session.activity.run.is_none())
        );
    }
    visual.update(|window, _| window.remove_window());
    drop(shell);
    fixture.close();
}
