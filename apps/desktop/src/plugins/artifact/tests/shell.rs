use super::*;
use crate::ui::observation;
use crate::{backend::Services, resources::SideResource, shell::Shell};
use sailry_protocol::artifact::Kind;
use std::{cell::RefCell, rc::Rc};

#[gpui_kit::test]
fn mounts_captured_header(cx: &mut TestAppContext) {
    init(cx);
    for remote in [false, true] {
        for kind in [Kind::Document, Kind::Preview] {
            let mut initial = write(None, None);
            initial["kind"] = json!(kind);
            initial["title"] = json!(
                "Architecture with captured resources across the execution Node and authenticated controller views"
            );
            initial["summary"] = json!("Preserve the full authored summary across constrained conversation widths and immutable historical revisions. ".repeat(2));
            if kind == Kind::Preview {
                initial["html"] = json!(
                    "<!doctype html><html><head></head><body><h1>Native preview</h1></body></html>"
                );
            }
            let fixture = chat::Fixture::with_server(remote, |runtime| {
                runtime.block_on(crate::agent_fixture::Server::scripted_tools(vec![(
                    crate::agent_fixture::plugin_tool(PACKAGE, "write_brief"),
                    initial,
                )]))
            });
            install(&fixture);
            fixture.start();
            let first = published(&fixture, 1);
            cx.update(|cx| {
                cx.set_global(Services {
                    profile: fixture.node.profile().to_path_buf(),
                    runtime: fixture.runtime.clone(),
                    local: fixture.node.local(),
                    link: fixture.node.link(),
                    relay_enabled: false,
                });
                cx.set_global(crate::preferences::Preferences::default());
            });
            let mut owner = None;
            let (_, visual) = cx.add_window_view(|window, cx| {
                let shell = cx.new(|cx| Shell::new(window, cx));
                shell.update(cx, |shell, cx| {
                    let live = shell.live.as_mut().unwrap();
                    live.override_transport(fixture.transport.clone());
                    live.select(fixture.node.id(), cx);
                    shell.reveal_session(fixture.session.clone(), window, cx);
                });
                owner = Some(shell.clone());
                Root::new(shell, window, cx)
            });
            let shell = owner.unwrap();
            let handle = visual.update(|window, _| window.window_handle());
            visual.simulate_window_resize(handle, size(px(1440.), px(900.)));
            wait(visual, |cx| {
                shell
                    .read(cx)
                    .current_chat()
                    .is_some_and(|source| source.read(cx).document_visible(&first))
            });
            let card_source =
                shell.read_with(visual, |shell, _| shell.current_chat().unwrap().clone());
            let opened = Rc::new(RefCell::new(Vec::new()));
            let events = opened.clone();
            let documents = visual.update(|_, cx| {
                cx.subscribe(&card_source, move |_, event, _| {
                    if let crate::conversation::live::Event::Document(artifact) = event {
                        events.borrow_mut().push(artifact.clone());
                    }
                })
            });
            let mut card_widths = Vec::new();
            for width in [1440., 960., 1440.] {
                visual.simulate_window_resize(handle, size(px(width), px(900.)));
                wait(visual, |_| true);
                card_widths.push(card(visual, &first));
            }
            assert!(card_widths[1] <= card_widths[0]);
            assert_eq!(card_widths[2], card_widths[0]);
            chat::tap(
                visual,
                &format!("artifact-document-title-{}-{}", first.id, first.revision),
            );
            assert!(opened.borrow().is_empty(), "the title is not an action");
            let labels = actions(visual, &first);
            let mut expected = vec![
                crate::tr("artifact_open").to_string(),
                crate::tr("conversation_export_html").to_string(),
            ];
            if kind == Kind::Document {
                expected.push(crate::tr("conversation_export_image").to_string());
            }
            assert_eq!(labels, expected);
            visual.simulate_keystrokes("escape");
            visual.run_until_parked();
            assert!(visual.update(|window, _| window.try_find("popup-menu").is_none()));
            assert!(opened.borrow().is_empty());

            let model_requests = fixture.server.requests.lock().unwrap().len();
            actions(visual, &first);
            visual.simulate_keystrokes("down down enter");
            let platform = visual.cx.clone();
            wait(visual, |_| platform.did_prompt_for_new_path());
            assert!(shell.read_with(visual, |shell, _| shell.side_resource.is_none()));
            visual.simulate_new_path_selection(|_| None);
            let platform = visual.cx.clone();
            wait(visual, |_| !platform.did_prompt_for_new_path());
            assert_eq!(
                fixture.server.requests.lock().unwrap().len(),
                model_requests
            );
            assert!(opened.borrow().is_empty());

            actions(visual, &first);
            visual.simulate_keystrokes("down enter");
            wait(visual, |_| opened.borrow().len() == 1);
            assert_eq!(opened.borrow()[0], first);
            chat::tap(
                visual,
                &format!("artifact-document-open-{}-{}", first.id, first.revision),
            );
            assert_eq!(opened.borrow().as_slice(), &[first.clone(), first.clone()]);
            let (source, panel) = shell.read_with(visual, |shell, _| {
                let Some(SideResource::Plugin(panel)) = &shell.side_resource else {
                    panic!("document card must open the Shell resource pane");
                };
                (shell.current_chat().unwrap().clone(), panel.clone())
            });
            version(&panel, visual, 1);
            let mut widths = Vec::new();
            for width in [1440., 1680., 1440.] {
                visual.simulate_window_resize(handle, size(px(width), px(900.)));
                wait(visual, |_| true);
                widths.push(controls(visual));
            }
            assert!(widths[1] > widths[0]);
            assert_eq!(widths[2], widths[0]);
            let host = panel.read_with(visual, |panel, _| panel.mounted.as_ref().unwrap().host());
            assert_eq!(host.target(), fixture.node.id());
            assert_eq!(host.context().session, Some(first.session));

            let mut next = write(Some(first.id), Some(1));
            next["kind"] = json!(kind);
            if kind == Kind::Preview {
                next["html"] = json!(
                    "<!doctype html><html><head></head><body><h1>Revised native preview</h1></body></html>"
                );
            }
            fixture.server.replace_tools(vec![(
                crate::agent_fixture::plugin_tool(PACKAGE, "write_brief"),
                next,
            )]);
            wait(visual, |cx| {
                source.read(cx).document_writable(first.session)
            });
            let (sender, status) = tokio::sync::watch::channel(None);
            assert!(visual.update(|window, cx| source.update(cx, |source, cx| {
                source.send_document(
                    first.session,
                    "Revise the document".into(),
                    sender,
                    window,
                    cx,
                )
            })));
            wait(visual, |_| *status.borrow() == Some(true));
            assert_eq!(published(&fixture, 2).id, first.id);
            version(&panel, visual, 2);
            controls(visual);
            visual.update(|window, cx| window.clear_notifications(cx));
            crate::feedback::tests::settle(visual);
            visual.update(|window, cx| {
                window.render_frame(cx);
                let header = panel.read(cx).header().unwrap();
                let id = header.read(cx).filter_id("brief-version").unwrap();
                window.within(id).click("input", cx);
            });
            visual.run_until_parked();
            visual.update(|window, cx| {
                window.render_frame(cx);
                assert!(gpui_kit::component::GlobalState::is_in_deferred_context(cx));
                let header = panel.read(cx).header().unwrap();
                assert_eq!(
                    header
                        .read(cx)
                        .filter_interaction("brief-version", window, cx),
                    Some((Some(0), Some("2".into()), true))
                );
                window.press("down", cx);
            });
            visual.run_until_parked();
            visual.update(|window, cx| {
                window.render_frame(cx);
                window.press("enter", cx);
            });
            version(&panel, visual, 1);
            controls(visual);

            chat::tap(visual, "brief-export");
            shown(visual, "popup-menu");
            assert!(visual.debug_bounds("brief-export-picker").is_none());
            visual.update(|window, cx| {
                window.render_frame(cx);
                let html_label = crate::tr("conversation_export_html");
                let mut expected = vec![html_label.to_string()];
                if kind == Kind::Document {
                    expected.push(crate::tr("conversation_export_image").to_string());
                }
                assert_eq!(
                    observation::roles_within(window, "popup-menu", Role::MenuItem)
                        .iter()
                        .map(|item| item.label().unwrap().to_owned())
                        .collect::<Vec<_>>(),
                    expected
                );
                let mut menu = window.within("popup-menu");
                let html = menu.find(0usize);
                assert_eq!(html.role(), Some(Role::MenuItem));
                assert_eq!(html.label(), Some(html_label.as_ref()));
                assert!(html.visible());
                menu.click(0usize, cx);
            });
            let platform = visual.cx.clone();
            wait(visual, |_| platform.did_prompt_for_new_path());
            wait(visual, |cx| export_disabled(&panel, cx) == Some(true));
            assert!(visual.debug_bounds("brief-export-picker").is_none());
            visual.simulate_new_path_selection(|_| None);
            wait(visual, |cx| export_disabled(&panel, cx) == Some(false));
            assert!(!visual.did_prompt_for_new_path());
            chat::tap(visual, "brief-export");
            shown(visual, "popup-menu");
            assert!(visual.debug_bounds("brief-export-picker").is_none());
            visual.update(|window, cx| window.press("escape", cx));
            visual.update(|window, _| window.remove_window());
            drop(panel);
            drop(source);
            drop(documents);
            drop(card_source);
            drop(shell);
            host.close();
            drop(host);
            cx.update(|cx| cx.remove_global::<Services>());
            fixture.close();
        }
    }
}

#[gpui_kit::test]
#[cfg(any(target_os = "macos", target_os = "windows"))]
fn opens_new_card_without_restoring_older_draft(cx: &mut TestAppContext) {
    init(cx);
    for remote in [false, true] {
        let fixture = chat::Fixture::with_server(remote, |runtime| {
            runtime.block_on(crate::agent_fixture::Server::scripted_tools(vec![(
                crate::agent_fixture::plugin_tool(PACKAGE, "write_brief"),
                write(None, None),
            )]))
        });
        install(&fixture);
        fixture.start();
        let first = published(&fixture, 1);
        cx.update(|cx| {
            cx.set_global(Services {
                profile: fixture.node.profile().to_path_buf(),
                runtime: fixture.runtime.clone(),
                local: fixture.node.local(),
                link: fixture.node.link(),
                relay_enabled: false,
            });
            cx.set_global(crate::preferences::Preferences::default());
        });
        cx.update(|cx| cx.set_global(surface::tests::Mounted::default()));
        let mut owner = None;
        let (_, visual) = cx.add_window_view(|window, cx| {
            let shell = cx.new(|cx| Shell::new(window, cx));
            shell.update(cx, |shell, cx| {
                let live = shell.live.as_mut().unwrap();
                live.override_transport(fixture.transport.clone());
                live.select(fixture.node.id(), cx);
                shell.reveal_session(fixture.session.clone(), window, cx);
            });
            owner = Some(shell.clone());
            Root::new(shell, window, cx)
        });
        let shell = owner.unwrap();
        let handle = visual.update(|window, _| window.window_handle());
        visual.simulate_window_resize(handle, size(px(1440.), px(900.)));
        wait(visual, |cx| {
            shell
                .read(cx)
                .current_chat()
                .is_some_and(|source| source.read(cx).document_visible(&first))
        });
        let source = shell.read_with(visual, |shell, _| shell.current_chat().unwrap().clone());
        chat::tap(visual, &format!("artifact-document-open-{}-1", first.id));
        let original = shell.read_with(visual, |shell, _| {
            let Some(SideResource::Plugin(panel)) = &shell.side_resource else {
                panic!("document card must open the Shell resource pane");
            };
            panel.clone()
        });
        version(&original, visual, 1);
        let (resource, host) = original.read_with(visual, |panel, _| {
            (
                panel.document.clone().unwrap(),
                panel.mounted.as_ref().unwrap().host(),
            )
        });
        let key = (resource.binding.client.target(), resource.artifact.id);
        let owner = visual.read(|cx| cx.global::<surface::tests::Mounted>().owner(key));
        surface::tests::settle_mount(owner, resource.artifact.revision, visual);
        let cache = surface::tests::Draft::new(resource, host, visual).close();

        fixture.server.replace_tools(vec![(
            crate::agent_fixture::plugin_tool(PACKAGE, "write_brief"),
            write(Some(first.id), Some(1)),
        )]);
        wait(visual, |cx| {
            source.read(cx).document_writable(first.session)
        });
        let (sender, status) = tokio::sync::watch::channel(None);
        assert!(visual.update(|window, cx| source.update(cx, |source, cx| {
            source.send_document(
                first.session,
                "Revise the document".into(),
                sender,
                window,
                cx,
            )
        })));
        wait(visual, |_| *status.borrow() == Some(true));
        let second = published(&fixture, 2);
        wait(visual, |cx| source.read(cx).document_visible(&second));
        visual.update(|window, cx| window.clear_notifications(cx));
        crate::feedback::tests::settle(visual);
        chat::tap(visual, &format!("artifact-document-open-{}-2", second.id));
        let panel = shell.read_with(visual, |shell, _| {
            let Some(SideResource::Plugin(panel)) = &shell.side_resource else {
                panic!("revised card must open the Shell resource pane");
            };
            panel.clone()
        });
        assert_ne!(panel.entity_id(), original.entity_id());
        assert_eq!(
            panel.read_with(visual, |panel, _| panel
                .document
                .as_ref()
                .unwrap()
                .artifact
                .clone()),
            second
        );
        wait(visual, |cx| {
            surface::tests::mounted_revision(fixture.binding.client.target(), second.id, cx)
                == Some((2, 2))
        });
        version(&panel, visual, 2);
        visual.update(|_, cx| {
            let node = fixture.binding.client.target();
            assert_eq!(
                crate::preferences::artifacts::drafts(node, first.id, 1, cx).len(),
                1
            );
            assert!(crate::preferences::artifacts::drafts(node, first.id, 2, cx).is_empty());
        });

        visual.update(|window, cx| {
            window.clear_notifications(cx);
            window.render_frame(cx);
            let header = panel.read(cx).header().unwrap();
            let id = header.read(cx).filter_id("brief-version").unwrap();
            window.within(id).click("input", cx);
        });
        visual.run_until_parked();
        visual.update(|window, cx| window.press("down", cx));
        visual.run_until_parked();
        visual.update(|window, cx| window.press("enter", cx));
        version(&panel, visual, 1);
        wait(visual, |cx| {
            surface::tests::mounted_revision(fixture.binding.client.target(), first.id, cx)
                == Some((1, 2))
        });
        fixture.server.replace_tools(vec![(
            crate::agent_fixture::plugin_tool(PACKAGE, "write_brief"),
            write(Some(first.id), Some(2)),
        )]);
        wait(visual, |cx| {
            source.read(cx).document_writable(first.session)
        });
        let (sender, status) = tokio::sync::watch::channel(None);
        assert!(visual.update(|window, cx| source.update(cx, |source, cx| {
            source.send_document(
                first.session,
                "Revise the document again".into(),
                sender,
                window,
                cx,
            )
        })));
        wait(visual, |_| *status.borrow() == Some(true));
        published(&fixture, 3);
        wait(visual, |cx| {
            surface::tests::mounted_revision(fixture.binding.client.target(), first.id, cx)
                == Some((1, 3))
        });
        version(&panel, visual, 1);
        visual.update(|window, _| window.remove_window());
        drop(panel);
        drop(original);
        drop(source);
        drop(shell);
        drop(cache);
        cx.update(|cx| cx.remove_global::<Services>());
        fixture.close();
    }
}

fn card(visual: &mut VisualTestContext, artifact: &Summary) -> Pixels {
    let id = artifact.id;
    let revision = artifact.revision;
    visual.update(|window, cx| window.render_frame(cx));
    let tile = visual
        .debug_bounds(Box::leak(
            format!("artifact-document-icon-{id}-{revision}").into_boxed_str(),
        ))
        .unwrap();
    let title = visual
        .debug_bounds(Box::leak(
            format!("artifact-document-title-{id}-{revision}").into_boxed_str(),
        ))
        .unwrap();
    let label = visual
        .debug_bounds(Box::leak(
            format!("artifact-document-label-{id}-{revision}").into_boxed_str(),
        ))
        .unwrap();
    let summary = visual
        .debug_bounds(Box::leak(
            format!("artifact-document-summary-{id}-{revision}").into_boxed_str(),
        ))
        .unwrap();
    let open = visual
        .debug_bounds(Box::leak(
            format!("artifact-document-open-{id}-{revision}").into_boxed_str(),
        ))
        .unwrap();
    let more = visual
        .debug_bounds(Box::leak(
            format!("artifact-document-more-{id}-{revision}").into_boxed_str(),
        ))
        .unwrap();
    assert_eq!(tile.size, size(px(40.), px(40.)));
    assert_eq!(title.size.height, px(56.));
    let side = visual.update(|window, _| window.rem_size() * 1.75);
    assert_eq!(
        visual.debug_bounds("file-type-html").unwrap().size,
        size(side, side)
    );
    assert!(tile.right() <= label.left());
    assert_eq!(label.left(), summary.left());
    assert!(label.bottom() <= summary.top());
    assert!(label.right() <= title.right() && summary.right() <= title.right());
    assert!(title.right() <= open.left() && open.right() <= more.left());
    let line = visual.update(|window, _| window.rem_size() * 1.5);
    assert!(label.size.height <= line && summary.size.height <= line);
    visual.update(|window, _| {
        assert_eq!(
            window
                .find(format!("artifact-document-label-{id}-{revision}"))
                .label(),
            Some(artifact.title.as_str())
        );
        assert_eq!(
            window
                .find(format!("artifact-document-summary-{id}-{revision}"))
                .label(),
            Some(artifact.summary.as_str())
        );
    });
    title.size.width
}

fn actions(visual: &mut VisualTestContext, artifact: &Summary) -> Vec<String> {
    chat::tap(
        visual,
        &format!(
            "artifact-document-more-{}-{}",
            artifact.id, artifact.revision
        ),
    );
    visual.update(|window, cx| {
        window.render_frame(cx);
        observation::roles(window, Role::MenuItem)
            .iter()
            .map(|item| item.label().unwrap().to_owned())
            .collect()
    })
}

fn export_disabled(panel: &Entity<Panel>, cx: &App) -> Option<bool> {
    panel
        .read(cx)
        .header()
        .and_then(|header| header.read(cx).action_disabled("brief-export"))
}

fn controls(visual: &mut VisualTestContext) -> Pixels {
    let header = visual.debug_bounds("plugin-header").unwrap();
    assert_eq!(header.size.height, px(crate::preview::HEADER_HEIGHT));
    assert!(header.left() > visual.debug_bounds("main-content").unwrap().left());
    for selector in ["brief-version", "brief-export"] {
        let bounds = visual.debug_bounds(selector).unwrap();
        assert!(bounds.size.width > px(0.) && bounds.size.height > px(0.));
        assert!(bounds.left() >= header.left() && bounds.right() <= header.right());
        assert!(bounds.top() >= header.top() && bounds.bottom() <= header.bottom());
    }
    let version = visual.debug_bounds("brief-version").unwrap();
    let export = visual.debug_bounds("brief-export").unwrap();
    let gap = visual.update(|window, _| window.rem_size() * 0.75);
    assert!((export.left() - version.right() - gap).abs() < px(1.));
    assert!((header.right() - export.right() - gap).abs() < px(1.));
    header.size.width
}

fn shown(visual: &mut VisualTestContext, selector: &'static str) {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        wait(visual, |_| true);
        let observed = visual.update(|window, _| window.try_find(selector).is_some());
        if observed || visual.debug_bounds(selector).is_some() {
            return;
        }
        assert!(Instant::now() < deadline, "missing {selector}");
        std::thread::sleep(Duration::from_millis(10));
    }
}
