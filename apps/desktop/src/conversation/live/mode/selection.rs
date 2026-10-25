//! Mode changes share the existing atomic session configuration command.
use super::PluginMode;
use sailry_protocol::{SessionConfig, WorkMode};
use serde_json::Value;

pub(super) fn configure(
    config: &mut SessionConfig,
    modes: &[PluginMode],
    base: WorkMode,
    selected: Option<&PluginMode>,
) {
    for mode in modes {
        if let Some(options) = config.options.get_mut(&mode.package.name)
            && mode.declaration.selected(Some(options))
        {
            options.insert(
                mode.declaration.option.clone(),
                Value::String(String::new()),
            );
        }
    }
    config.mode = selected.map_or(base, |mode| mode.declaration.base());
    if let Some(mode) = selected {
        config
            .options
            .entry(mode.package.name.clone())
            .or_default()
            .insert(
                mode.declaration.option.clone(),
                Value::String(mode.declaration.id.clone()),
            );
    }
}

pub(super) fn selected(config: &SessionConfig, modes: &[PluginMode]) -> Option<PluginMode> {
    let mut selected = modes.iter().filter(|mode| {
        mode.declaration
            .selected(config.options.get(&mode.package.name))
    });
    let first = selected.next()?;
    (selected.next().is_none() && first.declaration.allows(config)).then(|| first.clone())
}

#[cfg(test)]
mod tests {
    use super::*;
    use sailry_protocol::{Permission, ProviderId};
    use serde_json::json;

    fn modes() -> [PluginMode; 2] {
        [("first", true), ("second", false)].map(|(name, write)| PluginMode {
            package: serde_json::from_value(json!({"name":name,"revision":1,"settings_revision":0,"digest":"a".repeat(64)})).unwrap(),
            declaration: serde_json::from_value(json!({"id":"review","option":"mode",
                "display":{"label":"Review"},"permissions":{"write":write},"instructions":"Inspect changes"})).unwrap(),
        })
    }

    #[test]
    fn switches_owners_and_returns_to_build() {
        let modes = modes();
        let mut config: SessionConfig = serde_json::from_value(json!({
            "provider":ProviderId::new(),"model":"fixture","mode":"code","permission":"ask","credential":null,
            "options":{"first":{"proactive":true,"retired":42},"second":{"unrelated":"keep"},"other":{"value":true}}
        })).unwrap();
        let preserved = config.clone();
        for mode in &modes {
            configure(&mut config, &modes, WorkMode::Code, Some(mode));
            assert_eq!(selected(&config, &modes).unwrap().package, mode.package);
            assert_eq!(config.mode, mode.declaration.base());
            assert_eq!(config.permission, Permission::Ask);
        }
        assert_eq!(config.options["first"]["mode"], json!(""));
        configure(&mut config, &modes, WorkMode::Code, None);
        assert!(selected(&config, &modes).is_none());
        assert_eq!(config.mode, WorkMode::Code);
        assert_eq!(config.options["second"]["mode"], json!(""));
        for (owner, values) in preserved.options {
            for (key, value) in values {
                assert_eq!(config.options[&owner][&key], value);
            }
        }
    }

    #[test]
    fn rejects_ambiguous_or_unavailable_selections() {
        let modes = modes();
        let mut config: SessionConfig = serde_json::from_value(json!({
            "provider":ProviderId::new(),"model":"fixture","mode":"plan","permission":"full","credential":null,
            "options":{"first":{"mode":"review"},"second":{"mode":"review"}}
        })).unwrap();
        assert!(selected(&config, &modes).is_none());
        assert!(selected(&config, &[]).is_none());
        configure(&mut config, &modes, WorkMode::Plan, None);
        assert!(selected(&config, &modes).is_none());
        assert_eq!(config.mode, WorkMode::Plan);
        configure(&mut config, &modes, WorkMode::Plan, Some(&modes[1]));
        config.mode = WorkMode::Code;
        assert!(selected(&config, &modes).is_none());
    }
}
