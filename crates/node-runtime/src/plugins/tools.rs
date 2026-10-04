//! Stable tool identities are shared by registration and admitted-call authorization.
use sailry_protocol::{
    plugin::{Action, Info},
    tool::Operation,
};

pub(crate) fn alias(package: &str, tool: &str) -> String {
    let digest = blake3::hash(package.as_bytes()).to_hex();
    format!("plugin_{}_{}", &digest[..16], tool)
}

pub(crate) fn operation(package: &Info, name: &str) -> Option<Operation> {
    package
        .extension
        .as_ref()?
        .tools
        .iter()
        .find_map(|tool| {
            (tool.valid() && alias(&package.summary.name, &tool.name) == name)
                .then(|| {
                    tool.operation
                        .or_else(|| tool.handler.as_ref().and_then(|handler| handler.operation))
                })
                .flatten()
        })
        .or_else(|| computer(package, name))
}

pub(crate) fn owns_computer(package: &Info) -> bool {
    package.extension.as_ref().is_some_and(|extension| {
        extension
            .actions
            .iter()
            .any(|action| matches!(action, Action::ReadComputer | Action::ControlComputer))
    })
}

/// Native tool ownership follows the frozen capability declaration, not a
/// package identity or caller-provided permission hint.
pub(crate) fn computer(package: &Info, name: &str) -> Option<Operation> {
    if !owns_computer(package) {
        return None;
    }
    let operation = if crate::computer::read_only(name)? {
        Operation::ReadComputer
    } else {
        Operation::ControlComputer
    };
    package
        .extension
        .as_ref()?
        .actions
        .contains(&operation.action()?)
        .then_some(operation)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn package(actions: &[Action]) -> Info {
        serde_json::from_value(json!({
            "summary":{
                "name":"native-provider","revision":1,"digest":"frozen-package",
                "settings_revision":0,"enabled":true,
            },
            "extension":{"api_version":"v1","actions":actions},
        }))
        .unwrap()
    }

    #[test]
    fn matches_native_grants() {
        let catalog = crate::computer::catalog().as_array().unwrap();
        let name = |read_only| {
            catalog
                .iter()
                .find(|definition| definition["annotations"]["readOnlyHint"] == read_only)
                .unwrap()["name"]
                .as_str()
                .unwrap()
        };
        let reader = package(&[Action::ReadComputer]);
        let controller = package(&[Action::ControlComputer]);
        assert!(owns_computer(&reader));
        assert_eq!(
            operation(&reader, name(true)),
            Some(Operation::ReadComputer)
        );
        assert_eq!(operation(&reader, name(false)), None);
        assert_eq!(operation(&controller, name(true)), None);
        assert_eq!(
            operation(&controller, name(false)),
            Some(Operation::ControlComputer)
        );
        assert_eq!(operation(&controller, "unknown-native-tool"), None);
    }

    #[test]
    fn does_not_grant_by_identity() {
        let mut package = package(&[]);
        package.summary.name = "computer".into();
        assert!(!owns_computer(&package));
        for definition in crate::computer::catalog().as_array().unwrap() {
            assert_eq!(
                operation(&package, definition["name"].as_str().unwrap()),
                None
            );
        }
    }
}
