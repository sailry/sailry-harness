use super::*;
use sailry_protocol::Effort;

fn draw(cx: &mut VisualTestContext) {
    cx.run_until_parked();
    cx.update(|window, cx| {
        window.refresh();
        window.draw(cx).clear(cx);
    });
}

#[gpui::test]
fn fetches_and_saves(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    cx.update(gpui_kit::init);
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let catalog = runtime.block_on(Server::start(ModelApi::Anthropic, |_| {
        let mut metadata = catalog::metadata();
        metadata["anthropic"]["models"]["known"]["reasoning_options"] =
            json!([{"type":"effort","values":["high"]}]);
        Reply::Json(metadata)
    }));
    let server = runtime.block_on(Server::start(ModelApi::ChatCompletions, |_| {
        Reply::Json(json!({"data":[{"id":"known", "supported_reasoning_levels":[
            {"effort":"low"}, {"effort":"medium"}, {"effort":"high"}, {"effort":"xhigh"}],
            "default_reasoning_level":"high"}]}))
    }));
    for remote in [false, true] {
        let fixture = Fixture::with_catalog(remote, Some(&catalog.endpoint));
        fixture.execute(Command::RefreshModelCatalog);
        let mut editor = None;
        let (_, visual) = cx.add_window_view(|window, cx| {
            let owner = fixture.owner(window, cx);
            let view = cx.new(|cx| Editor::new(owner, None, window, cx));
            view.update(cx, |editor, cx| {
                editor.select_preset(Preset::CompatibleChat, window, cx);
                editor.name.update(cx, |input, cx| {
                    input.set_value("Reasoning fixture", window, cx)
                });
                editor.endpoint.update(cx, |input, cx| {
                    input.set_value(server.endpoint.clone(), window, cx)
                });
                editor.credential.update(cx, |input, cx| {
                    input.set_value("isolated-reasoning-key", window, cx)
                });
                let model = super::super::super::super::data::Model {
                    efforts: vec![Effort::Default],
                    custom_efforts: false,
                    default_effort: Effort::Default,
                    ..super::super::super::super::data::Model::example("known")
                };
                editor.models.push(Draft::new(0, model, window, cx));
                editor.next_model = 1;
                editor.models[0].expanded = true;
                editor.models[0].advanced = true;
                editor.discover(window, cx);
            });
            editor = Some(view.clone());
            Root::new(view, window, cx)
        });
        let editor = editor.unwrap();
        wait(visual, |cx| editor.read(cx).probe.is_none());
        draw(visual);
        let model = editor.read_with(visual, |editor, cx| editor.models[0].value(cx).unwrap());
        assert_eq!(
            model.efforts,
            [Effort::Low, Effort::Medium, Effort::High, Effort::XHigh]
        );
        assert_eq!(model.default_effort, Effort::High);
        visual.update(|_, cx| {
            editor.update(cx, |editor, cx| {
                editor.scroll.set_offset(point(px(0.), px(-5000.)));
                cx.notify();
            })
        });
        draw(visual);
        let add = visual.debug_bounds("effort-add-0").unwrap();
        visual.simulate_click(add.center(), Modifiers::default());
        draw(visual);
        assert!(editor.read_with(visual, |editor, cx| editor.models[0].value(cx).is_err()));
        visual.simulate_input("max");
        draw(visual);
        assert_eq!(
            editor.read_with(visual, |editor, cx| editor.models[0]
                .value(cx)
                .unwrap()
                .efforts
                .last()
                .copied()),
            Some(Effort::Max)
        );
        visual.update(|_, cx| {
            editor.update(cx, |editor, cx| {
                editor.scroll.set_offset(point(px(0.), px(-5000.)));
                cx.notify();
            })
        });
        draw(visual);
        let default = visual.debug_bounds("effort-default-0-4").unwrap();
        visual.simulate_click(default.center(), Modifiers::default());
        draw(visual);
        let remove = visual.debug_bounds("effort-remove-0-1").unwrap();
        visual.simulate_click(remove.center(), Modifiers::default());
        draw(visual);
        let custom = editor.read_with(visual, |editor, cx| editor.models[0].value(cx).unwrap());
        assert!(custom.custom_efforts);
        assert_eq!(custom.default_effort, Effort::Max);
        assert!(!custom.efforts.contains(&Effort::Medium));
        visual.update(|window, cx| editor.update(cx, |editor, cx| editor.discover(window, cx)));
        wait(visual, |cx| editor.read(cx).probe.is_none());
        assert_eq!(
            editor.read_with(visual, |editor, cx| editor.models[0].value(cx).unwrap()),
            custom
        );
        visual.update(|window, cx| editor.update(cx, |editor, cx| editor.save(window, cx)));
        wait(visual, |cx| !editor.read(cx).pending);
        assert_eq!(editor.read_with(visual, |editor, _| editor.error), None);
        let Output::Providers(providers) = fixture.execute(Command::ListProviders) else {
            panic!("providers expected")
        };
        assert_eq!(providers[0].models[0].efforts, custom.efforts);
        assert_eq!(providers[0].models[0].default_effort, custom.default_effort);
        assert!(providers[0].models[0].custom_efforts);
        visual.update(|window, _| window.remove_window());
        fixture.close();
    }
}
