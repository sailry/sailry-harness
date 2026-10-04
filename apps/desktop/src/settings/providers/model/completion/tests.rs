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
fn preserves_manual_fields(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let (_, visual) = cx.add_window_view(|_, _| View);
    visual.update(|window, cx| {
        let mut draft = draft(window, cx);
        draft
            .context
            .update(cx, |input, cx| input.set_value("32768", window, cx));
        draft.model.web_search = true;
        draft.model.generates = vec![sailry_protocol::media::Generation::Image];
        draft.missing.generates = false;
        draft.expanded = true;
        let context = draft.context.clone();
        draft.fill(None, Some(&reference()), Preset::AzureOpenAi, window, cx);
        let model = draft.value(cx).unwrap();
        assert_eq!((model.context, model.output), (32768, 1024));
        assert!(model.vision && model.tools && model.reasoning && model.web_search);
        assert_eq!(model.efforts, [Effort::Low, Effort::High]);
        assert_eq!(model.default_effort, Effort::Low);
        assert_eq!(model.generates, [sailry_protocol::media::Generation::Image]);
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
fn keeps_reported_limits(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let (_, visual) = cx.add_window_view(|_, _| View);
    visual.update(|window, cx| {
        let mut draft = Draft::discovered(
            0,
            discovery::Model {
                id: "fixture".into(),
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
        draft.missing.tools = false;
        draft.model.tools = false;
        draft.advanced = true;
        let native = discovery::Model {
            id: "fixture".into(),
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
        assert!(!model.vision && !model.tools && model.reasoning);
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
fn preserves_disabled_capabilities(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let mut editor = None;
    let (_, visual) = cx.add_window_view(|window, cx| {
        let owner = cx.new(|cx| crate::settings::Workspace::new(window, cx));
        let view = cx.new(|cx| Editor::new(owner, None, window, cx));
        view.update(cx, |editor, cx| {
            editor.add_model(window, cx);
            editor.models[0]
                .id
                .update(cx, |input, cx| input.set_value("fixture", window, cx));
        });
        editor = Some(view.clone());
        Root::new(view, window, cx)
    });
    let editor = editor.unwrap();
    let handle = visual.update(|window, _| window.window_handle());
    visual.simulate_window_resize(handle, size(px(1280.), px(1000.)));
    visual.update(|window, cx| window.draw(cx).clear(cx));
    let advanced = visual.debug_bounds("model-advanced-0").unwrap();
    visual.simulate_click(advanced.center(), Modifiers::default());
    for selector in [
        "model-capability-0-0",
        "model-capability-0-1",
        "model-capability-0-2",
    ] {
        for _ in 0..2 {
            visual.update(|window, cx| window.draw(cx).clear(cx));
            let bounds = visual.debug_bounds(selector).unwrap();
            visual.simulate_click(bounds.center(), Modifiers::default());
        }
    }
    visual.update(|window, cx| {
        editor.update(cx, |editor, cx| {
            let draft = &mut editor.models[0];
            draft.fill(None, Some(&reference()), Preset::AzureOpenAi, window, cx);
            let model = draft.value(cx).unwrap();
            assert!(!model.vision && !model.tools && !model.reasoning);
            assert_eq!((model.context, model.output), (8192, 1024));
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
