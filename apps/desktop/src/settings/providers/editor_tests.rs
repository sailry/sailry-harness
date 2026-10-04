use super::*;
use core::prelude::v1::test;

#[gpui::test]
fn creates_outside_the_filter(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let mut state = None;
    let (_, visual) = cx.add_window_view(|window, cx| {
        let owner = cx.new(|cx| Workspace::new(window, cx));
        owner.update(cx, |owner, _| {
            owner.providers.category = super::super::data::Category::Other;
        });
        let editor = cx.new(|cx| Editor::new(owner.clone(), None, window, cx));
        state = Some((owner, editor.clone()));
        Root::new(editor, window, cx)
    });
    let (owner, editor) = state.unwrap();
    visual.update(|window, cx| {
        editor.update(cx, |editor, cx| {
            assert_eq!(editor.preset, Preset::OpenAi);
            editor
                .name
                .update(cx, |input, cx| input.set_value("Relay", window, cx));
            editor
                .credential
                .update(cx, |input, cx| input.set_value("fixture-key", window, cx));
            editor.add_model(window, cx);
        });
        window.draw(cx).clear(cx);
    });
    let bounds = visual.debug_bounds("provider-type").unwrap();
    visual.simulate_click(
        point(bounds.left() + px(40.), bounds.center().y),
        Modifiers::default(),
    );
    visual.update(|window, cx| window.draw(cx).clear(cx));
    visual.simulate_mouse_move(point(px(1.), px(1.)), None, Modifiers::default());
    let group = visual
        .debug_bounds("provider-kind-brand-provider_anthropic")
        .unwrap();
    visual.simulate_click(group.center(), Modifiers::default());
    visual.update(|window, cx| window.draw(cx).clear(cx));
    visual.simulate_keystrokes("down down enter");
    visual.update(|window, cx| {
        editor.update(cx, |editor, cx| {
            assert_eq!(editor.preset, Preset::CompatibleAnthropic);
            assert_eq!(editor.name.read(cx).value(), "Relay");
            assert!(editor.credential.read(cx).value().is_empty());
            assert!(editor.models.is_empty());
            editor.endpoint.update(cx, |input, cx| {
                input.set_value("https://example.invalid", window, cx)
            });
            editor.credential.update(cx, |input, cx| {
                input.set_value("new-fixture-key", window, cx)
            });
            editor.discover(window, cx);
            editor.save(window, cx);
            assert!(editor.error.is_none());
        });
        assert_eq!(
            owner.read(cx).providers.category,
            super::super::data::Category::Anthropic
        );
        assert!(owner.read(cx).providers.channels.iter().any(
            |channel| channel.name == "Relay" && channel.preset == Preset::CompatibleAnthropic
        ));
    });
}

#[gpui::test]
fn discovery_preserves_edits(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let (_, visual) = cx.add_window_view(|window, cx| {
        let workspace = cx.new(|cx| Workspace::new(window, cx));
        Root::new(workspace, window, cx)
    });
    visual.update(|window, cx| {
        let workspace = cx.new(|cx| Workspace::new(window, cx));
        let editor = cx.new(|cx| Editor::new(workspace.clone(), Some(0), window, cx));
        editor.update(cx, |editor, cx| {
            editor.add_model(window, cx);
            editor.models[1]
                .id
                .update(cx, |input, cx| input.set_value("custom-model", window, cx));
            editor.default_model = 1;
            editor.discover(window, cx);
            editor.discover(window, cx);
            assert_eq!(editor.models.len(), 3);
            let channel = editor.channel(cx).unwrap();
            assert_eq!(channel.default_model, "custom-model");
            assert_eq!(workspace.read(cx).providers.channels[0].models.len(), 1);
        });
    });
}

#[gpui::test]
fn validates_and_saves_only_preview(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let (_, visual) = cx.add_window_view(|window, cx| {
        let workspace = cx.new(|cx| Workspace::new(window, cx));
        Root::new(workspace, window, cx)
    });
    visual.update(|window, cx| {
        let workspace = cx.new(|cx| Workspace::new(window, cx));
        let editor = cx.new(|cx| Editor::new(workspace.clone(), None, window, cx));
        editor.update(cx, |editor, cx| {
            assert!(editor.name.read(cx).value().is_empty());
            assert_eq!(editor.channel(cx).err(), Some("provider_name_required"));
            editor.name.update(cx, |input, cx| {
                input.set_value("Preview channel", window, cx)
            });
            assert_eq!(
                editor.channel(cx).err(),
                Some("provider_preview_key_required")
            );
            editor.credential.update(cx, |input, cx| {
                input.set_value("dummy-preview-key", window, cx)
            });
            assert_eq!(editor.channel(cx).err(), Some("provider_model_required"));
            editor.discover(window, cx);
            editor.models[1].id.update(cx, |input, cx| {
                input.set_value("preview-text-1", window, cx)
            });
            assert_eq!(editor.channel(cx).err(), Some("provider_model_duplicate"));
            editor.models[1]
                .id
                .update(cx, |input, cx| input.set_value("custom-model", window, cx));
            let channel = editor.channel(cx).unwrap();
            assert!(channel.credential_configured);
            workspace.update(cx, |workspace, _| workspace.providers.save(None, channel));
            assert_eq!(workspace.read(cx).providers.channels.len(), 2);
            editor.select_preset(Preset::ChatGpt, window, cx);
            assert_eq!(editor.name.read(cx).value(), "Preview channel");
            assert!(editor.credential.read(cx).value().is_empty());
            assert!(editor.models.is_empty());
        });
    });
}

#[gpui::test]
fn names_are_user_owned(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let (_, visual) = cx.add_window_view(|window, cx| {
        let workspace = cx.new(|cx| Workspace::new(window, cx));
        Root::new(workspace, window, cx)
    });
    visual.update(|window, cx| {
        let owner = cx.new(|cx| Workspace::new(window, cx));
        for category in super::super::data::Category::ALL {
            owner.update(cx, |owner, _| owner.providers.category = category);
            let editor = cx.new(|cx| Editor::new(owner.clone(), None, window, cx));
            editor.update(cx, |editor, cx| {
                assert_eq!(editor.preset, Preset::OpenAi);
                assert!(editor.name.read(cx).value().is_empty());
                assert_eq!(
                    editor.name.read(cx).presentation().placeholder().as_ref(),
                    tr("provider_name_hint").as_ref()
                );
                for preset in Preset::all() {
                    editor.select_preset(preset, window, cx);
                    assert!(editor.name.read(cx).value().is_empty());
                }
            });
        }
        let saved = owner.read(cx).providers.channels[0].name.clone();
        let editor = cx.new(|cx| Editor::new(owner, Some(0), window, cx));
        editor.update(cx, |editor, cx| {
            assert_eq!(editor.name.read(cx).value(), saved);
            for preset in Preset::all() {
                editor.select_preset(preset, window, cx);
                assert_eq!(editor.name.read(cx).value(), saved);
            }
        });
    });
}

#[test]
fn presets() {
    assert_eq!(Preset::all().count(), 35);
    assert_eq!(Preset::all().filter(|p| p.oauth()).count(), 3);
    assert_eq!(Preset::all().filter(|p| p.custom_endpoint()).count(), 6);
}

#[test]
fn english_names() {
    let keys = super::super::data::Family::all()
        .map(|family| family.key())
        .chain(Preset::all().map(|preset| preset.key()));
    for key in keys {
        let label = tr(key);
        assert!(label.is_ascii(), "provider name must remain English: {key}");
        assert_ne!(label.as_ref(), key, "missing provider name: {key}");
    }
    for (preset, label) in [
        (Preset::CompatibleResponses, "OpenAI Compatible"),
        (Preset::CompatibleAnthropic, "Anthropic Compatible"),
        (Preset::CompatibleGemini, "Gemini Compatible"),
    ] {
        assert_eq!(tr(preset.key()).as_ref(), label);
    }
}

#[test]
fn channel_kinds() {
    let kinds = Preset::all()
        .filter(|preset| preset.kind() == *preset)
        .collect::<Vec<_>>();
    assert_eq!(kinds.len(), 31);
    assert_eq!(
        kinds
            .iter()
            .map(|preset| tr(preset.key()))
            .collect::<HashSet<_>>()
            .len(),
        31
    );
    for preset in Preset::all() {
        for variant in preset.protocols() {
            assert_eq!(variant.kind(), preset.kind());
            assert_eq!(variant.authentication(), preset.authentication());
        }
    }
    assert_eq!(Preset::CompatibleChat.kind(), Preset::CompatibleResponses);
    assert_eq!(Preset::CopilotResponses.kind(), Preset::Copilot);
}

#[gpui::test]
fn selects_types_and_protocols(cx: &mut TestAppContext) {
    fn draw(cx: &mut VisualTestContext) {
        cx.run_until_parked();
        cx.update(|window, cx| {
            window.refresh();
            window.draw(cx).clear(cx);
        });
    }
    fn choose(cx: &mut VisualTestContext, selector: &'static str, index: usize) {
        draw(cx);
        let bounds = cx.debug_bounds(selector).unwrap();
        cx.simulate_click(bounds.center(), Modifiers::default());
        draw(cx);
        cx.simulate_mouse_move(point(px(1.), px(1.)), None, Modifiers::default());
        for _ in 0..=index {
            cx.simulate_keystrokes("down");
            draw(cx);
        }
        cx.simulate_keystrokes("enter");
        draw(cx);
    }
    fn choose_kind(cx: &mut VisualTestContext, selector: &'static str) {
        let trigger = cx.debug_bounds("provider-type").unwrap();
        cx.simulate_click(trigger.center(), Modifiers::default());
        draw(cx);
        let row = cx.debug_bounds(selector).unwrap();
        cx.simulate_click(row.center(), Modifiers::default());
        draw(cx);
        assert!(cx.debug_bounds("provider-kind-picker").is_none());
    }
    cx.update(|cx| {
        gpui_kit::init(cx);
        crate::theme::init(cx);
    });
    let mut editor = None;
    let (_, visual) = cx.add_window_view(|window, cx| {
        let workspace = cx.new(|cx| Workspace::new(window, cx));
        let view = cx.new(|cx| Editor::new(workspace, None, window, cx));
        editor = Some(view.clone());
        Root::new(view, window, cx)
    });
    let editor = editor.unwrap();
    for mode in [ThemeMode::Light, ThemeMode::Dark] {
        for width in [760., 1280.] {
            let handle = visual.update(|window, cx| {
                crate::theme::select(Some(mode), window, cx);
                window.window_handle()
            });
            visual.simulate_window_resize(handle, size(px(width), px(820.)));
            draw(visual);
            assert!(visual.debug_bounds("provider-api").is_none());
            choose_kind(visual, "provider-kind-preset_compatible");
            assert_eq!(
                editor.read_with(visual, |editor, _| editor.preset),
                Preset::CompatibleResponses
            );
            let kind = visual.debug_bounds("provider-type").unwrap();
            let api = visual.debug_bounds("provider-api").unwrap();
            let row = visual.debug_bounds("provider-kind-row").unwrap();
            assert!(kind.right() < api.left() && api.right() <= px(width));
            assert_eq!(kind.center().y, api.center().y);
            assert!((api.right() - row.right()).abs() < px(1.));
            assert!((kind.left() - row.left()).abs() < px(1.));
            visual.update(|window, cx| {
                editor.update(cx, |editor, cx| {
                    editor
                        .name
                        .update(cx, |input, cx| input.set_value("My gateway", window, cx));
                    editor.endpoint.update(cx, |input, cx| {
                        input.set_value("https://example.com/v1", window, cx)
                    });
                    editor
                        .credential
                        .update(cx, |input, cx| input.set_value("synthetic-key", window, cx));
                })
            });
            choose(visual, "provider-api", 1);
            choose_kind(visual, "provider-kind-preset_compatible");
            editor.read_with(visual, |editor, cx| {
                assert_eq!(editor.preset, Preset::CompatibleChat);
                assert_eq!(editor.name.read(cx).value(), "My gateway");
                assert_eq!(editor.endpoint.read(cx).value(), "https://example.com/v1");
                assert_eq!(editor.credential.read(cx).value(), "synthetic-key");
            });
            choose_kind(visual, "provider-kind-preset_chatgpt");
            assert_eq!(
                editor.read_with(visual, |editor, _| editor.preset),
                Preset::ChatGpt
            );
            assert!(visual.debug_bounds("provider-oauth-note").is_some());
            assert_eq!(
                editor.read_with(visual, |editor, cx| editor.name.read(cx).value()),
                "My gateway"
            );
            assert!(visual.debug_bounds("provider-api").is_none());
            choose_kind(visual, "provider-kind-preset_openai");
            assert_eq!(
                editor.read_with(visual, |editor, _| editor.preset),
                Preset::OpenAi
            );
            assert!(visual.debug_bounds("provider-api").is_none());
            assert!(visual.debug_bounds("provider-oauth-note").is_none());
        }
    }
}

#[gpui::test]
fn model_rows_and_toasts(cx: &mut TestAppContext) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        crate::theme::init(cx);
    });
    let mut editor = None;
    let (_, visual) = cx.add_window_view(|window, cx| {
        let workspace = cx.new(|cx| Workspace::new(window, cx));
        let view = cx.new(|cx| Editor::new(workspace, Some(0), window, cx));
        view.update(cx, |editor, cx| {
            editor.name.update(cx, |input, cx| {
                input.set_value("Preview channel", window, cx)
            });
            editor.configured = true;
            editor.discover(window, cx);
        });
        editor = Some(view.clone());
        Root::new(view, window, cx)
    });
    let editor = editor.unwrap();
    let draw = |visual: &mut VisualTestContext| {
        visual.run_until_parked();
        visual.update(|window, cx| window.draw(cx).clear(cx));
    };
    draw(visual);
    assert!(visual.debug_bounds("provider-clear-key").is_none());
    assert!(visual.debug_bounds("provider-validate").is_none());
    assert!(visual.debug_bounds("provider-validation").is_none());
    assert!(visual.debug_bounds("provider-discover").is_some());
    assert_eq!(
        visual.update(|window, cx| window.notifications(cx).len()),
        0
    );
    let trigger = visual.debug_bounds("model-expand-0").unwrap();
    let title = visual.debug_bounds("model-title-0").unwrap();
    assert!(title.left() - trigger.left() <= px(32.));
    visual.simulate_click(title.center(), Modifiers::default());
    draw(visual);
    assert!(editor.read_with(visual, |editor, _| editor.models[0].expanded));
    assert!(visual.debug_bounds("model-capability-0-0").is_none());
    let advanced = visual.debug_bounds("model-advanced-0").unwrap();
    visual.simulate_click(advanced.center(), Modifiers::default());
    draw(visual);
    assert!(editor.read_with(visual, |editor, _| editor.models[0].advanced));
    assert!(visual.debug_bounds("model-capability-0-0").is_some());
    let advanced = visual.debug_bounds("model-advanced-0").unwrap();
    visual.simulate_click(advanced.center(), Modifiers::default());
    draw(visual);
    assert!(visual.debug_bounds("model-capability-0-0").is_none());
    let trigger = visual.debug_bounds("model-expand-0").unwrap();
    visual.simulate_click(trigger.center(), Modifiers::default());
    draw(visual);
    assert!(!editor.read_with(visual, |editor, _| editor.models[0].expanded));
    let choose = visual.debug_bounds("model-default-1").unwrap();
    visual.simulate_click(choose.center(), Modifiers::default());
    draw(visual);
    assert_eq!(
        editor.read_with(visual, |editor, _| editor.default_model),
        1
    );
    visual.update(|window, cx| {
        editor.update(cx, |editor, cx| {
            editor.models[0]
                .id
                .update(cx, |input, cx| input.set_value("", window, cx));
            editor.save(window, cx);
        });
    });
    draw(visual);
    assert_eq!(
        editor.read_with(visual, |editor, _| editor.error),
        Some("provider_model_required")
    );
    assert!(visual.debug_bounds("provider-error").is_none());
    assert_eq!(
        visual.update(|window, cx| window.notifications(cx).len()),
        1
    );
    visual.update(|window, cx| editor.update(cx, |editor, cx| editor.save(window, cx)));
    draw(visual);
    assert_eq!(
        visual.update(|window, cx| window.notifications(cx).len()),
        1
    );
}
