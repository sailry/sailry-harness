//! Safe binding data comes only from the invocation, never live controller selection.
use super::*;
use sailry_protocol::{
    database::{Connection, Engine},
    tool::Operation,
};

pub(super) fn input(invocation: &Invocation, package: &plugin::Info, selected: &[String]) -> Value {
    let operations: Vec<_> = package
        .extension
        .iter()
        .flat_map(|extension| &extension.tools)
        .filter(|tool| selected.contains(&tool.name))
        .flat_map(|tool| {
            tool.operation
                .into_iter()
                .chain(tool.handler.iter().flat_map(|handler| {
                    handler.operation.into_iter().chain(
                        handler
                            .flow
                            .iter()
                            .flat_map(|flow| flow.operations.iter().copied()),
                    )
                }))
        })
        .collect();
    let databases = operations.iter().any(|operation| {
        matches!(
            operation,
            Operation::BrowseDatabase | Operation::QueryDatabase | Operation::ExecuteDatabase
        )
    });
    let ssh = operations
        .iter()
        .any(|operation| matches!(operation, Operation::RunSsh | Operation::TransferSsh));
    let roles: Vec<_> = invocation
        .turn
        .roles
        .profiles
        .iter()
        .filter(|_| operations.contains(&Operation::DelegateAgent))
        .map(|role| json!({"key":role.key,"name":role.name,"description":role.description}))
        .collect();
    let databases: Vec<_> = invocation
        .connections
        .databases
        .iter()
        .filter(|_| databases)
        .map(database)
        .collect();
    let ssh: Vec<_> = invocation
        .connections
        .ssh
        .iter()
        .filter(|_| ssh)
        .map(|profile| json!({"id":profile.id,"name":profile.name}))
        .collect();
    let names: Vec<_> = selected
        .iter()
        .map(|name| operations::alias(&package.summary.name, name))
        .collect();
    let config = package
        .extension
        .as_ref()
        .filter(|extension| {
            extension
                .actions
                .contains(&plugin::Action::ReadConversation)
        })
        .map(|_| &invocation.turn.config);
    json!({"roles":roles,"automatic":invocation.automatic,"mode":invocation.turn.config.mode,"config":config,"names":names,"connections":{"bound":invocation.connections.bound,"databases":databases,"ssh":ssh}})
}

fn database(profile: &sailry_protocol::database::Profile) -> Value {
    let (engine, database) = match &profile.connection {
        Connection::Sqlite { .. } => (Engine::Sqlite, "main"),
        Connection::Network {
            engine, database, ..
        }
        | Connection::Socket {
            engine, database, ..
        }
        | Connection::Ssh {
            engine, database, ..
        } => (*engine, database.as_str()),
    };
    json!({"id":profile.id,"name":profile.name,"engine":engine,"database":database,"read_only":profile.read_only})
}

#[cfg(test)]
mod tests {
    use super::*;
    use sailry_protocol::database::{Profile, Tls};

    #[test]
    fn keeps_connection_metadata_private() {
        let profile = Profile {
            id: sailry_protocol::DatabaseId::new(),
            revision: 1,
            name: "Reports".into(),
            sharing: None,
            read_only: true,
            connection: Connection::Network {
                engine: Engine::Postgres,
                host: "private-host".into(),
                port: 5432,
                database: "analytics".into(),
                username: "private-user".into(),
                tls: Tls::Disable,
            },
        };
        let value = database(&profile);
        assert_eq!(
            value,
            json!({"id":profile.id,"name":"Reports","engine":"postgres","database":"analytics","read_only":true})
        );
        assert!(!value.to_string().contains("private"));
        let profile = Profile {
            connection: Connection::Sqlite {
                path: "/private/location.sqlite".into(),
            },
            ..profile
        };
        assert_eq!(database(&profile)["database"], "main");
        assert!(!database(&profile).to_string().contains("location.sqlite"));
    }
}
