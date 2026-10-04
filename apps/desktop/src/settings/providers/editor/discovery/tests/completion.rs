use super::*;
use sailry_protocol::Effort;

#[gpui::test]
fn preserves_manual_models(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    cx.update(gpui_kit::init);
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let catalog = runtime.block_on(Server::start(ModelApi::Anthropic, |_| {
        Reply::Json(catalog::metadata())
    }));
    for remote in [false, true] {
        for preset in [
            Preset::AzureOpenAi,
            Preset::AzureAi,
            Preset::BedrockIam,
            Preset::VertexAdc,
        ] {
            let fixture = Fixture::with_catalog(remote, Some(&catalog.endpoint));
            fixture.execute(Command::RefreshModelCatalog);
            let manual = super::super::super::super::data::Model {
                id: "known".into(),
                context: 65536,
                output: 2048,
                vision: false,
                tools: false,
                reasoning: true,
                web_search: false,
                generates: vec![sailry_protocol::media::Generation::Video],
                efforts: vec![Effort::Default],
                custom_efforts: false,
                default_effort: Effort::Default,
            };
            let mut editor = None;
            let (_, visual) = cx.add_window_view(|window, cx| {
                let owner = fixture.owner(window, cx);
                let view = cx.new(|cx| Editor::new(owner, None, window, cx));
                view.update(cx, |editor, cx| {
                    editor.select_preset(preset, window, cx);
                    editor.add_model(window, cx);
                    editor.models[0] = Draft::new(0, manual.clone(), window, cx);
                    editor.add_model(window, cx);
                    editor.models[1]
                        .id
                        .update(cx, |input, cx| input.set_value("unavailable", window, cx));
                    editor.default_model = 1;
                    editor.discover(window, cx);
                });
                editor = Some(view.clone());
                Root::new(view, window, cx)
            });
            let editor = editor.unwrap();
            wait(visual, |cx| editor.read(cx).probe.is_none());
            editor.read_with(visual, |editor, cx| {
                assert_eq!(editor.error, None);
                let mut expected = manual.clone();
                expected.efforts = [Effort::Low, Effort::Medium, Effort::High, Effort::XHigh]
                    .into_iter()
                    .filter(|effort| effort.validate(preset.api(), expected.output).is_ok())
                    .collect();
                expected.default_effort = Effort::initial(&expected.efforts, Effort::Default);
                assert_eq!(editor.models[0].value(cx).unwrap(), expected);
                let completed = editor.models[1].value(cx).unwrap();
                assert_eq!((completed.context, completed.output), (4096, 128));
                assert_eq!(editor.default_model, 1);
            });
            visual.update(|window, cx| {
                editor.update(cx, |editor, cx| editor.close(window, cx));
                window.remove_window();
            });
            fixture.close();
        }
    }
}

#[gpui::test]
fn keeps_late_edits(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    cx.update(gpui_kit::init);
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let catalog = runtime.block_on(Server::start(ModelApi::Anthropic, |_| {
        Reply::Json(catalog::metadata())
    }));
    for remote in [false, true] {
        let fixture = Fixture::with_catalog(remote, Some(&catalog.endpoint));
        fixture.execute(Command::RefreshModelCatalog);
        let mut editor = None;
        let (_, visual) = cx.add_window_view(|window, cx| {
            let owner = fixture.owner(window, cx);
            let view = cx.new(|cx| Editor::new(owner, None, window, cx));
            view.update(cx, |editor, cx| {
                editor.select_preset(Preset::AzureOpenAi, window, cx);
                editor.add_model(window, cx);
                editor.models[0]
                    .id
                    .update(cx, |input, cx| input.set_value("known", window, cx));
                editor.discover(window, cx);
                editor.models[0]
                    .id
                    .update(cx, |input, cx| input.set_value("edited", window, cx));
                editor.add_model(window, cx);
                editor.models[1]
                    .id
                    .update(cx, |input, cx| input.set_value("unavailable", window, cx));
                editor.default_model = 1;
            });
            editor = Some(view.clone());
            Root::new(view, window, cx)
        });
        let editor = editor.unwrap();
        wait(visual, |cx| editor.read(cx).probe.is_none());
        editor.read_with(visual, |editor, cx| {
            assert_eq!(editor.error, None);
            assert_eq!(editor.models.len(), 2);
            assert_eq!(editor.default_model, 1);
            assert_eq!(editor.models[0].value(cx).unwrap().id, "edited");
            for draft in &editor.models {
                let model = draft.value(cx).unwrap();
                assert_eq!((model.context, model.output), (200_000, 16_384));
                assert!(!model.vision && !model.tools && !model.reasoning);
            }
        });
        visual.update(|window, cx| {
            editor.update(cx, |editor, cx| editor.close(window, cx));
            window.remove_window();
        });
        fixture.close();
    }
}
