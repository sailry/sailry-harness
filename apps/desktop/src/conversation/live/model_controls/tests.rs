use super::*;
use core::prelude::v1::test;

#[test]
fn orders_explicit_levels_and_budgets() {
    let mut choices = [Effort::High, Effort::Disabled, Effort::Max, Effort::Low];
    choices.sort_by_key(|effort| order(*effort));
    assert_eq!(
        choices,
        [Effort::Disabled, Effort::Low, Effort::High, Effort::Max]
    );
    assert!(order(Effort::Budget(1024)) < order(Effort::Budget(8192)));
}

mod feature_choices {
    use super::*;
    use sailry_protocol::conversation::features::{Feature, FeatureOption};
    use std::collections::BTreeMap;

    fn declarations() -> Vec<Feature> {
        vec![
            Feature {
                id: "speed".into(),
                name: Some("Speed".into()),
                icon: None,
                options: vec![
                    FeatureOption {
                        id: "fast".into(),
                        name: Some("Fast".into()),
                    },
                    FeatureOption {
                        id: "future-choice".into(),
                        name: None,
                    },
                ],
            },
            Feature {
                id: "format".into(),
                name: None,
                icon: None,
                options: vec![FeatureOption {
                    id: "compact".into(),
                    name: Some("Compact".into()),
                }],
            },
        ]
    }

    #[test]
    fn removes_unadvertised_choices() {
        let selected = BTreeMap::from([
            ("speed".into(), "future-choice".into()),
            ("format".into(), "missing".into()),
            ("removed".into(), "value".into()),
        ]);
        assert_eq!(
            super::super::features::retain(&declarations(), &selected),
            BTreeMap::from([("speed".into(), "future-choice".into())])
        );
        assert!(super::super::features::retain(&[], &selected).is_empty());
    }

    #[test]
    fn labels_follow_declared_order() {
        let labels = |selected: &BTreeMap<_, _>| {
            super::super::features::selected(&declarations(), selected)
                .into_iter()
                .map(|choice| choice.label)
                .collect::<Vec<_>>()
        };
        let selected = BTreeMap::from([
            ("speed".into(), "fast".into()),
            ("format".into(), "compact".into()),
        ]);
        assert_eq!(
            labels(&selected),
            vec![tr("composer_feature_fast"), SharedString::from("Compact")]
        );
        let mut selected = selected;
        selected.insert("speed".into(), "future-choice".into());
        assert_eq!(selected.len(), 2);
        assert_eq!(labels(&selected)[0], "future-choice");
        selected.remove("speed");
        assert_eq!(labels(&selected), vec![SharedString::from("Compact")]);
    }

    #[test]
    fn fast_glyph_uses_the_declared_choice() {
        let mut features = declarations();
        let selected = BTreeMap::from([("speed".into(), "fast".into())]);
        assert!(super::super::features::selected(&features, &selected)[0].fast);
        assert!(
            super::super::features::selected(&features, &selected)[0]
                .icon
                .is_some()
        );
        features[0].id = "custom-speed".into();
        let custom = BTreeMap::from([("custom-speed".into(), "fast".into())]);
        assert!(!super::super::features::selected(&features, &custom)[0].fast);
        assert!(
            super::super::features::selected(&features, &custom)[0]
                .icon
                .is_none()
        );
        assert!(super::super::features::selected(&features, &selected).is_empty());
    }
}
