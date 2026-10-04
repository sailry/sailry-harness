use super::*;
mod lifecycle;

#[gpui::test]
fn retains_async_policy(cx: &mut TestAppContext) {
    init(cx);
    let first = Fixture::new(false);
    let second = Fixture::new(true);
    for (fixture, name) in [(&first, "first-only.txt"), (&second, "second-only.txt")] {
        std::fs::write(
            fixture.directory.path().join("project").join(name),
            "Scoped file",
        )
        .unwrap();
        fixture.package();
        fixture.install(0);
    }
    let mut panels = Vec::new();
    let (_, visual) = cx.add_window_view(|window, cx| {
        for fixture in [&first, &second] {
            let source = cx.new(|cx| {
                crate::conversation::live::View::new(
                    fixture.binding.clone(),
                    Some(fixture.session.clone()),
                    window,
                    cx,
                )
            });
            panels.push(cx.new(|cx| Panel::new(source, cx)));
        }
        let content = cx.new(|_| Pair(panels.clone()));
        Root::new(content, window, cx)
    });
    wait(visual, |cx| {
        panels.iter().all(|panel| {
            let panel = panel.read(cx);
            let metadata = panel.metadata.read(cx);
            panel.connected
                && metadata.settled()
                && metadata.entries.get("project-summary").is_some_and(|info| {
                    info.summary.enabled
                        && info.extension.as_ref().is_some_and(|ext| {
                            ext.desktop
                                .as_ref()
                                .is_some_and(|desktop| desktop.entry.is_some())
                        })
                })
        })
    });
    gpui_shell::policy::set_default(
        gpui_shell::policy::Policy::new().with_application("previous-host"),
    );
    visual.update(|window, cx| {
        for panel in &panels {
            let package = panel
                .read(cx)
                .metadata
                .read(cx)
                .entries
                .get("project-summary")
                .expect("installed package entry")
                .summary
                .reference();
            panel.update(cx, |panel, cx| panel.open(package, window, cx));
        }
    });
    wait(visual, |cx| {
        snapshot(&panels[0], cx).contains("first-only.txt")
            && snapshot(&panels[1], cx).contains("second-only.txt")
    });
    assert_eq!(gpui_shell::policy::default().application(), "previous-host");
    for (index, fixture, unexpected) in [
        (0, &first, "second-only.txt"),
        (1, &second, "first-only.txt"),
    ] {
        assert!(
            !panels[index]
                .read_with(visual, |_, cx| snapshot(&panels[index], cx))
                .contains(unexpected)
        );
        for request in fixture.transport.requests.lock().unwrap().iter() {
            assert_eq!(request.target, fixture.node.id());
            assert_eq!(
                request.plugin.as_ref().unwrap().worktree,
                fixture.binding.worktree
            );
            assert_eq!(
                request.plugin.as_ref().unwrap().session,
                Some(fixture.session.id)
            );
        }
    }
    gpui_shell::policy::set_default(gpui_shell::policy::Policy::new());
    visual.update(|window, _| window.remove_window());
    drop(panels);
    first.close();
    second.close();
}

struct Pair(Vec<Entity<Panel>>);
impl Render for Pair {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        h_flex().size_full().children(
            self.0
                .iter()
                .map(|panel| div().w_1_2().h_full().child(panel.clone())),
        )
    }
}

#[gpui::test]
fn checks_access(cx: &mut TestAppContext) {
    init(cx);
    for remote in [false, true] {
        let fixture = Fixture::new(remote);
        fixture.package();
        let root = fixture.directory.path().join("project/package");
        std::fs::write(
            root.join("dev.sailry.platform/desktop/main.js"),
            include_str!("scopes.js"),
        )
        .unwrap();
        let mut manifest: serde_json::Value =
            serde_json::from_slice(&std::fs::read(root.join("plugin.json")).unwrap()).unwrap();
        manifest["extensions"]["dev.sailry.platform"]["actions"] = serde_json::json!([]);
        std::fs::write(root.join("plugin.json"), manifest.to_string()).unwrap();
        fixture.install(0);
        let (panel, visual) = mount(&fixture, cx);
        open_package(visual, &panel, "project-summary");
        wait(visual, |cx| snapshot(&panel, cx).contains("done"));
        let result = panel.read_with(visual, |_, cx| snapshot(&panel, cx));
        assert!(!result.contains("ALLOWED"), "{result}");
        for expected in [
            "own-source",
            "read-denied",
            "write-denied",
            "process-denied",
            "network-denied",
            "clipboard-denied",
            "storage-denied",
            "watch-denied",
            "node-permission_denied",
            "settings-public",
        ] {
            assert!(result.contains(expected), "{expected}: {result}");
        }
        assert_eq!(fixture.transport.files.load(Ordering::SeqCst), 0);
        visual.update(|window, _| window.remove_window());
        drop(panel);
        fixture.close();
    }
}
