use super::*;
use crate::settings::providers::Preset;
use core::prelude::v1::test;

struct View;

impl Render for View {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
    }
}

fn reference() -> catalog::Model {
    catalog::Model {
        id: "fixture".into(),
        name: "Fixture".into(),
        context: Some(8192),
        output: Some(1024),
        inputs: vec!["text".into(), "image".into()],
        outputs: vec!["text".into()],
        tools: Some(true),
        reasoning: Some(true),
        options: vec![catalog::Reasoning::Effort {
            values: vec![Some("low".into()), Some("high".into())],
        }],
    }
}

fn draft(window: &mut Window, cx: &mut App) -> Draft {
    Draft::discovered(
        0,
        discovery::Model {
            id: "fixture".into(),
            display_name: None,
            context: None,
            output: None,
            capabilities: None,
        },
        None,
        Preset::AzureOpenAi,
        window,
        cx,
    )
}

#[gpui::test]
fn preserves_edited_context(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let (_, visual) = cx.add_window_view(|_, _| View);
    visual.update(|window, cx| {
        let mut draft = draft(window, cx);
        draft
            .context
            .update(cx, |input, cx| input.set_value("32768", window, cx));
        draft.expanded = true;
        let context = draft.context.clone();
        draft.fill(None, Some(&reference()), Preset::AzureOpenAi, window, cx);
        let model = draft.value(cx).unwrap();
        assert_eq!((model.context, model.output), (32768, 1024));
        assert!(model.vision && model.tools && model.reasoning);
        assert!(!model.web_search);
        assert_eq!(model.efforts, [Effort::Low, Effort::High]);
        assert_eq!(model.default_effort, Effort::Low);
        assert!(model.generates.is_empty());
        assert!(draft.expanded);
        assert_eq!(draft.context, context);
    });
}

#[gpui::test]
fn keeps_unreported_options_empty(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let (_, visual) = cx.add_window_view(|_, _| View);
    visual.update(|window, cx| {
        let mut draft = draft(window, cx);
        let mut reference = reference();
        reference.options.clear();
        draft.fill(
            None,
            Some(&reference),
            Preset::CompatibleResponses,
            window,
            cx,
        );
        let model = draft.value(cx).unwrap();
        assert!(model.reasoning);
        assert!(model.efforts.is_empty());
        assert_eq!(model.default_effort, Effort::Default);
    });
}

#[gpui::test]
fn retains_unreported_efforts(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let (_, visual) = cx.add_window_view(|_, _| View);
    visual.update(|window, cx| {
        let mut draft = draft(window, cx);
        draft.model.reasoning = true;
        draft.model.efforts = vec![Effort::High];
        draft.model.default_effort = Effort::High;
        draft.efforts = reasoning::Rows::new(&draft.model, window, cx);
        let mut reference = reference();
        reference.options.clear();
        draft.fill(
            None,
            Some(&reference),
            Preset::CompatibleResponses,
            window,
            cx,
        );
        let model = draft.value(cx).unwrap();
        assert_eq!(model.efforts, [Effort::High]);
        assert_eq!(model.default_effort, Effort::High);
    });
}

#[gpui::test]
fn clears_unreported_reasoning_summary(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let (_, visual) = cx.add_window_view(|_, _| View);
    visual.update(|window, cx| {
        for cached in [false, true] {
            let mut model = Model::example("fixture");
            model.efforts.clear();
            model.default_effort = Effort::Default;
            model.generates = vec![sailry_protocol::media::Generation::Image];
            let mut draft = Draft::new(0, model, window, cx);
            let mut reference = reference();
            reference.reasoning = None;
            reference.options.clear();
            reference.outputs.clear();
            draft.fill(
                None,
                cached.then_some(&reference),
                Preset::CompatibleResponses,
                window,
                cx,
            );
            let model = draft.value(cx).unwrap();
            assert!(!model.reasoning);
            assert!(model.generates.is_empty());
            assert!(model.efforts.is_empty());
            assert_eq!(model.default_effort, Effort::Default);
        }
    });
}

#[gpui::test]
fn preserves_configured_choices(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let (_, visual) = cx.add_window_view(|_, _| View);
    visual.update(|window, cx| {
        for custom in [false, true] {
            let mut model = Model::example("fixture");
            model.efforts = vec![Effort::Low, Effort::High];
            model.default_effort = Effort::High;
            model.custom_efforts = custom;
            let mut draft = Draft::new(0, model, window, cx);
            let native = discovery::Model {
                id: "fixture".into(),
                display_name: None,
                context: None,
                output: None,
                capabilities: Some(discovery::Capabilities {
                    efforts: Some(vec![Effort::Medium]),
                    default_effort: Some(Effort::Medium),
                    ..Default::default()
                }),
            };
            draft.fill(
                Some(&native),
                Some(&reference()),
                Preset::CompatibleResponses,
                window,
                cx,
            );
            let model = draft.value(cx).unwrap();
            assert_eq!(model.efforts, [Effort::Low, Effort::High]);
            assert_eq!(model.default_effort, Effort::High);
            assert_eq!(model.custom_efforts, custom);
            assert_eq!(model.capabilities, native.capabilities);
        }
    });
}

#[gpui::test]
fn preserves_empty_custom_choices(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let (_, visual) = cx.add_window_view(|_, _| View);
    visual.update(|window, cx| {
        let mut model = Model::example("fixture");
        model.efforts.clear();
        model.default_effort = Effort::Default;
        model.custom_efforts = true;
        let mut draft = Draft::new(0, model, window, cx);
        let native = discovery::Model {
            id: "fixture".into(),
            display_name: None,
            context: None,
            output: None,
            capabilities: Some(discovery::Capabilities {
                efforts: Some(vec![Effort::High]),
                default_effort: Some(Effort::High),
                ..Default::default()
            }),
        };
        draft.fill(
            Some(&native),
            Some(&reference()),
            Preset::CompatibleResponses,
            window,
            cx,
        );
        let model = draft.value(cx).unwrap();
        assert!(model.efforts.is_empty());
        assert_eq!(model.default_effort, Effort::Default);
        assert!(model.custom_efforts);
        assert_eq!(model.capabilities, native.capabilities);
    });
}

#[gpui::test]
fn resolves_reported_reasoning(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let (_, visual) = cx.add_window_view(|_, _| View);
    visual.update(|window, cx| {
        for (reported, cached, expected) in [
            (None, None, true),
            (None, Some(false), false),
            (Some(false), Some(true), false),
            (Some(true), Some(false), true),
        ] {
            let mut draft = draft(window, cx);
            let native = discovery::Model {
                id: "fixture".into(),
                display_name: None,
                context: None,
                output: None,
                capabilities: Some(discovery::Capabilities {
                    reasoning: reported,
                    efforts: Some(vec![Effort::High]),
                    default_effort: Some(Effort::High),
                    ..Default::default()
                }),
            };
            let mut reference = reference();
            reference.reasoning = cached;
            draft.fill(
                Some(&native),
                Some(&reference),
                Preset::CompatibleResponses,
                window,
                cx,
            );
            let model = draft.value(cx).unwrap();
            assert_eq!(model.reasoning, expected);
            assert_eq!(model.efforts, [Effort::High]);
            assert_eq!(model.default_effort, Effort::High);
        }
    });
}

#[gpui::test]
fn keeps_reported_limits(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let (_, visual) = cx.add_window_view(|_, _| View);
    visual.update(|window, cx| {
        let mut draft = Draft::discovered(
            0,
            discovery::Model {
                id: "fixture".into(),
                display_name: None,
                context: Some(400_000),
                output: Some(16_384),
                capabilities: Some(discovery::Capabilities {
                    vision: Some(false),
                    tools: Some(false),
                    ..Default::default()
                }),
            },
            None,
            Preset::AzureOpenAi,
            window,
            cx,
        );
        draft.fill(None, Some(&reference()), Preset::AzureOpenAi, window, cx);
        let model = draft.value(cx).unwrap();
        assert_eq!((model.context, model.output), (400_000, 16_384));
        assert!(!model.vision && !model.tools);
        assert!(model.reasoning);
    });
}

#[gpui::test]
fn fills_native_gaps(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let (_, visual) = cx.add_window_view(|_, _| View);
    visual.update(|window, cx| {
        let mut draft = draft(window, cx);
        draft
            .context
            .update(cx, |input, cx| input.set_value("32768", window, cx));
        draft.advanced = true;
        let native = discovery::Model {
            id: "fixture".into(),
            display_name: None,
            context: Some(4096),
            output: Some(128),
            capabilities: Some(discovery::Capabilities {
                vision: Some(false),
                tools: Some(true),
                reasoning: Some(true),
                ..Default::default()
            }),
        };
        draft.fill(
            Some(&native),
            Some(&reference()),
            Preset::CompatibleResponses,
            window,
            cx,
        );
        let model = draft.value(cx).unwrap();
        assert_eq!((model.context, model.output), (32768, 128));
        assert!(!model.vision && model.tools && model.reasoning);
        assert!(draft.advanced);
    });
}

#[gpui::test]
fn preserves_invalid_input(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let (_, visual) = cx.add_window_view(|_, _| View);
    visual.update(|window, cx| {
        let mut draft = draft(window, cx);
        draft
            .context
            .update(cx, |input, cx| input.set_value("-1", window, cx));
        draft.fill(None, Some(&reference()), Preset::AzureOpenAi, window, cx);
        assert_eq!(draft.context.read(cx).value(), "-1");
        assert_eq!(draft.model.output, 16_384);
        assert_eq!(draft.value(cx).unwrap_err(), "provider_limits_invalid");
    });
}

#[gpui::test]
fn hides_capability_controls(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let mut editor = None;
    let (_, visual) = cx.add_window_view(|window, cx| {
        let owner = cx.new(|cx| crate::settings::Workspace::new(window, cx));
        let view = cx.new(|cx| Editor::new(owner, None, window, cx));
        view.update(cx, |editor, cx| {
            let mut draft = Draft::discovered(
                0,
                discovery::Model {
                    id: "fixture".into(),
                    display_name: None,
                    context: None,
                    output: None,
                    capabilities: Some(discovery::Capabilities {
                        vision: Some(false),
                        tools: Some(false),
                        reasoning: Some(false),
                        ..Default::default()
                    }),
                },
                Some(&reference()),
                Preset::AzureOpenAi,
                window,
                cx,
            );
            draft.expanded = true;
            editor.models.push(draft);
            cx.notify();
        });
        editor = Some(view.clone());
        Root::new(view, window, cx)
    });
    let editor = editor.unwrap();
    let handle = visual.update(|window, _| window.window_handle());
    visual.simulate_window_resize(handle, size(px(1280.), px(1000.)));
    visual.update(|window, cx| window.draw(cx).clear(cx));
    assert!(visual.debug_bounds("model-context-input-0").is_some());
    assert!(visual.debug_bounds("model-advanced-0").is_none());
    for selector in [
        "model-capability-0-0",
        "model-capability-0-1",
        "model-capability-0-2",
        "model-capability-0-4",
        "model-capability-0-5",
        "model-capability-0-6",
    ] {
        assert!(visual.debug_bounds(selector).is_none());
    }
    visual.update(|window, cx| {
        editor.update(cx, |editor, cx| {
            let draft = &mut editor.models[0];
            draft.fill(None, Some(&reference()), Preset::AzureOpenAi, window, cx);
            let model = draft.value(cx).unwrap();
            assert!(!model.vision && !model.tools && !model.reasoning);
            assert_eq!((model.context, model.output), (8192, 1024));
            let saved = super::super::super::live::model(&model);
            assert!(!saved.vision && !saved.tools && !saved.reasoning);
            assert_eq!((saved.context, saved.output), (8192, 1024));
            assert_eq!(saved.capabilities, model.capabilities);
            let mut draft = Draft::discovered(
                0,
                discovery::Model {
                    id: "custom-model".into(),
                    display_name: None,
                    context: None,
                    output: None,
                    capabilities: None,
                },
                None,
                Preset::CompatibleResponses,
                window,
                cx,
            );
            draft.expanded = true;
            editor.models[0] = draft;
            cx.notify();
        });
        window.draw(cx).clear(cx);
    });
    let advanced = visual.debug_bounds("model-advanced-0").unwrap();
    visual.simulate_click(advanced.center(), Modifiers::default());
    visual.update(|window, cx| window.draw(cx).clear(cx));
    let add = visual.debug_bounds("effort-add-0").unwrap();
    visual.simulate_click(add.center(), Modifiers::default());
    visual.update(|window, cx| window.draw(cx).clear(cx));
    assert_eq!(
        editor.read_with(visual, |editor, cx| editor.models[0].value(cx).unwrap_err()),
        "provider_effort_invalid"
    );
    visual.simulate_input("high");
    visual.update(|window, cx| {
        window.draw(cx).clear(cx);
        editor.read_with(cx, |editor, cx| {
            let model = editor.models[0].value(cx).unwrap();
            assert_eq!(model.capabilities, None);
            assert_eq!(model.efforts, [Effort::High]);
            assert_eq!(model.default_effort, Effort::High);
            assert!(model.custom_efforts);
        });
    });
}

#[gpui::test]
fn native_efforts_override_reference(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let (_, visual) = cx.add_window_view(|_, _| View);
    visual.update(|window, cx| {
        let mut draft = draft(window, cx);
        let native = discovery::Model {
            id: "fixture".into(),
            display_name: None,
            context: None,
            output: None,
            capabilities: Some(discovery::Capabilities {
                reasoning: Some(true),
                efforts: Some(vec![Effort::High, Effort::XHigh, Effort::Max]),
                default_effort: Some(Effort::XHigh),
                ..Default::default()
            }),
        };
        draft.fill(
            Some(&native),
            Some(&reference()),
            Preset::CompatibleResponses,
            window,
            cx,
        );
        let model = draft.value(cx).unwrap();
        assert_eq!(model.efforts, [Effort::High, Effort::XHigh, Effort::Max]);
        assert_eq!(model.default_effort, Effort::XHigh);
    });
}
