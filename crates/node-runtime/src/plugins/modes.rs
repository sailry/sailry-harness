//! Mode identity and instructions use the same package snapshot as turn tools.
use sailry_protocol::{ErrorCode, Fault, SessionConfig, plugin};

#[cfg(test)]
pub(crate) mod tests;

pub(crate) fn valid(extension: &plugin::Extension) -> bool {
    if !plugin::modes::valid(&extension.conversation_modes) {
        return false;
    }
    if extension.conversation_modes.is_empty() {
        return true;
    }
    if extension.scope != plugin::Scope::Host {
        return false;
    }
    let Ok(Some(schema)) = super::options::schema(extension) else {
        return false;
    };
    extension.conversation_modes.iter().all(|mode| {
        schema.properties.get(&mode.option).is_some_and(|field| {
            field.kind == plugin::settings::Kind::String
                && field.default.as_ref().and_then(serde_json::Value::as_str) == Some("")
                && field.choices.as_ref().is_some_and(|choices| {
                    choices
                        .iter()
                        .any(|value| value.as_str() == Some(mode.id.as_str()))
                        && choices.iter().any(|value| value.as_str() == Some(""))
                        && choices.iter().all(|value| {
                            value.as_str().is_some_and(|value| {
                                value.is_empty()
                                    || extension.conversation_modes.iter().any(|entry| {
                                        entry.option == mode.option && entry.id == value
                                    })
                            })
                        })
                })
        })
    })
}

pub(crate) fn selected<'a>(
    packages: &'a [plugin::Info],
    config: &SessionConfig,
) -> Result<Option<(&'a plugin::Info, &'a plugin::modes::Declaration)>, Fault> {
    let mut selected = None;
    for package in packages {
        let Some(extension) = package.extension.as_ref() else {
            continue;
        };
        if extension.conversation_modes.is_empty() {
            continue;
        }
        let options = super::options::captured(package, config)?;
        for mode in &extension.conversation_modes {
            if !mode.selected(Some(&options)) {
                continue;
            }
            if selected.is_some() {
                return Err(Fault::invalid("only one conversation mode can be selected"));
            }
            if !mode.allows(config) {
                return Err(Fault::new(
                    ErrorCode::PermissionDenied,
                    "conversation mode does not allow writing",
                ));
            }
            selected = Some((package, mode));
        }
    }
    Ok(selected)
}
