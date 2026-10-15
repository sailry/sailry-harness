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
