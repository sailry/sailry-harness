use super::*;
use core::prelude::v1::test;
use std::collections::BTreeMap;

fn catalog(api: &str) -> (ProviderId, Models) {
    let provider = ProviderId::new();
    let record = serde_json::from_value(serde_json::json!({
        "id": provider,
        "revision": 1,
        "name": "Channel",
        "api": api,
        "authentication": "api_key",
        "endpoint": "https://example.test/v1",
        "enabled": true,
        "default_model": "first"
    }))
    .unwrap();
    let mut channel = crate::settings::channel(0, &record);
    let mut first = Model::example("first");
    first.capabilities = Some(
        serde_json::from_value(serde_json::json!({
            "features": [{"id": "speed", "options": [{"id": "fast"}]}],
            "vision": null, "tools": null, "reasoning": null,
            "efforts": null, "default_effort": null
        }))
        .unwrap(),
    );
    let mut second = first.clone();
    second.id = "second".into();
    second.capabilities.as_mut().unwrap().features[0].options[0].id = "ultrafast".into();
    let mut plain = Model::example("plain");
    plain.reasoning = false;
    channel.models = vec![first, second, plain];
    (
        provider,
        Models {
            channels: vec![channel],
            providers: vec![(0, provider)],
            binding: None,
        },
    )
}

#[gpui::test]
fn picking_preserves_supported_choices(cx: &mut TestAppContext) {
    cx.update(|cx| {
        let fast = BTreeMap::from([("speed".to_owned(), "fast".to_owned())]);
        let mut extra = fast.clone();
        extra.insert("removed".into(), "choice".into());
        for (case, api, model, features, expected) in [
            (
                "supported option",
                "responses",
                "first",
                Some(fast.clone()),
                Some(fast.clone()),
            ),
            (
                "removed option",
                "responses",
                "second",
                Some(fast.clone()),
                Some(BTreeMap::new()),
            ),
            (
                "removed feature",
                "responses",
                "plain",
                Some(fast.clone()),
                Some(BTreeMap::new()),
            ),
            ("inherited choices", "responses", "first", None, None),
            (
                "explicitly disabled",
                "responses",
                "first",
                Some(BTreeMap::new()),
                Some(BTreeMap::new()),
            ),
            (
                "unsupported api",
                "anthropic",
                "first",
                Some(fast.clone()),
                Some(BTreeMap::new()),
            ),
            (
                "mixed choices",
                "responses",
                "first",
                Some(extra),
                Some(fast),
            ),
        ] {
            let (provider, models) = catalog(api);
            let field = cx.new(|cx| {
                ModelField::new(
                    models,
                    Some(role::Model {
                        provider,
                        model: "first".into(),
                        effort: Some(Effort::Low),
                        features: features.clone(),
                    }),
                    "teams_follow_lead",
                    cx,
                )
            });
            field.update(cx, |field, cx| {
                field.pick(
                    &Picked {
                        selection: Selection {
                            channel: 0,
                            model: model.into(),
                        },
                        effort: Effort::High,
                    },
                    cx,
                );
                let selected = field.value().unwrap();
                assert_eq!(selected.provider, provider, "{case}");
                assert_eq!(selected.model, model, "{case}");
                assert_eq!(
                    selected.effort,
                    (model != "plain").then_some(Effort::High),
                    "{case}"
                );
                assert_eq!(selected.features, expected, "{case}");
            });
        }
    });
}
