use super::*;
use sailry_protocol::{
    WorktreeId,
    plugin::{Context, Reference, desktop::Surface},
};

fn context() -> Context {
    Context {
        package: Reference {
            name: "fixture".into(),
            digest: "a".repeat(64),
            settings_revision: 1,
        },
        surface: Surface::Workspace,
        worktree: Some(WorktreeId::new()),
        session: None,
        turn: None,
        invocation: None,
    }
}

#[test]
fn separates_catalog_and_worktree_instances() {
    let context = context();
    assert_eq!(
        command(Operation::CodeCatalog, json!({}), &context).unwrap(),
        Command::ReadCodeCatalog
    );
    assert_eq!(
        command(Operation::CodeServers, json!({}), &context).unwrap(),
        Command::ListCodeServers {
            worktree: context.worktree.unwrap()
        }
    );
    assert_eq!(
        command(
            Operation::CodePackages,
            json!({"installed": true}),
            &context
        )
        .unwrap(),
        Command::ListCodeInstallations
    );
    assert_eq!(
        command(
            Operation::CodePackages,
            json!({"installed": false}),
            &context
        )
        .unwrap(),
        Command::ListCodePackages {
            worktree: context.worktree.unwrap()
        }
    );
    assert!(
        command(
            Operation::CodeServers,
            json!({"worktree":WorktreeId::new()}),
            &context
        )
        .is_err()
    );
}

#[test]
fn confines_exact_lifecycle_descriptors() {
    let context = context();
    let startup = code::Startup {
        worktree: context.worktree.unwrap(),
        root: "src".into(),
        server: "fixture".into(),
        executable: "/fixture/server".into(),
        revision: "b".repeat(64),
    };
    for operation in [Operation::StartCodeServer, Operation::StopCodeServer] {
        let output = command(operation, serde_json::to_value(&startup).unwrap(), &context).unwrap();
        assert_eq!(
            output,
            if operation == Operation::StartCodeServer {
                Command::StartCodeServer {
                    startup: startup.clone(),
                }
            } else {
                Command::StopCodeServer {
                    startup: startup.clone(),
                }
            }
        );
        let mut foreign = startup.clone();
        foreign.worktree = WorktreeId::new();
        assert_eq!(
            command(operation, serde_json::to_value(foreign).unwrap(), &context)
                .unwrap_err()
                .code,
            sailry_protocol::ErrorCode::PermissionDenied
        );
        let mut extra = serde_json::to_value(&startup).unwrap();
        extra["args"] = json!(["--arbitrary"]);
        assert!(command(operation, extra, &context).is_err());
        assert!(!operation.read_only());
        assert_eq!(operation.action(), Some(plugin::Action::ManageCode));
    }
}
