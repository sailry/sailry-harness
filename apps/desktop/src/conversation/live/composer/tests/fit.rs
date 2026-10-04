use super::*;

struct Columns {
    view: Entity<View>,
    width: Pixels,
}

impl Render for Columns {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        h_flex()
            .size_full()
            .child(
                div()
                    .w(self.width)
                    .h_full()
                    .flex_shrink_0()
                    .child(self.view.clone()),
            )
            .child(div().flex_1())
    }
}

#[gpui::test]
fn uses_column_width(cx: &mut TestAppContext) {
    init(cx);
    for remote in [false, true] {
        let fixture = Fixture::with_tools(remote, vec![]);
        let mut owner = None;
        let mut layout = None;
        let (_, visual) = cx.add_window_view(|window, cx| {
            let view = cx.new(|cx| {
                View::new(
                    fixture.binding.clone(),
                    Some(fixture.session.clone()),
                    window,
                    cx,
                )
            });
            owner = Some(view.clone());
            let columns = cx.new(|_| Columns {
                view,
                width: px(900.),
            });
            layout = Some(columns.clone());
            Root::new(columns, window, cx)
        });
        let view = owner.unwrap();
        let columns = layout.unwrap();
        let handle = visual.update(|window, _| window.window_handle());
        visual.simulate_window_resize(handle, size(px(1440.), px(820.)));
        wait(visual, |cx| {
            view.read(cx).connected() && view.read(cx).configured()
        });
        tap(visual, "live-chat-input");
        visual.simulate_input("**Preserved draft** 中文");
        for width in [900., 520., 900., 360., 900.] {
            columns.update(visual, |columns, cx| {
                columns.width = px(width);
                cx.notify();
            });
            wait(visual, |cx| {
                view.read(cx).compact_composer == (width < 680.)
            });
            visual.update(|window, cx| window.draw(cx).clear(cx));
            assert_eq!(
                visual.update(|window, _| window.viewport_size().width),
                px(1440.)
            );
            assert_eq!(
                visual.debug_bounds("composer-settings").is_some(),
                width < 680.
            );
            assert_eq!(
                visual.debug_bounds("live-chat-mode").is_some(),
                width >= 680.
            );
            assert_eq!(
                visual.debug_bounds("live-chat-model").is_some(),
                width >= 680.
            );
            assert!(visual.update(|window, cx| {
                view.read(cx)
                    .input
                    .read(cx)
                    .focus_handle(cx)
                    .is_focused(window)
            }));
            assert_eq!(
                view.read_with(visual, |view, cx| view.input.read(cx).value()),
                "**Preserved draft** 中文"
            );
        }
        visual.simulate_input(" 🙂");
        assert_eq!(
            view.read_with(visual, |view, cx| view.input.read(cx).value()),
            "**Preserved draft** 中文 🙂"
        );
        assert!(fixture.server.requests.lock().unwrap().is_empty());
        visual.update(|window, _| window.remove_window());
        drop(columns);
        drop(view);
        fixture.close();
    }
}
