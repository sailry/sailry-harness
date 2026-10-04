//! Plain handlers inherit their declared capabilities, not an implicit read-only claim.
use sailry_protocol::{plugin::Action, tool};

pub(super) fn read_only(handler: &tool::Handler, actions: &[Action]) -> bool {
    if let Some(operation) = handler.operation {
        return operation.read_only();
    }
    if let Some(flow) = &handler.flow {
        return flow
            .operations
            .iter()
            .all(|operation| operation.read_only());
    }
    if handler.read_only {
        return true;
    }
    actions.iter().all(|action| {
        matches!(
            action,
            Action::ReadActivity
                | Action::ReadUsage
                | Action::ReadDatabases
                | Action::ReadSsh
                | Action::ReadRoles
                | Action::ReadModels
                | Action::InspectMedia
                | Action::ReadMediaSettings
                | Action::ReadComputer
                | Action::ReadBrowser
                | Action::ReadExternalBrowser
                | Action::ReadFiles
                | Action::ReadGit
                | Action::ReadWorktrees
                | Action::ReadCommands
                | Action::ReadTerminals
                | Action::ReadConversation
                | Action::ReadProjects
                | Action::ReadStorage
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn classifies_plain_handlers_by_permissions() {
        let mut handler: tool::Handler = serde_json::from_value(json!({
            "name":"run","parameters":{"type":"object"}
        }))
        .unwrap();
        assert!(read_only(&handler, &[]));
        assert!(read_only(
            &handler,
            &[Action::ReadStorage, Action::ReadFiles]
        ));
        for action in [Action::WriteStorage, Action::Dispatch, Action::Http] {
            assert!(!read_only(&handler, &[Action::ReadStorage, action]));
        }
        handler.read_only = true;
        assert!(read_only(&handler, &[Action::WriteStorage]));
    }

    #[test]
    fn classifies_bound_handlers_by_operations() {
        for binding in [
            json!({"operation":"storage.get"}),
            json!({"flow":{"operations":["storage.get"]}}),
        ] {
            let mut definition = json!({"name":"run","parameters":{"type":"object"}});
            definition
                .as_object_mut()
                .unwrap()
                .extend(binding.as_object().unwrap().clone());
            let handler = serde_json::from_value(definition).unwrap();
            assert!(read_only(&handler, &[Action::WriteStorage]));
        }
        for binding in [
            json!({"operation":"storage.set"}),
            json!({"flow":{"operations":["storage.get","storage.set"]}}),
        ] {
            let mut definition =
                json!({"name":"run","read_only":true,"parameters":{"type":"object"}});
            definition
                .as_object_mut()
                .unwrap()
                .extend(binding.as_object().unwrap().clone());
            let handler = serde_json::from_value(definition).unwrap();
            assert!(!read_only(&handler, &[]));
        }
    }
}
