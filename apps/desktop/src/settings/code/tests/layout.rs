use super::*;

#[gpui::test]
fn aligns_flat_servers_and_keeps_icon_actions(cx: &mut TestAppContext) {
    init(cx);
    for remote in [false, true] {
        let fixture = Fixture::new(remote);
        std::fs::write(fixture.root.join("index.html"), "<p>Fixture</p>\n").unwrap();
        std::fs::write(fixture.root.join("script.js"), "const fixture = 1;\n").unwrap();
        std::fs::write(
            fixture.root.join("script.ts"),
            "const fixture: number = 1;\n",
        )
        .unwrap();
        let (owner, cx) = fixture.mount(cx);
        let panel = cx.update(|_, cx| selected(&owner, cx));
        cx.update(|_, cx| {
            let rows = panel.read(cx).rows();
            let typescript = rows
                .iter()
                .filter(|row| row.server == "typescript-language-server")
                .collect::<Vec<_>>();
            assert_eq!(typescript.len(), 1);
            assert!(typescript[0].languages.contains(&"javascript".into()));
            assert!(typescript[0].languages.contains(&"typescript".into()));
        });
        let handle = cx.update(|window, _| window.window_handle());
        for width in [560., 1100.] {
            cx.simulate_window_resize(handle, size(px(width), px(1100.)));
            draw(cx);
            assert!(cx.debug_bounds("code-language-0").is_none());
            assert!(cx.debug_bounds("code-refresh").is_none());
            reveal(cx, "code-tab-installed");
            let toolbar = bounds(cx, "code-server-toolbar".into());
            let installed = bounds(cx, "code-tab-installed".into());
            let available = bounds(cx, "code-tab-available".into());
            let list = bounds(cx, "settings-surface-code_languages".into());
            assert!(toolbar.contains(&installed.center()));
            assert!(toolbar.contains(&available.center()));
            assert!(available.right() < toolbar.right());
            assert!(list.top() >= toolbar.bottom());
            reveal(cx, "code-server-superhtml");
            let first_actions = bounds(cx, "code-server-superhtml-actions".into());
            for server in [
                "superhtml",
                "vscode-html-language-server",
                "typescript-language-server",
            ] {
                reveal(cx, &format!("code-server-{server}"));
                let entry = bounds(cx, format!("code-server-{server}"));
                let content = bounds(cx, format!("code-server-{server}-content"));
                let actions = bounds(cx, format!("code-server-{server}-actions"));
                let details = bounds(cx, format!("code-details-{server}"));
                let install = bounds(cx, format!("code-install-{server}"));
                assert!(content.size.width > px(0.));
                assert!(content.right() < actions.left());
                assert_eq!(actions.size.width, px(120.));
                assert_eq!(actions.left(), first_actions.left());
                assert_eq!(actions.right(), entry.right());
                assert!((details.center().y - entry.center().y).abs() <= px(0.5));
                assert!((install.center().y - details.center().y).abs() <= px(0.5));
                assert_eq!(install.size, details.size);
                assert!(actions.contains(&details.center()));
                assert!(actions.contains(&install.center()));
                assert!(lookup(cx, &format!("code-server-{server}-control")).is_none());
                assert!(lookup(cx, &format!("code-edit-{server}")).is_none());
            }
        }
        assert!(fixture.transport.requests.lock().unwrap().is_empty());
        tap(cx, "code-details-vscode-html-language-server");
        assert!(cx.debug_bounds("code-server-details").is_some());
        assert!(cx.debug_bounds("code-server-description").is_some());
        assert!(
            cx.debug_bounds("settings-surface-code_information")
                .is_some()
        );
        assert!(cx.debug_bounds("code-detail-package").is_some());
        assert!(cx.debug_bounds("code-detail-command").is_some());
        assert!(cx.debug_bounds("code-detail-licenses").is_some());
        assert!(cx.debug_bounds("code-details-uninstall").is_none());
        assert!(fixture.transport.requests.lock().unwrap().is_empty());
        tap(cx, "code-details-close");
        tap(cx, "code-tab-installed");
        assert!(cx.debug_bounds("empty-code_installations_empty").is_some());
        assert!(cx.debug_bounds("code-install-superhtml").is_none());
        tap(cx, "code-tab-available");
        assert!(cx.debug_bounds("code-install-superhtml").is_some());
        fixture.close(cx);
    }
}

fn bounds(cx: &mut VisualTestContext, selector: String) -> Bounds<Pixels> {
    lookup(cx, &selector).unwrap()
}
