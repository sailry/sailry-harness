use super::*;
use core::prelude::v1::test;
use gpui_kit::component::Root;
use sailry_protocol::{Authentication, conversation::ModelApi};

#[gpui::test]
fn selection_and_filters(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let picker = cx.new(|_| Picker::new());
    let channels = [ModelApi::Responses, ModelApi::Anthropic, ModelApi::DeepSeek]
        .into_iter()
        .enumerate()
        .map(|(id, api)| {
            let mut provider = crate::provider_fixture::provider(Authentication::ChatGpt, api);
            provider.authentication = Authentication::ApiKey;
            provider.endpoint = match api {
                ModelApi::Responses => "https://api.openai.com/v1",
                ModelApi::Anthropic => "https://api.anthropic.com",
                ModelApi::DeepSeek => "https://api.deepseek.com",
                _ => unreachable!(),
            }
            .into();
            let mut channel = crate::settings::channel(id, &provider);
            channel
                .models
                .push(crate::settings::Model::example("fixture"));
            channel
        })
        .collect::<Vec<_>>();
    let selection = |channel| {
        Some(Selection {
            channel,
            model: "fixture".into(),
        })
    };
    picker.update(cx, |picker, cx| {
        picker.set(channels.clone(), selection(0), cx);
        assert_eq!(picker.family, Some(ModelCategory::OpenAi));
        picker.family = Some(ModelCategory::Anthropic);
        picker.set(channels.clone(), selection(0), cx);
        assert_eq!(picker.family, Some(ModelCategory::Anthropic));
        picker.reveal(cx);
        assert_eq!(picker.family, Some(ModelCategory::OpenAi));
        picker.collapsed.insert(2);
        picker.set(channels.clone(), selection(2), cx);
        assert_eq!(picker.family.unwrap().key(), "provider_deepseek");
        assert!(!picker.collapsed.contains(&2));
        picker.set(channels, selection(1), cx);
        assert_eq!(picker.family, Some(ModelCategory::Anthropic));
    });
}

#[gpui::test]
fn scrolls_expanded_groups(cx: &mut TestAppContext) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        crate::theme::init(cx);
        cx.set_reduce_motion(true);
    });
    let mut owner = None;
    let (_, visual) = cx.add_window_view(|window, cx| {
        let picker = cx.new(|_| Picker::new());
        picker.update(cx, |picker, cx| {
            picker.categories = false;
            picker.set_groups(
                (0..2)
                    .map(|id| Group {
                        id,
                        name: format!("Provider {id}"),
                        category: ModelCategory::Other,
                        models: (0..if id == 0 { 12 } else { 2 })
                            .map(|index| Choice {
                                id: format!("model-{index}"),
                                label: format!("Model {index}"),
                                effort: sailry_protocol::Effort::Default,
                                efforts: vec![],
                            })
                            .collect(),
                    })
                    .collect(),
                None,
                cx,
            );
        });
        owner = Some(picker.clone());
        Root::new(picker, window, cx)
    });
    let picker = owner.unwrap();
    let draw = |visual: &mut VisualTestContext| {
        visual.run_until_parked();
        visual.update(|window, cx| window.draw(cx).clear(cx));
    };
    draw(visual);
    let scroll = visual.debug_bounds("composer-model-scroll").unwrap();
    assert!(scroll.size.height <= px(320.));
    visual.simulate_event(ScrollWheelEvent {
        position: scroll.center(),
        delta: ScrollDelta::Pixels(point(px(0.), px(-2000.))),
        touch_phase: TouchPhase::Moved,
        modifiers: Modifiers::default(),
    });
    draw(visual);
    let bottom = picker.read_with(visual, |picker, _| picker.scroll.offset().y);
    assert!(bottom < px(-100.));
    for collapsed in [true, false] {
        let header = visual.debug_bounds("composer-model-group-1").unwrap();
        visual.simulate_click(header.center(), Modifiers::default());
        visual
            .executor()
            .advance_clock(std::time::Duration::from_secs(1));
        draw(visual);
        assert_eq!(
            picker.read_with(visual, |picker, _| picker.collapsed.contains(&1)),
            collapsed
        );
    }
    visual.simulate_event(ScrollWheelEvent {
        position: scroll.center(),
        delta: ScrollDelta::Pixels(point(px(0.), px(-2000.))),
        touch_phase: TouchPhase::Moved,
        modifiers: Modifiers::default(),
    });
    draw(visual);
    let last = visual
        .debug_bounds("composer-model-option-1-model-1")
        .unwrap();
    assert!(
        last.top() >= scroll.top() && last.bottom() <= scroll.bottom(),
        "last provider must remain reachable: {last:?}, {scroll:?}"
    );
    let selected = std::rc::Rc::new(std::cell::RefCell::new(None));
    let received = selected.clone();
    let _subscription = visual.update(|_, cx| {
        cx.subscribe(&picker, move |_, picked: &Picked, _| {
            *received.borrow_mut() = Some(picked.selection.clone());
        })
    });
    visual.simulate_click(last.center(), Modifiers::default());
    draw(visual);
    assert_eq!(
        *selected.borrow(),
        Some(Selection {
            channel: 1,
            model: "model-1".into()
        })
    );
}

#[gpui::test]
fn brand_navigation(cx: &mut TestAppContext) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        crate::theme::init(cx);
    });
    let (_, visual) = cx.add_window_view(|window, cx| {
        let picker = cx.new(|_| Picker::new());
        picker.update(cx, |picker, cx| {
            picker.set_groups(
                vec![Group {
                    id: 0,
                    name: "Provider".into(),
                    category: ModelCategory::OpenAi,
                    models: vec![Choice {
                        id: "fixture".into(),
                        label: "Fixture".into(),
                        effort: sailry_protocol::Effort::Default,
                        efforts: vec![],
                    }],
                }],
                Some(Selection {
                    channel: 0,
                    model: "fixture".into(),
                }),
                cx,
            );
        });
        Root::new(picker, window, cx)
    });
    for width in [480., 1280.] {
        let handle = visual.update(|window, _| window.window_handle());
        visual.simulate_window_resize(handle, size(px(width), px(820.)));
        visual.run_until_parked();
        visual.update(|window, cx| window.draw(cx).clear(cx));
        let panel = visual.debug_bounds("composer-model-picker").unwrap();
        let navigation = visual.debug_bounds("composer-model-navigation").unwrap();
        let model = visual
            .debug_bounds("composer-model-option-0-fixture")
            .unwrap();
        assert!(navigation.size.width <= px(48.));
        assert!(model.size.width >= px(120.));
        assert!(model.left() >= navigation.right());
        assert!(model.right() <= panel.right());
        assert!(visual.debug_bounds("composer-model-family-0").is_some());
        assert!(visual.debug_bounds("composer-model-family-1").is_none());
        assert!(visual.debug_bounds("composer-model-family-2").is_none());
    }
}

#[gpui::test]
fn categories_follow_available_providers(cx: &mut TestAppContext) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        crate::theme::init(cx);
    });
    let channel = |id, api, enabled, models| {
        let mut provider = crate::provider_fixture::provider(Authentication::ApiKey, api);
        provider.enabled = enabled;
        let mut channel = crate::settings::channel(id, &provider);
        if models {
            channel
                .models
                .push(crate::settings::Model::example("fixture"));
        }
        channel
    };
    let mut owner = None;
    let (_, visual) = cx.add_window_view(|window, cx| {
        let picker = cx.new(|_| Picker::new());
        picker.update(cx, |picker, cx| {
            picker.set(
                vec![
                    channel(0, ModelApi::Responses, true, true),
                    channel(1, ModelApi::Responses, true, true),
                    channel(2, ModelApi::Anthropic, true, true),
                    channel(3, ModelApi::Gemini, false, true),
                    channel(4, ModelApi::OpenCodeGo, true, false),
                ],
                Some(Selection {
                    channel: 0,
                    model: "fixture".into(),
                }),
                cx,
            )
        });
        owner = Some(picker.clone());
        Root::new(picker, window, cx)
    });
    let picker = owner.unwrap();
    let draw = |visual: &mut VisualTestContext| {
        visual.run_until_parked();
        visual.update(|window, cx| window.draw(cx).clear(cx));
    };
    draw(visual);
    assert!(visual.debug_bounds("composer-model-family-0").is_some());
    assert!(visual.debug_bounds("composer-model-family-1").is_some());
    for id in [
        "composer-model-family-2",
        "composer-model-family-3",
        "composer-model-family-4",
    ] {
        assert!(visual.debug_bounds(id).is_none());
    }
    let family = visual.debug_bounds("composer-model-family-1").unwrap();
    visual.simulate_click(family.center(), Modifiers::default());
    draw(visual);
    assert!(
        visual
            .debug_bounds("composer-model-option-2-fixture")
            .is_some()
    );
    assert!(
        visual
            .debug_bounds("composer-model-option-0-fixture")
            .is_none()
    );
    assert!(
        visual
            .debug_bounds("composer-model-option-1-fixture")
            .is_none()
    );
    picker.update(visual, |picker, cx| {
        picker.set(
            vec![channel(0, ModelApi::Responses, true, true)],
            Some(Selection {
                channel: 0,
                model: "fixture".into(),
            }),
            cx,
        )
    });
    draw(visual);
    assert_eq!(
        picker.read_with(visual, |picker, _| picker.family),
        Some(ModelCategory::OpenAi)
    );
    assert!(visual.debug_bounds("composer-model-family-1").is_none());
    assert!(
        visual
            .debug_bounds("composer-model-option-0-fixture")
            .is_some()
    );
    assert!(visual.debug_bounds("composer-model-empty").is_none());
    picker.update(visual, |picker, cx| picker.set(Vec::new(), None, cx));
    draw(visual);
    assert_eq!(picker.read_with(visual, |picker, _| picker.family), None);
    assert!(visual.debug_bounds("composer-model-family-0").is_none());
    assert!(visual.debug_bounds("composer-model-empty").is_some());
}
