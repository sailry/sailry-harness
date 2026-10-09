use super::*;

#[gpui::test]
fn aligns_grouped_servers_and_keeps_controls(cx: &mut TestAppContext) {
    init(cx);
    for remote in [false, true] {
        let fixture = Fixture::new(remote);
        std::fs::write(fixture.root.join("index.html"), "<p>Fixture</p>\n").unwrap();
        std::fs::write(fixture.root.join("script.js"), "const fixture = 1;\n").unwrap();
        let (owner, cx) = fixture.mount(cx);
        let panel = cx.update(|_, cx| selected(&owner, cx));
        cx.update(|_, cx| {
            let languages = &panel.read(cx).workspace.as_ref().unwrap().languages;
            assert_eq!(languages[0].id, "html");
            assert_eq!(languages[1].id, "javascript");
            assert_eq!(
                languages[0]
                    .servers
                    .iter()
                    .map(|candidate| candidate.server.as_str())
                    .collect::<Vec<_>>(),
                vec!["vscode-html-language-server", "superhtml"]
            );
        });
        let handle = cx.update(|window, _| window.window_handle());
        for width in [560., 1100.] {
            cx.simulate_window_resize(handle, size(px(width), px(1100.)));
            draw(cx);
            let first_control = cx.debug_bounds("code-server-0-0-control").unwrap();
            let first_actions = cx.debug_bounds("code-server-0-0-actions").unwrap();
            for (language, server) in [(0, 0), (0, 1), (1, 0)] {
                let entry = bounds(cx, format!("code-server-{language}-{server}"));
                let content = bounds(cx, format!("code-server-{language}-{server}-content"));
                let control = bounds(cx, format!("code-server-{language}-{server}-control"));
                let actions = bounds(cx, format!("code-server-{language}-{server}-actions"));
                let enabled = bounds(cx, format!("code-enabled-{language}-{server}"));
                let edit = bounds(cx, format!("code-edit-{language}-{server}"));
                assert!(content.size.width > px(0.));
                assert!(content.right() < control.left());
                assert!(control.right() < actions.left());
                assert_eq!(control.size.width, px(48.));
                assert_eq!(actions.size.width, px(120.));
                assert_eq!(control.left(), first_control.left());
                assert_eq!(actions.left(), first_actions.left());
                assert_eq!(actions.right(), entry.right());
                assert!((control.center().y - entry.center().y).abs() <= px(0.5));
                assert!((edit.center().y - enabled.center().y).abs() <= px(0.5));
                assert!(control.contains(&enabled.center()));
                assert!(entry.contains(&enabled.center()));
                assert!(actions.contains(&edit.center()));
                if let Some(install) = cx.debug_bounds(Box::leak(
                    format!("code-install-{language}-{server}").into_boxed_str(),
                )) {
                    assert!((install.center().y - enabled.center().y).abs() <= px(0.5));
                    assert_eq!(install.size, edit.size);
                    assert!(actions.contains(&install.center()));
                }
            }
            let html = cx.debug_bounds("code-language-0").unwrap();
            let javascript = cx.debug_bounds("code-language-1").unwrap();
            assert!(html.bottom() <= javascript.top());
            for server in [0, 1] {
                let entry = bounds(cx, format!("code-server-0-{server}"));
                assert!(html.contains(&entry.center()));
            }
        }
        assert!(fixture.transport.requests.lock().unwrap().is_empty());
        for (server, name) in [(0, "vscode-html-language-server"), (1, "superhtml")] {
            tap(cx, &format!("code-enabled-0-{server}"));
            wait(cx, |cx| !panel.read(cx).pending && !panel.read(cx).loading);
            assert!(fixture.settings().servers[name].disabled);
        }
        fixture.close(cx);
    }
}

fn bounds(cx: &mut VisualTestContext, selector: String) -> Bounds<Pixels> {
    cx.debug_bounds(Box::leak(selector.into_boxed_str()))
        .unwrap()
}
