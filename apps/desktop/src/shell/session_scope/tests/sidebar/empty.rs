use super::*;

#[gpui::test]
fn tracks_catalog_and_disclosure(cx: &mut TestAppContext) {
    let fixture = Fixture::new();
    let (shell, visual) = mount(cx, &fixture);
    let handle = visual.update(|window, _| window.window_handle());
    visual.simulate_window_resize(handle, size(px(1280.), px(820.)));
    for index in 0..2 {
        open(&shell, visual, &fixture, index, &fixture.sessions[index]);
        visual.update(|window, cx| {
            crate::preferences::update(cx, |data| data.recent = Some(Vec::new()));
            window.draw(cx).clear(cx);
        });
        if visual.debug_bounds("sidebar-recent-viewport").is_none() {
            click(&shell, visual, "recent-toggle".into());
        }
        for empty in [
            "active-sessions-empty",
            "unassigned-sessions-empty",
            "recent-sessions-empty",
        ] {
            let row = visual.debug_bounds(empty).unwrap();
            let guide = visual
                .debug_bounds(selector(format!("{empty}-guide")))
                .unwrap();
            let vertical = visual
                .debug_bounds(selector(format!("{empty}-guide-vertical")))
                .unwrap();
            assert!(
                guide.left() > row.left() - px(30.) && guide.right() < row.left(),
                "{empty}: row {row:?}, guide {guide:?}"
            );
            assert!(vertical.bottom() <= row.center().y + px(1.));
        }
        visual.update(|window, cx| window.draw(cx).clear(cx));
        assert!(visual.debug_bounds("sidebar-projects-empty").is_none());
        let populated = shell.read_with(visual, |shell, _| {
            shell.live.as_ref().unwrap().view.snapshot.clone().unwrap()
        });

        // Loading and empty catalogs are presentation fixtures, not database mutations.
        visual.update(|window, cx| {
            shell.update(cx, |shell, cx| {
                shell.live.as_mut().unwrap().view.snapshot = None;
                cx.notify();
            });
            window.draw(cx).clear(cx);
        });
        assert!(visual.debug_bounds("sidebar-projects-empty").is_none());
        visual.update(|window, cx| {
            shell.update(cx, |shell, cx| {
                let mut other = populated.clone();
                other.node = fixture.nodes[1 - index].id();
                other.projects.clear();
                shell.live.as_mut().unwrap().view.snapshot = Some(other);
                cx.notify();
            });
            window.draw(cx).clear(cx);
        });
        assert!(visual.debug_bounds("sidebar-projects-empty").is_none());
        visual.update(|window, cx| {
            shell.update(cx, |shell, cx| {
                let mut empty = populated.clone();
                empty.projects.clear();
                shell.live.as_mut().unwrap().view.snapshot = Some(empty);
                cx.notify();
            });
            window.draw(cx).clear(cx);
        });
        let heading = visual.debug_bounds("projects-heading").unwrap();
        let viewport = visual.debug_bounds("sidebar-projects-viewport").unwrap();
        let empty = visual.debug_bounds("sidebar-projects-empty").unwrap();
        assert!(empty.top() >= heading.bottom());
        assert!(empty.bottom() <= viewport.bottom());
        assert!(empty.size.height > px(0.) && empty.size.height <= px(32.));
        click(&shell, visual, "projects-toggle".into());
        visual.update(|window, cx| window.draw(cx).clear(cx));
        assert!(visual.debug_bounds("sidebar-projects-empty").is_none());
        click(&shell, visual, "projects-toggle".into());
        visual.update(|window, cx| window.draw(cx).clear(cx));
        assert!(visual.debug_bounds("sidebar-projects-empty").is_some());

        visual.update(|window, cx| {
            shell.update(cx, |shell, cx| {
                shell.live.as_mut().unwrap().view.snapshot = Some(populated);
                cx.notify();
            });
            window.draw(cx).clear(cx);
        });
        assert!(visual.debug_bounds("sidebar-projects-empty").is_none());
        assert!(
            visual
                .debug_bounds(selector(format!(
                    "live-project-{}",
                    fixture.sessions[index].project.unwrap()
                )))
                .is_some()
        );
    }
    visual.update(|window, _| window.remove_window());
    fixture.close();
}
