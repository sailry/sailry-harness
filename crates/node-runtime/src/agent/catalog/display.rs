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
    ListAgents,
    AddAgent,
    EditAgent,
    DeleteAgent,
    ApplyAgents,
    Subagent,
    GuideAgent,
    WaitAgent,
    ListTeams,
    AddTeam,
    EditTeam,
    DeleteTeam,
    ApplyTeam,
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
            Self::ListAgents => "tool_list_agents",
            Self::AddAgent => "tool_add_agent",
            Self::EditAgent => "tool_edit_agent",
            Self::DeleteAgent => "tool_delete_agent",
            Self::ApplyAgents => "tool_apply_agents",
            Self::Subagent => "tool_spawn_agent",
            Self::GuideAgent => "tool_guide_agent",
            Self::WaitAgent => "tool_wait_agent",
            Self::ListTeams => "tool_list_teams",
            Self::AddTeam => "tool_add_team",
            Self::EditTeam => "tool_edit_team",
            Self::DeleteTeam => "tool_delete_team",
            Self::ApplyTeam => "tool_apply_team",
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

/// Agent and team tools share one icon; `input` names the argument fields to summarize.
pub(in crate::agent) fn team(label: Label, input: serde_json::Value) -> Display {
    let mut display = new(label);
    display.icon = Some(sailry_protocol::plugin::desktop::Icon::Name(
        "reicon:users/users2".into(),
    ));
    if !input.is_null() {
        display.input = Some(serde_json::from_value(input).unwrap());
    }
    display
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
    display.output =
        Some(serde_json::from_value(json!({"preview":{"separator":" ","parts":parts}})).unwrap());
    display
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;

    #[test]
    fn localizes_team_tools() {
        for (label, english, chinese) in [
            (Label::ListAgents, "List agents", "列出代理"),
            (Label::Subagent, "Subagent", "子代理"),
            (Label::WaitAgent, "Wait for subagent", "等待子代理"),
            (Label::ApplyTeam, "Apply team", "应用团队"),
        ] {
            let display = team(label, json!({"summary":{"path":"/name"}}));
            assert!(display.valid());
            assert_eq!(
                (display.label("en"), display.label("zh-CN")),
                (english, chinese)
            );
            assert_eq!(display.locales.len(), 9);
            assert!(display.icon.is_some());
            assert!(display.input.is_some());
        }
        assert!(team(Label::ListTeams, Value::Null).input.is_none());
    }

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
                assert!(output.notices(&args, &result, "en").is_empty());
                assert!(output.notices(&args, &result, "zh-CN").is_empty());
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
