use super::*;

fn fill(visual: &mut VisualTestContext, selector: String) -> Quad {
    let bounds = visual
        .debug_bounds(Box::leak(selector.into_boxed_str()))
        .unwrap();
    visual.update(|window, _| {
        let bounds = Bounds::from_corners(
            window.pixel_snap_point(bounds.origin),
            window.pixel_snap_point(bounds.bottom_right()),
        )
        .scale(window.scale_factor());
        window
            .painted_quads()
            .into_iter()
            .find(|quad| {
                quad.bounds == bounds && quad.background != Hsla::transparent_black().into()
            })
            .unwrap_or_else(|| panic!("theme card must paint its surface at {bounds:?}"))
    })
}

#[gpui::test]
fn rounds_each_exposed_surface(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    let directory = tempfile::tempdir().unwrap();
    let source = theme::fixture::write(directory.path(), "tide");
    cx.update(|cx| {
        cx.set_global(crate::preferences::Preferences::open(
            directory.path().join("desktop/preferences.json"),
        ));
        gpui_kit::init(cx);
        theme::init(cx);
        cx.update_global::<theme::Catalog, _>(|catalog, _| {
            catalog
                .add(theme::package::Loaded::read(&source, true).unwrap())
                .unwrap();
        });
    });
    let (_, visual) = cx.add_window_view(|window, cx| {
        let view = cx.new(|cx| Workspace::new(window, cx));
        view.update(cx, |view, cx| {
            view.select(super::super::super::Section::Appearance, cx)
        });
        let mounted = cx.new(|_| Mounted(view));
        Root::new(mounted, window, cx)
    });
    for mode in [ThemeMode::Light, ThemeMode::Dark] {
        for (width, scale) in [(420., 1.), (960., 2.)] {
            let handle = visual.update(|window, cx| {
                theme::select(Some(mode), window, cx);
                window.window_handle()
            });
            visual.simulate_window_resize(handle, size(px(width), px(820.)));
            visual.simulate_scale_factor_change(scale);
            for radius in [0., 8., 32.] {
                visual.update(|_, cx| {
                    Theme::global_mut(cx).radius_lg = px(radius);
                    Theme::global_mut(cx).group_box = rgba(0x53678999).into();
                    cx.refresh_windows();
                });
                settle(visual);
                let expected = visual
                    .update(|window, _| px((radius - 1.).max(0.)).scale(window.scale_factor()));
                for index in [0, 1] {
                    let id = visual
                        .update(|_, cx| cx.global::<theme::Catalog>().packages[index].id.clone());
                    let header = fill(
                        visual,
                        format!("theme-preview-surface-{id}-{}", mode.is_dark()),
                    );
                    assert_eq!(header.corner_radii.top_left, expected);
                    assert_eq!(header.corner_radii.top_right, expected);
                    assert_eq!(header.corner_radii.bottom_left, px(0.).scale(1.));
                    assert_eq!(header.corner_radii.bottom_right, px(0.).scale(1.));
                    let sidebar = fill(
                        visual,
                        format!("theme-preview-sidebar-{id}-{}", mode.is_dark()),
                    );
                    assert_eq!(sidebar.corner_radii.top_left, expected);
                    assert_eq!(sidebar.corner_radii.top_right, px(0.).scale(1.));
                    let footer = fill(visual, format!("theme-card-label-fill-{index}"));
                    assert_eq!(footer.corner_radii.top_left, px(0.).scale(1.));
                    assert_eq!(footer.corner_radii.top_right, px(0.).scale(1.));
                    assert_eq!(footer.corner_radii.bottom_left, expected);
                    assert_eq!(footer.corner_radii.bottom_right, expected);
                    assert_eq!(header.bounds.left(), footer.bounds.left());
                    assert_eq!(header.bounds.right(), footer.bounds.right());
                    let row = visual
                        .debug_bounds(Box::leak(
                            format!("theme-card-label-{index}").into_boxed_str(),
                        ))
                        .unwrap();
                    let row = visual.update(|window, _| {
                        Bounds::from_corners(
                            window.pixel_snap_point(row.origin),
                            window.pixel_snap_point(row.bottom_right()),
                        )
                        .scale(window.scale_factor())
                    });
                    assert_eq!(header.bounds.bottom(), row.top());
                    assert_eq!(footer.bounds.bottom(), row.bottom());
                    assert_eq!(footer.bounds.intersect(&footer.content_mask.bounds), row);
                }
            }
        }
    }
}
