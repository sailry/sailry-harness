use super::*;
use core::prelude::v1::test;

#[test]
fn preserves_unicode_and_mixed_fonts() {
    let detail = super::super::Detail {
        running: true,
        mono: true,
        text: "docs/中文🙂/e\u{301}.rs\nsecond line".into(),
        ..Default::default()
    };
    let summary = Summary::new("summary".into(), "Create folder".into(), &detail);
    let mut style = TextStyle::default();
    style.font_family = "UI face".into();
    let runs = summary.runs(&style, "Mono face".into(), hsla(0., 0., 0.5, 1.));
    assert_eq!(
        summary.text,
        "Create folder docs/中文🙂/e\u{301}.rs\nsecond line"
    );
    let mut offset = 0;
    for run in &runs {
        assert!(summary.text.is_char_boundary(offset));
        assert_eq!(
            run.font.family,
            if offset < summary.detail_start {
                "UI face"
            } else {
                "Mono face"
            }
        );
        offset += run.len;
        assert!(summary.text.is_char_boundary(offset));
    }
    assert_eq!(offset, summary.text.len());
    assert_eq!(detail.accessible(&"Create folder".into()), summary.text);
}

struct Activity;

impl Render for Activity {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div().w(px(320.)).child(super::super::trigger(
            "mixed-activity".into(),
            IconName::Folder,
            "Create folder".into(),
            super::super::Detail {
                running: true,
                mono: true,
                text: "docs/中文🙂/e\u{301}.rs ".repeat(60).into(),
                ..Default::default()
            },
            false,
            cx,
        ))
    }
}

#[gpui::test]
fn shares_one_scan_and_native_bounds(cx: &mut TestAppContext) {
    crate::conversation::live::tests::fixture::init(cx);
    let (_, visual) = cx.add_window_view(|window, cx| {
        let view = cx.new(|_| Activity);
        Root::new(view, window, cx)
    });
    for mode in [ThemeMode::Light, ThemeMode::Dark] {
        for reduced in [true, false] {
            visual.update(|window, cx| {
                Theme::change(mode, Some(window), cx);
                cx.set_reduce_motion(reduced);
                window.refresh();
                window.draw(cx).clear(cx);
            });
            let frames = visual.update(|window, cx| {
                window.simulate_next_frame(cx);
                window.draw(cx).clear(cx);
                window.simulate_next_frame(cx)
            });
            assert_eq!(frames > 0, !reduced);
            let trigger = visual.debug_bounds("mixed-activity").unwrap();
            let text = visual
                .debug_bounds("mixed-activity-summary-shimmer")
                .unwrap();
            assert_eq!(trigger.size.height, px(24.));
            assert!(trigger.right() <= px(320.));
            assert!(text.right() <= trigger.right());
            assert!(
                visual
                    .debug_bounds("mixed-activity-summary-label")
                    .is_none()
            );
        }
    }
    visual.update(|window, _| window.remove_window());
}
