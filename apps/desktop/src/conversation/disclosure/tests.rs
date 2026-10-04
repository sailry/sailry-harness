use super::*;
use core::prelude::v1::test;
use gpui_kit::component::collapsible::Collapsible;

struct Harness {
    open: bool,
    detail: SharedString,
}

impl Render for Harness {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div().w(px(560.)).child(
            Collapsible::new()
                .open(self.open)
                .w_full()
                .child(
                    trigger(
                        "activity".into(),
                        IconName::BookOpen,
                        "Read".into(),
                        self.detail.clone(),
                        self.open,
                        cx,
                    )
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.open = !this.open;
                        cx.notify();
                    })),
                )
                .content(div().debug_selector(|| "output".into()).w_full().h_20()),
        )
    }
}

#[gpui::test]
fn preserves_inline_alignment(cx: &mut TestAppContext) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        crate::theme::init(cx);
    });
    let (_, visual) = cx.add_window_view(|window, cx| {
        let view = cx.new(|_| Harness {
            open: false,
            detail: "source.txt".into(),
        });
        Root::new(view, window, cx)
    });
    for mode in [ThemeMode::Light, ThemeMode::Dark] {
        visual.update(|window, cx| {
            Theme::change(mode, Some(window), cx);
            window.draw(cx).clear(cx);
        });
        let collapsed = visual.debug_bounds("activity-summary").unwrap();
        let trigger = visual.debug_bounds("activity").unwrap();
        assert_eq!(collapsed.left(), trigger.left());
        assert!(trigger.size.width < px(300.));
        visual.simulate_click(trigger.center(), Modifiers::default());
        visual.run_until_parked();
        visual.update(|window, cx| window.draw(cx).clear(cx));
        let expanded = visual.debug_bounds("activity-summary").unwrap();
        let output = visual.debug_bounds("output").unwrap();
        assert_eq!(expanded.left(), collapsed.left());
        assert_eq!(expanded.left(), output.left());
        assert_eq!(expanded.size, collapsed.size);
        visual.simulate_click(trigger.center(), Modifiers::default());
        visual.run_until_parked();
    }
}

#[gpui::test]
fn long_details_fit_the_column(cx: &mut TestAppContext) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        crate::theme::init(cx);
    });
    let (_, visual) = cx.add_window_view(|window, cx| {
        let view = cx.new(|_| Harness {
            open: false,
            detail: "A long command with arguments ".repeat(100).into(),
        });
        Root::new(view, window, cx)
    });
    visual.update(|window, cx| window.draw(cx).clear(cx));
    let trigger = visual.debug_bounds("activity").unwrap();
    let summary = visual.debug_bounds("activity-summary").unwrap();
    assert!(
        trigger.right() <= px(560.),
        "trigger exceeds column: {trigger:?}"
    );
    assert!(
        summary.right() <= px(560.),
        "summary exceeds column: {summary:?}"
    );
    assert_eq!(trigger.size.height, px(24.));
}
