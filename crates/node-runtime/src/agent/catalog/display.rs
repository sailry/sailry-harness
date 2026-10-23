//! Localized native declarations are captured once with each admitted tool call.
use sailry_protocol::{plugin::desktop::Navigation, tool::Display};
use serde_json::json;
use std::{collections::BTreeMap, sync::OnceLock};

#[derive(Clone, Copy)]
pub(in crate::agent) enum Label {
    LoadSkill,
    ReadSkillResource,
    ListSessions,
    ReadSession,
    SearchSession,
    Plugins,
    Skills,
    Mcp,
    Title,
    Install,
    Update,
    Uninstall,
}

impl Label {
    fn key(self) -> &'static str {
        match self {
            Self::LoadSkill => "tool_load_skill",
            Self::ReadSkillResource => "tool_read_skill_resource",
            Self::ListSessions => "tool_list_sessions",
            Self::ReadSession => "tool_read_session",
            Self::SearchSession => "tool_search_session",
            Self::Plugins => "settings_plugins",
            Self::Skills => "settings_skills",
            Self::Mcp => "settings_mcp",
            Self::Title => "tool_session_title",
            Self::Install => "plugins_install",
            Self::Update => "settings_update",
            Self::Uninstall => "plugins_uninstall",
        }
    }
}

pub(in crate::agent) fn new(label: Label) -> Display {
    name(label).into()
}

fn name(label: Label) -> Navigation {
    static LABELS: OnceLock<BTreeMap<String, BTreeMap<String, String>>> = OnceLock::new();
    let labels = LABELS.get_or_init(|| {
        let sources: [(&str, &[&str]); 10] = [
            (
                "en",
                &[
                    include_str!("../../../../../locales/application.en.yml"),
                    include_str!("../../../../../locales/en.yml"),
                ],
            ),
            ("zh-CN", &[include_str!("../../../../../locales/zh-CN.yml")]),
            (
                "zh-TW",
                &[include_str!("../../../../../locales/application.zh-TW.yml")],
            ),
            (
                "ja",
                &[include_str!("../../../../../locales/application.ja.yml")],
            ),
            (
                "ko",
                &[include_str!("../../../../../locales/application.ko.yml")],
            ),
            (
                "fr",
                &[include_str!("../../../../../locales/application.fr.yml")],
            ),
            (
                "de",
                &[include_str!("../../../../../locales/application.de.yml")],
            ),
            (
                "es",
                &[include_str!("../../../../../locales/application.es.yml")],
            ),
            (
                "pt-BR",
                &[include_str!("../../../../../locales/application.pt-BR.yml")],
            ),
            (
                "ru",
                &[include_str!("../../../../../locales/application.ru.yml")],
            ),
        ];
        sources
            .into_iter()
            .map(|(locale, sources)| {
                let mut labels = BTreeMap::new();
                for source in sources {
                    let values: BTreeMap<String, String> =
                        serde_saphyr::from_str(source).expect("native tool labels are valid");
                    labels.extend(values);
                }
                (locale.to_owned(), labels)
            })
            .collect()
    });
    let key = label.key();
    Navigation {
        label: labels["en"][key].clone(),
        icon: None,
        locales: labels
            .iter()
            .filter(|(locale, _)| locale.as_str() != "en")
            .map(|(locale, labels)| (locale.clone(), labels[key].clone()))
            .collect(),
    }
}

pub(in crate::agent) fn management(label: Label, version: Option<&str>) -> Display {
    let mut display = new(label);
    display.input = Some(serde_json::from_value(json!({"summary":{"path":"/name"}})).unwrap());
    let actions = ["install", "update", "uninstall"].map(|action| {
        json!({
            "value":{"source":"arguments","path":"/action"},"equals":action
        })
    });
    let mutations = json!({"any":actions});
    let mut parts = vec![json!({"value":{"source":"arguments","path":"/name"},"when":mutations})];
    if let Some(version) = version {
        parts.push(json!({"value":{"source":"result","path":version},"when":mutations}));
    }
    parts.push(json!({"rows":{"value":{"source":"result","path":"/failures"},"separator":"\n","text":{
        "separator":": ","parts":[{"value":{"source":"item","path":"/name"}},{"value":{"source":"item","path":"/error/code"}},{"value":{"source":"item","path":"/error/message"}}]
    }}}));
    let notices: Vec<_> = [
        ("install", Label::Install),
        ("update", Label::Update),
        ("uninstall", Label::Uninstall),
    ]
    .into_iter()
    .map(|(action, label)| {
        json!({"message":name(label),"when":{"all":[{
            "value":{"source":"arguments","path":"/action"},"equals":action
        }]}})
    })
    .collect();
    display.output = Some(
        serde_json::from_value(
            json!({"preview":{"separator":" ","parts":parts},"notices":notices}),
        )
        .unwrap(),
    );
    display
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;

    #[test]
    fn managers_expose_only_changes_and_faults() {
        for (label, version, result, expected, titles) in [
            (
                Label::Plugins,
                Some("/summary/version"),
                json!({"summary":{"version":"0.1.2"}}),
                "fixture 0.1.2",
                ("Plugins", "插件"),
            ),
            (
                Label::Skills,
                Some("/source/registry/version"),
                json!({"source":{"registry":{"version":"0.1.2"}}}),
                "fixture 0.1.2",
                ("Skills", "技能"),
            ),
            (
                Label::Mcp,
                None,
                json!({"name":"fixture"}),
                "fixture",
                ("MCP", "MCP"),
            ),
        ] {
            let display = management(label, version);
            assert!(display.valid());
            assert_eq!((display.label("en"), display.label("zh-CN")), titles);
            assert!(display.locales.contains_key("zh-CN"));
            let output = display.output.unwrap();
            let preview = output.preview.as_ref().unwrap();
            let original = result.clone();
            for action in ["list", "search", "inspect", "discover", "check_updates"] {
                let args = json!({"action":action,"name":"fixture"});
                assert!(!preview.visible(&args, &result));
                assert!(preview.render(&args, &result).is_empty());
                assert!(output.notices(&args, &result, "zh-CN").is_empty());
            }
            for action in ["install", "update", "uninstall"] {
                let args = json!({"action":action,"name":"fixture"});
                assert_eq!(preview.render(&args, &result), expected);
                assert_eq!(output.notices(&args, &result, "en").len(), 1);
            }
            let faults = json!({"failures":[{"name":"fixture","error":{"code":"revision_conflict","message":"Package changed"}}]});
            assert_eq!(
                preview.render(&json!({"action":"check_updates"}), &faults),
                "fixture: revision_conflict: Package changed"
            );
            assert_eq!(result, original);
            assert!(preview.render(&Value::Null, &Value::Null).is_empty());
        }
    }
}
