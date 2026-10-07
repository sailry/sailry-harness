use super::*;
use gpui_kit::test::TestWindowExt as _;

fn surface(visual: &mut VisualTestContext) {
    visual.update(|window, cx| {
        let bounds = window.find("theme-license-details").bounds();
        let scaled = Bounds::from_corners(
            window.pixel_snap_point(bounds.origin),
            window.pixel_snap_point(bounds.bottom_right()),
        )
        .scale(window.scale_factor());
        let color = cx.theme().tokens.accordion.color;
        let fills = window
            .painted_quads()
            .into_iter()
            .filter(|quad| {
                quad.bounds.left() >= scaled.left()
                    && quad.bounds.right() <= scaled.right()
                    && quad.bounds.top() >= scaled.top()
                    && quad.bounds.bottom() <= scaled.bottom()
                    && quad.background.as_solid().is_some_and(|paint| {
                        // Dialog animation multiplies alpha without changing the semantic fill.
                        paint.a > 0.
                            && paint.h == color.h
                            && paint.s == color.s
                            && paint.l == color.l
                    })
            })
            .collect::<Vec<_>>();
        assert_eq!(
            fills.len(),
            1,
            "license must have one background fill at {scaled:?}"
        );
        let fill = &fills[0];
        assert_eq!(fill.bounds, scaled);
        let radius = cx.theme().radius_lg.scale(window.scale_factor());
        assert_eq!(fill.corner_radii.top_left, radius);
        assert_eq!(fill.corner_radii.top_right, radius);
        assert_eq!(fill.corner_radii.bottom_left, radius);
        assert_eq!(fill.corner_radii.bottom_right, radius);
        let border = window
            .painted_quads()
            .into_iter()
            .find(|quad| {
                quad.bounds == scaled
                    && quad.border_widths.top > px(0.).scale(window.scale_factor())
            })
            .expect("license must retain its Kit border");
        assert_eq!(fill.corner_radii, border.corner_radii);
    });
}

#[gpui::test]
fn keeps_fill_inside_the_rounded_border(cx: &mut TestAppContext) {
    let directory = tempfile::tempdir().unwrap();
    let source = theme::fixture::write(directory.path(), "tide");
    let visual = mount(cx, directory.path());
    for mode in [ThemeMode::Light, ThemeMode::Dark] {
        for (width, scale) in [(420., 1.), (960., 2.)] {
            let handle = visual.update(|window, cx| {
                Theme::change(mode, Some(window), cx);
                // A distinct translucent semantic fill detects duplicate interior painting.
                Theme::global_mut(cx).tokens.accordion = Hsla::from(rgba(0x5389a899)).into();
                window.window_handle()
            });
            visual.simulate_window_resize(handle, size(px(width), px(1100.)));
            visual.simulate_scale_factor_change(scale);
            visual.update(|window, cx| {
                import::preview(
                    theme::package::Loaded::read(&source, true).unwrap(),
                    window,
                    cx,
                )
            });
            settle(visual);
            surface(visual);
            assert!(visual.debug_bounds("theme-license-text").is_none());
            let footer = actions(visual).map(|bounds| bounds.size);
            click(visual, "theme-license-toggle");
            assert!(visual.debug_bounds("theme-license-text").is_some());
            surface(visual);
            assert_eq!(actions(visual).map(|bounds| bounds.size), footer);
            click(visual, "theme-license-toggle");
            for _ in 0..100 {
                if visual.debug_bounds("theme-license-text").is_none() {
                    break;
                }
                settle(visual);
            }
            assert!(visual.debug_bounds("theme-license-text").is_none());
            surface(visual);
            click(visual, "theme-import-cancel");
            assert!(!visual.update(|window, cx| window.has_active_dialog(cx)));
        }
    }
    assert!(!directory.path().join("desktop/themes/tide").exists());
}
