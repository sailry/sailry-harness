use super::*;

fn package(fixture: &Fixture, settings: bool) {
    fixture.package("1.0.0");
    let root = fixture.directory.path().join("project/package");
    let mut extension = json!({"api_version":"v1", "actions":[]});
    if settings {
        extension["settings_schema"] = json!("dev.sailry.platform/settings.json");
        std::fs::write(
            root.join("dev.sailry.platform/settings.json"),
            json!({
                "$schema":sailry_protocol::plugin::settings::SCHEMA,
                "type":"object", "additionalProperties":false,
                "properties":{"ai_model":{"type":"string", "minLength":1, "x-sailry-model":true},
                    "ai_model_effort":{"type":"string","x-sailry-model-effort":"ai_model","default":""}},
                "required":["ai_model"],
                "x-sailry-locales":{"zh-CN":{"ai_model":"AI 模型"}}
            })
            .to_string(),
        )
        .unwrap();
    }
    std::fs::write(root.join("plugin.json"), json!({
        "$schema":"https://agent-plugins.org/schemas/1.0.0/plugin.schema.json", "name":"example", "version":"1.0.0",
        "extensions":{"dev.sailry.platform":extension}
    }).to_string()).unwrap();
    fixture.execute(Command::InstallPlugin {
        worktree: fixture.worktree,
        path: "package".into(),
        name: "example".into(),
        expected_revision: 0,
    });
}

#[gpui::test]
fn selects_a_concrete_model(cx: &mut TestAppContext) {
    init(cx);
    for remote in [false, true] {
        let fixture = Fixture::new(remote);
        package(&fixture, true);
        let provider = sailry_protocol::conversation::Provider {
            id: sailry_protocol::ProviderId::new(),
            revision: 0,
            name: "Fixture provider".into(),
            api: sailry_protocol::conversation::ModelApi::Responses,
            authentication: sailry_protocol::Authentication::ApiKey,
            endpoint: "http://127.0.0.1:12345/v1".into(),
            enabled: true,
            credential: None,
            default_model: "fixture".into(),
            options: None,
            oauth: None,
            models: vec![sailry_protocol::conversation::Model {
                id: "fixture".into(),
                context: 4096,
                output: 128,
                vision: false,
                tools: false,
                reasoning: true,
                web_search: false,
                generates: vec![],
                efforts: vec![sailry_protocol::Effort::XHigh, sailry_protocol::Effort::Max],
                custom_efforts: false,
                default_effort: sailry_protocol::Effort::XHigh,
            }],
        };
        let selected = format!("{}/fixture", provider.id);
        fixture.execute(Command::PutProvider {
            provider,
            expected_revision: 0,
        });
        let (owner, visual) = fixture.mount(cx);
        let editor = open(&fixture, &owner, visual);
        assert_eq!(editor.read_with(visual, |editor, _| editor.fields.len()), 2);
        assert!(editor.read_with(visual, |editor, cx| editor.prepare(cx).is_err()));
        assert!(visual.debug_bounds("plugin-model-type-ai_model").is_none());
        tap(visual, "plugin-model-choice-ai_model");
        tap(
            visual,
            Box::leak(format!("composer-model-option-0-{selected}").into_boxed_str()),
        );
        visual.update(|_, cx| {
            let editor = editor.read(cx);
            let field = editor.fields.iter().find(|field| field.read(cx).name == "ai_model_effort").unwrap();
            assert!(matches!(field.read(cx).value(cx).unwrap(), fields::Value::Public(Some(value)) if value == json!("")));
        });
        next(visual, "plugin-setting-ai_model_effort");
        tap(visual, "plugin-settings-save");
        shown(visual, "plugin-settings-form", false);
        assert_eq!(state(&fixture).values["ai_model"], selected);
        assert_eq!(state(&fixture).values["ai_model_effort"], "xhigh");
        assert!(state(&fixture).ready);
        let saved = state(&fixture);
        let mut invalid = saved.values.clone();
        invalid.insert("ai_model_effort".into(), json!("low"));
        let request = fixture.client.prepare(Command::SavePluginSettings {
            package: saved.package,
            values: invalid,
            secrets: Default::default(),
        });
        assert_eq!(
            fixture
                .runtime
                .block_on(fixture.client.execute(request))
                .unwrap_err()
                .code,
            ErrorCode::InvalidRequest
        );
        assert_eq!(state(&fixture).values["ai_model_effort"], "xhigh");

        let Output::Providers(mut providers) = fixture.execute(Command::ListProviders) else {
            panic!("providers expected")
        };
        let mut provider = providers.remove(0);
        provider.enabled = false;
        fixture.execute(Command::PutProvider {
            expected_revision: provider.revision,
            provider,
        });
        let editor = open(&fixture, &owner, visual);
        tap(visual, "plugin-settings-save");
        wait(visual, |cx| {
            editor.read(cx).error == Some("plugins_settings_model_unavailable")
        });
        assert_eq!(state(&fixture).values["ai_model"], selected);
        tap(visual, "plugin-settings-cancel");
        fixture.close(visual);
    }
}

#[gpui::test]
fn leaves_configuration_closed_without_settings(cx: &mut TestAppContext) {
    init(cx);
    for remote in [false, true] {
        let fixture = Fixture::new(remote);
        package(&fixture, false);
        let (owner, visual) = fixture.mount(cx);
        wait(visual, |cx| {
            owner
                .read(cx)
                .plugin_catalog
                .metadata
                .as_ref()
                .is_some_and(|metadata| metadata.read(cx).entries.contains_key("example"))
        });
        crate::settings::plugins::test_support::menu(visual, "plugin-menu-example", 0);
        shown(visual, "plugin-settings-form", false);
        fixture.close(visual);
    }
}
