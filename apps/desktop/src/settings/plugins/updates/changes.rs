use sailry_protocol::plugin::Info;
use std::collections::BTreeSet;

pub(super) struct Changes {
    pub tools: Delta,
    pub skills: Delta,
    pub settings: Delta,
    pub actions: Delta,
}

pub(super) struct Delta {
    pub added: Vec<String>,
    pub removed: Vec<String>,
}

impl Delta {
    fn between(previous: BTreeSet<String>, candidate: BTreeSet<String>) -> Self {
        Self {
            added: candidate.difference(&previous).cloned().collect(),
            removed: previous.difference(&candidate).cloned().collect(),
        }
    }

    pub fn empty(&self) -> bool {
        self.added.is_empty() && self.removed.is_empty()
    }
}

impl Changes {
    pub fn between(previous: &Info, candidate: &Info) -> Self {
        let tools = |info: &Info| {
            info.extension
                .iter()
                .flat_map(|extension| &extension.tools)
                .map(|tool| tool.name.clone())
                .collect()
        };
        let skills = |info: &Info| info.skills.iter().map(|skill| skill.name.clone()).collect();
        let settings = |info: &Info| {
            info.settings
                .iter()
                .flat_map(|schema| schema.properties.keys())
                .cloned()
                .collect()
        };
        let actions = |info: &Info| {
            let mut actions: BTreeSet<String> = info
                .extension
                .iter()
                .flat_map(|extension| &extension.actions)
                .filter_map(|action| serde_json::to_value(action).ok())
                .filter_map(|value| value.as_str().map(str::to_owned))
                .collect();
            for connector in info
                .extension
                .iter()
                .flat_map(|extension| &extension.connectors)
            {
                for auth in &connector.auth {
                    if let sailry_protocol::plugin::connector::Auth::LocalCli { .. } = auth {
                        actions.insert(
                            serde_json::json!({
                                "local_cli": auth,
                                "origins": connector.origins,
                                "inject": connector.inject,
                            })
                            .to_string(),
                        );
                    }
                }
            }
            actions
        };
        Self {
            tools: Delta::between(tools(previous), tools(candidate)),
            skills: Delta::between(skills(previous), skills(candidate)),
            settings: Delta::between(settings(previous), settings(candidate)),
            actions: Delta::between(actions(previous), actions(candidate)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn info(suffix: &str, action: &str) -> Info {
        serde_json::from_value(serde_json::json!({
            "summary": {"name":"example", "revision":1, "digest":suffix,
                "settings_revision":0, "version":"0.1.0"},
            "extension": {"api_version":"v1", "actions":[action],
                "tools":[{"name":format!("tool_{suffix}")},{"name":"retained"}]},
            "skills":[{"name":format!("skill_{suffix}"), "description":"Inspect changes",
                "path":"skills/analysis/SKILL.md"}],
            "settings": {"$schema":"https://json-schema.org/draft/2020-12/schema", "type":"object",
                "properties": {format!("setting_{suffix}"):{"type":"string"}}, "additionalProperties":false}
        })).unwrap()
    }

    #[test]
    fn extracts_declared_changes() {
        let changes = Changes::between(&info("old", "files.read"), &info("new", "files.write"));
        for (delta, kind) in [
            (&changes.tools, "tool"),
            (&changes.skills, "skill"),
            (&changes.settings, "setting"),
        ] {
            assert_eq!(delta.added, [format!("{kind}_new")]);
            assert_eq!(delta.removed, [format!("{kind}_old")]);
        }
        assert_eq!(changes.actions.added, ["files.write"]);
        assert_eq!(changes.actions.removed, ["files.read"]);
    }

    #[test]
    fn omits_unchanged_entries() {
        let delta = Delta::between(
            ["retained", "removed"].map(str::to_owned).into(),
            ["retained", "added"].map(str::to_owned).into(),
        );
        assert_eq!(delta.added, ["added"]);
        assert_eq!(delta.removed, ["removed"]);
        assert!(!delta.empty());
        assert!(Delta::between(BTreeSet::new(), BTreeSet::new()).empty());
    }
}
