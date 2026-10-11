use super::*;
use crate::{backend::Services, resources::SideResource, shell::Shell};

#[gpui_kit::test]
fn mounts_captured_header(cx: &mut TestAppContext) {
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
        chat::tap(
            visual,
            &format!("artifact-document-open-{}-{}", first.id, first.revision),
        );
        let (source, panel) = shell.read_with(visual, |shell, _| {
            let Some(SideResource::Plugin(panel)) = &shell.side_resource else {
                panic!("document card must open the Shell resource pane");
            };
            (shell.current_chat().unwrap().clone(), panel.clone())
        });
        version(&panel, visual, 1);
        let header = visual.debug_bounds("plugin-header").unwrap();
        assert_eq!(header.size.height, px(crate::preview::HEADER_HEIGHT));
        assert!(header.left() > visual.debug_bounds("main-content").unwrap().left());
        for selector in ["brief-version", "brief-export"] {
            let bounds = visual.debug_bounds(selector).unwrap();
            assert!(bounds.size.width > px(0.) && bounds.size.height > px(0.));
            assert!(bounds.left() >= header.left() && bounds.right() <= header.right());
            assert!(bounds.top() >= header.top() && bounds.bottom() <= header.bottom());
        }
        let host = panel.read_with(visual, |panel, _| panel.mounted.as_ref().unwrap().host());
        assert_eq!(host.target(), fixture.node.id());
        assert_eq!(host.context().session, Some(first.session));

        fixture.server.replace_tools(vec![(
            crate::agent_fixture::plugin_tool(PACKAGE, "write_brief"),
            write(Some(first.id), Some(1)),
        )]);
        wait(visual, |cx| {
            source.read(cx).document_writable(first.session)
        });
        let (sender, status) = tokio::sync::watch::channel(None);
        assert!(visual.update(
            |window, cx| source.update(cx, |source, cx| source.send_document(
                first.session,
                "Revise the document".into(),
                sender,
                window,
                cx
            ))
        ));
        wait(visual, |_| *status.borrow() == Some(true));
        assert_eq!(published(&fixture, 2).id, first.id);
        version(&panel, visual, 2);
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

        chat::tap(visual, "brief-export");
        shown(visual, "brief-export-picker");
        assert!(visual.debug_bounds("brief-export-html").is_some());
        assert!(visual.debug_bounds("brief-export-image").is_some());
        visual.update(|window, cx| window.press("escape", cx));
        visual.update(|window, _| window.remove_window());
        drop(panel);
        drop(source);
        drop(shell);
        host.close();
        drop(host);
        cx.update(|cx| cx.remove_global::<Services>());
        fixture.close();
    }
}

fn shown(visual: &mut VisualTestContext, selector: &'static str) {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        wait(visual, |_| true);
        if visual.debug_bounds(selector).is_some() {
            return;
        }
        assert!(Instant::now() < deadline, "missing {selector}");
        std::thread::sleep(Duration::from_millis(10));
    }
}
