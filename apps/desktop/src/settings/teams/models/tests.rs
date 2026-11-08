use super::*;
use core::prelude::v1::test;
use std::collections::BTreeMap;

#[gpui::test]
fn picking_keeps_effort_and_features(cx: &mut TestAppContext) {
    cx.update(|cx| {
        let provider = ProviderId::new();
        let record = serde_json::from_value(serde_json::json!({
            "id": provider,
            "revision": 1,
            "name": "Channel",
            "api": "responses",
            "authentication": "api_key",
            "endpoint": "https://example.test/v1",
            "enabled": true,
            "default_model": "first"
        }))
        .unwrap();
        let mut channel = crate::settings::channel(0, &record);
        channel.models = vec![Model::example("first"), Model::example("second")];
        let models = Models {
            channels: vec![channel],
            providers: vec![(0, provider)],
            binding: None,
        };
        let features = Some(BTreeMap::from([("speed".to_owned(), "fast".to_owned())]));
        for model in ["first", "second"] {
            let field = cx.new(|cx| {
                ModelField::new(
                    models.clone(),
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
                assert_eq!(selected.provider, provider);
                assert_eq!(selected.model, model);
                assert_eq!(selected.effort, Some(Effort::High));
                assert_eq!(selected.features, features);
            });
        }
    });
}
