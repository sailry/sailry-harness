use super::*;
use sailry_protocol::{DatabaseId, SshId, database, ssh};

fn recipe() -> sailry_protocol::tool::Display {
    serde_json::from_value(json!({"label":"Run", "input":{"summary":{
        "path":"/connection", "format":"connection"
    }}}))
    .unwrap()
}

fn profile(name: &str) -> ssh::Profile {
    ssh::Profile {
        id: SshId::new(),
        revision: 1,
        name: name.into(),
        host: "ssh.example.invalid".into(),
        port: 22,
        username: "fixture".into(),
        authentication: ssh::Authentication::Agent,
        host_key: None,
        sharing: None,
    }
}

#[test]
fn captures_only_admitted_public_names() {
    let host = profile("Build host");
    let blank = profile(" ");
    let database = database::Profile {
        id: DatabaseId::new(),
        revision: 1,
        name: "Reports".into(),
        sharing: None,
        connection: database::Connection::Sqlite {
            path: "reports.sqlite3".into(),
        },
        read_only: true,
    };
    let catalog = crate::store::agent::connections::Catalog {
        bound: None,
        databases: vec![database.clone()],
        ssh: vec![host.clone(), blank],
    };
    for operation in [Operation::RunSsh, Operation::TransferSsh] {
        let captured = display(operation, recipe(), &catalog);
        let names = &captured
            .input
            .as_ref()
            .unwrap()
            .summary
            .as_ref()
            .unwrap()
            .names;
        assert_eq!(names.len(), 1);
        assert_eq!(names[&host.id.to_string()], "Build host");
        assert!(captured.valid());
        let text = serde_json::to_string(&captured).unwrap();
        assert!(!text.contains("ssh.example.invalid"));
        assert!(!text.contains("fixture"));
        assert!(!text.contains("authentication"));
    }
    let captured = display(Operation::QueryDatabase, recipe(), &catalog);
    let names = &captured
        .input
        .as_ref()
        .unwrap()
        .summary
        .as_ref()
        .unwrap()
        .names;
    assert_eq!(names.len(), 1);
    assert_eq!(names[&database.id.to_string()], "Reports");
    assert!(
        display(Operation::ReadFile, recipe(), &catalog)
            .input
            .unwrap()
            .summary
            .unwrap()
            .names
            .is_empty()
    );
}

#[test]
fn filters_mutations_in_plan() {
    use sailry_protocol::WorkMode;
    for (operation, planning) in [
        (Operation::SaveDatabaseConnection, false),
        (Operation::SaveSshConnection, false),
        (Operation::ExecuteDatabase, false),
        (Operation::ListDatabaseConnections, true),
        (Operation::ListSshConnections, true),
        (Operation::BrowseDatabase, true),
        (Operation::QueryDatabase, true),
        (Operation::RunSsh, true),
        (Operation::TransferSsh, true),
    ] {
        assert_eq!(
            supports_mode(operation, WorkMode::Plan),
            planning,
            "{operation:?}"
        );
        assert!(supports_mode(operation, WorkMode::Code), "{operation:?}");
    }
}
