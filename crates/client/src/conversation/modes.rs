//! Conversation mode selection without changing Node-owned configuration.
use sailry_protocol::{SessionConfig, WorkMode, plugin};
use serde::{Deserialize, Serialize};

#[cfg(test)]
mod tests;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Mode {
    pub package: plugin::Reference,
    pub declaration: plugin::modes::Declaration,
    pub enabled: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct View {
    pub selected: Option<String>,
    pub choices: Vec<String>,
}

impl Mode {
    pub fn key(&self) -> String {
        format!("{}/{}", self.package.name, self.declaration.id)
    }

    fn matches(&self, config: &SessionConfig) -> bool {
        self.declaration
            .selected(config.options.get(&self.package.name))
    }
}

pub fn key(mode: WorkMode) -> &'static str {
    match mode {
        WorkMode::Code => "code",
        WorkMode::Plan => "plan",
    }
}

pub fn choices<'a>(
    config: Option<&'a SessionConfig>,
    modes: &'a [Mode],
) -> impl Iterator<Item = &'a Mode> + 'a {
    modes
        .iter()
        .filter(move |mode| mode.enabled || config.is_some_and(|config| mode.matches(config)))
}

pub fn project(config: &SessionConfig, modes: &[Mode]) -> View {
    let mut selected = modes.iter().filter(|mode| mode.matches(config));
    let selected = match selected.next() {
        Some(first) => {
            (selected.next().is_none() && first.declaration.allows(config)).then(|| first.key())
        }
        None => Some(key(config.mode).to_owned()),
    };
    View {
        selected,
        choices: choices(Some(config), modes).map(Mode::key).collect(),
    }
}

pub fn select(config: &SessionConfig, modes: &[Mode], selected: &str) -> Option<SessionConfig> {
    let mode = modes
        .iter()
        .find(|mode| mode.enabled && mode.key() == selected);
    let base = match selected {
        "code" => WorkMode::Code,
        "plan" => WorkMode::Plan,
        _ => mode?.declaration.base(),
    };
    let mut config = config.clone();
    for mode in modes {
        if let Some(options) = config.options.get_mut(&mode.package.name)
            && mode.declaration.selected(Some(options))
        {
            options.insert(mode.declaration.option.clone(), String::new().into());
        }
    }
    config.mode = base;
    if let Some(mode) = mode {
        config
            .options
            .entry(mode.package.name.clone())
            .or_default()
            .insert(
                mode.declaration.option.clone(),
                mode.declaration.id.clone().into(),
            );
    }
    Some(config)
}
