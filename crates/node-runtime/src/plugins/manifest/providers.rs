//! Invalid channel declarations are isolated from their valid sibling presets.
use sailry_protocol::{
    conversation::presets::{Auth, Preset},
    plugin::{Issue, IssueKind},
};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};

pub(super) fn normalize(value: &Value, issues: &mut Vec<Issue>) -> Value {
    let mut value = value.clone();
    let original = value.clone();
    let Some(presets) = value
        .get_mut("model_providers")
        .and_then(Value::as_array_mut)
    else {
        return value;
    };
    let mut seen = BTreeSet::new();
    let mut groups = BTreeMap::new();
    let mut index = 0;
    presets.retain(|preset| {
        let current=index;
        index+=1;
        let valid=serde_json::from_value::<Preset>(preset.clone()).is_ok_and(|preset| {
            crate::providers::presets::validate(&preset,false,true).is_ok()
                && (!matches!(&preset.auth, Auth::Plugin{flow} if !declares(&original,"model_auth",flow)))
                && preset.adapter.as_ref().is_none_or(|adapter|declares(&original,"model_adapters",adapter))
                && !seen.contains(&preset.id)
                && crate::providers::presets::group(&preset, &mut groups)
                && seen.insert(preset.id)
        });
        if !valid {issues.push(Issue {path:format!("plugin.json.extensions.dev.sailry.platform.model_providers.{current}"),kind:IssueKind::InvalidExtensions});}
        valid
    });
    value
}

fn declares(value: &Value, field: &str, id: &str) -> bool {
    value
        .get(field)
        .and_then(Value::as_array)
        .is_some_and(|values| {
            values
                .iter()
                .any(|value| value.get("id").and_then(Value::as_str) == Some(id))
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn isolates_invalid_siblings_and_references() {
        let good = json!({"id":"valid","name":"Valid","routes":[{"protocol":"openai-chat-completions","endpoint":"https://example.test/v1"}]});
        let mut bad = good.clone();
        bad["id"] = json!("invalid");
        bad["auth"] = json!({"kind":"plugin","data":{"flow":"missing"}});
        let mut issues = Vec::new();
        let value = normalize(
            &json!({"model_providers":[bad,good.clone(),{"id":7},good.clone()]}),
            &mut issues,
        );
        assert_eq!(value["model_providers"], json!([good]));
        assert_eq!(issues.len(), 3);
        assert!(
            issues
                .iter()
                .all(|issue| issue.kind == IssueKind::InvalidExtensions)
        );
    }

    #[test]
    fn isolates_conflicting_brand_displays() {
        let first = json!({"id":"first","name":"First route","brand":"example",
            "brand_display":{"label":"Example","icon":"reicon:messages/chat"},
            "routes":[{"protocol":"openai-chat-completions","endpoint":"https://example.test/v1"}]});
        let mut second = first.clone();
        second["id"] = json!("second");
        let mut third = first.clone();
        third["id"] = json!("third");
        third["brand_display"]["label"] = json!("Different brand");
        let mut duplicate = first.clone();
        duplicate["brand"] = json!("another");
        duplicate["brand_display"]["label"] = json!("Duplicate label");
        let mut later = first.clone();
        later["id"] = json!("later");
        later["brand"] = json!("another");
        let mut issues = Vec::new();
        let value = normalize(
            &json!({"model_providers":[first.clone(),second.clone(),third,duplicate,later.clone()]}),
            &mut issues,
        );
        assert_eq!(value["model_providers"], json!([first, second, later]));
        assert_eq!(issues.len(), 2);
        assert_eq!(
            issues[0].path,
            "plugin.json.extensions.dev.sailry.platform.model_providers.2"
        );
        assert_eq!(issues[0].kind, IssueKind::InvalidExtensions);
    }
}
