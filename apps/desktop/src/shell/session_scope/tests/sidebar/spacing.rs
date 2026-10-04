use super::*;

#[gpui::test]
fn headings_keep_the_project_height(cx: &mut TestAppContext) {
    let fixture = Fixture::new();
    let (shell, visual) = mount(cx, &fixture);
    let window = visual.update(|window, _| window.window_handle());
    for index in 0..2 {
        let session = &fixture.sessions[index];
        let active = selector(format!("active-session-{}", session.id));
        open(&shell, visual, &fixture, index, session);
        for populated in [false, true] {
            if populated {
                fixture.submit(index);
                wait(visual, |cx| {
                    shell
                        .read(cx)
                        .active_sessions()
                        .iter()
                        .any(|(_, entry)| entry.id == session.id)
                });
            }
            for height in [700., 1000.] {
                visual.simulate_window_resize(window, size(px(1200.), px(height)));
                for (step, closed) in [false, true, false].into_iter().enumerate() {
                    if step > 0 {
                        click(&shell, visual, "hosts-toggle".into());
                        click(&shell, visual, "active-toggle".into());
                    }
                    visual.update(|window, cx| {
                        window.draw(cx).clear(cx);
                    });
                    assert_eq!(visual.debug_bounds("sidebar-active").is_some(), !closed);
                    assert_eq!(visual.debug_bounds(active).is_some(), populated && !closed);
                    let projects = visual.debug_bounds("projects-heading").unwrap();
                    for heading in ["hosts-heading", "active-toggle", "recent-toggle"] {
                        let bounds = visual.debug_bounds(heading).unwrap();
                        assert_eq!(
                            bounds.size.height, projects.size.height,
                            "{heading}: node={index}, height={height}, populated={populated}, closed={closed}"
                        );
                    }
                }
            }
        }
    }
    visual.update(|window, _| window.remove_window());
    fixture.close();
}
