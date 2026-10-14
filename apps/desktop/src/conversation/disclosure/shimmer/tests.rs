use super::*;
use core::prelude::v1::test;

#[test]
fn preserves_semantic_ink() {
    for base in [
        hsla(0., 0., 0.45, 1.),
        hsla(0., 0., 0.7, 0.65),
        hsla(0.05, 0.8, 0.5, 0.75),
        hsla(0.65, 0.6, 0.4, 1.),
    ] {
        // Off-band glyphs keep their original ink rather than a full-text highlight.
        assert_eq!(Glyphs::color(base, 0.), base);
        for amount in [0., 0.25, 0.5, 0.75, 1.] {
            let color = Glyphs::color(base, amount);
            assert_eq!((color.h, color.s, color.l), (base.h, base.s, base.l));
            assert!(color.a <= base.a);
            assert!(color.a >= base.a * 0.65);
        }
        assert!(Glyphs::color(base, 1.).a < base.a);
    }
}

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

struct Scan {
    phase: f32,
    layout: std::rc::Rc<std::cell::RefCell<Option<TextLayout>>>,
}

impl Render for Scan {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let summary = Summary::new(
            "fixed-scan".into(),
            "AV office fi café".into(),
            &super::super::Detail {
                mono: true,
                text: "inventory/中文/e\u{301}🙂.py".into(),
                secondary: true,
                ..Default::default()
            },
        );
        let style = window.text_style();
        let secondary = cx.theme().muted_foreground.opacity(0.65);
        let runs = summary.runs(&style, cx.theme().mono_font_family.clone(), secondary);
        let glyphs = Glyphs {
            text: StyledText::new(summary.text).with_runs(runs),
            detail_start: summary.detail_start,
            color: style.color,
            secondary: Some(secondary),
            phase: self.phase,
        };
        *self.layout.borrow_mut() = Some(glyphs.text.layout().clone());
        div()
            .debug_selector(|| "fixed-scan".into())
            .w(px(520.))
            .truncate()
            .text_size(cx.theme().typography_tokens().sm.size)
            .child(glyphs)
    }
}

#[gpui::test]
fn keeps_glyph_positions_across_scan_phases(cx: &mut TestAppContext) {
    crate::conversation::live::tests::fixture::init(cx);
    let layout = std::rc::Rc::new(std::cell::RefCell::new(None));
    let mut entity = None;
    let (_, visual) = cx.add_window_view(|window, cx| {
        let view = cx.new(|_| Scan {
            phase: 0.,
            layout: layout.clone(),
        });
        entity = Some(view.clone());
        Root::new(view, window, cx)
    });
    let entity = entity.unwrap();
    for mode in [ThemeMode::Light, ThemeMode::Dark] {
        let mut resting = None;
        for phase in [0., 0.125, 0.25, 0.5, 0.75, 1.] {
            entity.update(visual, |view, cx| {
                view.phase = phase;
                cx.notify();
            });
            visual.update(|window, cx| {
                Theme::change(mode, Some(window), cx);
                window.draw(cx).clear(cx);
            });
            let layout = layout.borrow();
            let layout = layout.as_ref().unwrap();
            let positions: Vec<_> = layout
                .line_layouts()
                .iter()
                .flat_map(|line| {
                    line.unwrapped_layout.runs.iter().flat_map(|run| {
                        run.glyphs
                            .iter()
                            .map(move |glyph| (run.font_id, glyph.id, glyph.index, glyph.position))
                    })
                })
                .collect();
            let current = (positions, layout.bounds(), layout.line_height());
            assert!(!current.0.is_empty());
            if let Some(resting) = &resting {
                assert_eq!(&current, resting, "scan phase: {phase}");
            } else {
                resting = Some(current);
            }
        }
    }
    visual.update(|window, _| window.remove_window());
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
            if !reduced {
                visual.simulate_mouse_move(text.center(), None, Modifiers::default());
                visual.run_until_parked();
                let frames = visual.update(|window, cx| {
                    window.draw(cx).clear(cx);
                    window.simulate_next_frame(cx);
                    window.draw(cx).clear(cx);
                    window.simulate_next_frame(cx)
                });
                assert_eq!(frames, 0, "hovered text keeps its normal static ink");
                visual.simulate_mouse_move(point(px(500.), px(500.)), None, Modifiers::default());
                visual.run_until_parked();
            }
        }
    }
    visual.update(|window, _| window.remove_window());
}
