use super::*;
use gpui_kit::component::slider::{Slider, SliderEvent, SliderState};

pub(in crate::settings) struct State {
    main: Entity<SliderState>,
    sidebar: Entity<SliderState>,
}

impl State {
    pub fn new(cx: &mut Context<Workspace>) -> Self {
        let saved = crate::preferences::data(cx).surfaces.unwrap_or_default();
        let mut slider = |sidebar: bool, opacity: f32| {
            let state = cx.new(|_| {
                SliderState::new()
                    .min(0.)
                    .max(100.)
                    .step(1.)
                    .default_value((100. - opacity) * 100. / 55.)
            });
            cx.subscribe(&state, move |_, _, event, cx| {
                let value = match event {
                    SliderEvent::Change(value) | SliderEvent::Release(value) => value.start(),
                };
                let opacity = 100. - value * 55. / 100.;
                if matches!(event, SliderEvent::Change(_)) {
                    cx.update_global::<crate::preferences::Preferences, _>(|preferences, _| {
                        let surfaces = preferences
                            .data
                            .surfaces
                            .get_or_insert_with(Default::default);
                        if sidebar {
                            surfaces.sidebar = Some(opacity);
                        } else {
                            surfaces.main = opacity;
                        }
                    });
                }
                if matches!(event, SliderEvent::Release(_)) {
                    crate::preferences::update(cx, |data| {
                        let surfaces = data.surfaces.get_or_insert_with(Default::default);
                        if sidebar {
                            surfaces.sidebar = Some(opacity);
                        } else {
                            surfaces.main = opacity;
                        }
                    });
                }
                cx.notify();
                cx.refresh_windows();
            })
            .detach();
            state
        };
        Self {
            main: slider(false, saved.main),
            sidebar: slider(true, saved.sidebar.unwrap_or(45.)),
        }
    }

    pub fn controls(&self, cx: &App) -> impl IntoElement {
        Group::new("appearance_transparency").children(
            [
                ("appearance_main_transparency", &self.main),
                ("appearance_sidebar_transparency", &self.sidebar),
            ]
            .map(|(key, state)| {
                Row::new(
                    key,
                    h_flex()
                        .w(px(240.))
                        .gap_3()
                        .child(
                            div()
                                .debug_selector(move || key.into())
                                .flex_1()
                                .child(Slider::new(state)),
                        )
                        .child(
                            div().w_10().text_right().child(format!(
                                "{}%",
                                state.read(cx).value().start().round() as u8
                            )),
                        ),
                )
            }),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::prelude::v1::test;
    #[gpui::test]
    fn changes_panel_tints_independently(cx: &mut TestAppContext) {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("preferences.json");
        cx.update(|cx| {
            gpui_kit::init(cx);
            crate::theme::init(cx);
            cx.set_global(crate::preferences::Preferences::open(path.clone()));
        });
        let mut owner = None;
        let (_, visual) = cx.add_window_view(|window, cx| {
            let workspace = cx.new(|cx| Workspace::new(window, cx));
            workspace.update(cx, |workspace, cx| {
                workspace.select(crate::settings::Section::Appearance, cx);
                assert_eq!(workspace.opacity.main.read(cx).value().start(), 30.);
                assert_eq!(workspace.opacity.sidebar.read(cx).value().start(), 100.);
                assert_eq!(crate::theme::panel_background(cx).a, 0.835);
                assert_eq!(crate::theme::sidebar_background(cx).a, 0.45);
            });
            owner = Some(workspace.clone());
            Root::new(workspace, window, cx)
        });
        visual.run_until_parked();
        visual.update(|window, cx| window.draw(cx).clear(cx));
        let bounds = visual.debug_bounds("appearance_main_transparency").unwrap();
        visual.simulate_click(bounds.center(), Modifiers::none());
        visual.run_until_parked();
        let surfaces = visual.update(|_, cx| crate::preferences::data(cx).surfaces.unwrap());
        assert!((65.0..=80.0).contains(&surfaces.main), "{surfaces:?}");
        assert_eq!(surfaces.sidebar, None);
        assert_eq!(
            crate::preferences::Preferences::open(path.clone())
                .data
                .surfaces,
            Some(surfaces.clone())
        );
        // Reopening uses the same percentage, including fractional stored alpha.
        visual.update(|window, cx| {
            let restored = cx.new(|cx| Workspace::new(window, cx));
            let before = owner
                .as_ref()
                .unwrap()
                .read(cx)
                .opacity
                .main
                .read(cx)
                .value()
                .start();
            let after = restored.read(cx).opacity.main.read(cx).value().start();
            assert!((before - after).abs() < 0.001);
        });
        let main_alpha = visual.update(|_, cx| crate::theme::panel_background(cx).a);
        let bounds = visual
            .debug_bounds("appearance_sidebar_transparency")
            .unwrap();
        visual.simulate_click(bounds.center(), Modifiers::none());
        visual.run_until_parked();
        let changed = visual.update(|_, cx| crate::preferences::data(cx).surfaces.unwrap());
        assert_eq!(changed.main, surfaces.main);
        assert!(
            (65.0..=80.0).contains(&changed.sidebar.unwrap()),
            "{changed:?}"
        );
        visual.update(|_, cx| {
            assert_eq!(crate::theme::panel_background(cx).a, main_alpha);
            assert_eq!(
                crate::theme::sidebar_background(cx).a,
                changed.sidebar.unwrap() / 100.
            );
        });
        assert_eq!(
            crate::preferences::Preferences::open(path.clone())
                .data
                .surfaces,
            Some(changed)
        );
        let end = point(bounds.right() - px(0.1), bounds.center().y);
        visual.simulate_click(end, Modifiers::none());
        visual.run_until_parked();
        visual.update(|_, cx| {
            assert_eq!(
                owner
                    .as_ref()
                    .unwrap()
                    .read(cx)
                    .opacity
                    .sidebar
                    .read(cx)
                    .value()
                    .start(),
                100.
            );
            assert_eq!(crate::theme::sidebar_background(cx).a, 0.45);
            assert_eq!(crate::theme::panel_background(cx).a, main_alpha);
        });
        assert_eq!(
            crate::preferences::Preferences::open(path)
                .data
                .surfaces
                .unwrap()
                .sidebar,
            Some(45.)
        );
    }
}
