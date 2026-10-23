//! Directory projection reads configuration and authorization without starting services.
use super::*;
use plugin::{
    connector::{Auth, State as ConnectionState},
    mcp::Availability,
};

pub(in crate::store::plugins) fn project(
    db: &Connection,
    profile: Option<&std::path::Path>,
    mut info: Info,
) -> Result<Info, Fault> {
    if info.mcp.is_empty() {
        return Ok(info);
    }
    let saved = settings::stored(db, &info)?;
    let configured = settings::resolve(db, &info).is_ok();
    let declarations = connectors::declarations(&info);
    let records = connectors::records(db)?;
    for server in &mut info.mcp {
        server.allowed = saved.mcp_allowed.get(&server.name).copied().unwrap_or(true);
        server.availability = if !info.summary.enabled {
            Availability::PackageDisabled
        } else if !server.allowed {
            Availability::ServerDisabled
        } else if !configured {
            Availability::ConfigurationRequired
        } else {
            let mut availability = Availability::Available;
            for declaration in &declarations {
                let bound = declaration.inject.iter().any(|item| item.server.as_ref() == Some(&server.name))
                    || declaration.auth.iter().any(|auth| matches!(auth, Auth::McpOauth {server: name, ..} if name == &server.name));
                if !bound {
                    continue;
                }
                let record = records.iter().find(|record| {
                    record.plugin == info.summary.name && record.connector == declaration.id
                });
                let Some(record) = record else {
                    // Manual headers and anonymous custom servers do not require OAuth.
                    let optional = info.mcp_source.is_some() || declaration.auth.iter().all(|auth| {
                        matches!(auth, Auth::Secret {fields} if fields.iter().all(|name| declaration.fields.iter().any(|field| field.name == *name && field.optional)))
                    });
                    if optional {
                        continue;
                    }
                    availability = Availability::ConnectionRequired;
                    break;
                };
                availability = if !record.enabled {
                    Availability::ConnectionDisabled
                } else {
                    match connectors::project(db, profile, record)?.state {
                        ConnectionState::Connected => Availability::Available,
                        ConnectionState::Expired => Availability::AuthorizationExpired,
                        ConnectionState::NeedsConfirmation => Availability::ConsentRequired,
                        ConnectionState::NotConnected | ConnectionState::Connecting => {
                            Availability::ConnectionRequired
                        }
                        ConnectionState::Failed => Availability::Unavailable,
                    }
                };
                if availability != Availability::Available {
                    break;
                }
            }
            availability
        };
    }
    Ok(info)
}

#[cfg(test)]
mod tests {
    use super::*;
    use plugin::connector::{Input, Kind, State};

    #[test]
    fn separates_configuration_and_authorization() {
        let directory = tempfile::tempdir().unwrap();
        let host = crate::plugins::Host::new(Some(directory.path().canonicalize().unwrap()));
        let mut info = host.install_bundled("github").unwrap();
        info.settings = None;
        let db = Connection::open_in_memory().unwrap();
        db.execute_batch(include_str!("../../schema.sql")).unwrap();
        super::super::super::save(&db, &info, true).unwrap();
        let projected = || project(&db, host.profile(), info.clone()).unwrap().mcp[0].availability;
        assert_eq!(projected(), Availability::ConnectionRequired);
        let (mut account, _) = connectors::create(
            &db,
            host.profile(),
            &Command::StartConnectorLogin {
                package: info.summary.reference(),
                connector: "github".into(),
                connection: None,
                expected_revision: None,
                method: Kind::Secret,
                input: Input::default(),
            },
        )
        .unwrap();
        for (state, expected) in [
            (State::NotConnected, Availability::ConnectionRequired),
            (State::Connecting, Availability::ConnectionRequired),
            (State::Connected, Availability::Available),
            (State::Expired, Availability::AuthorizationExpired),
            (State::Failed, Availability::Unavailable),
        ] {
            account.state = state;
            connectors::save(&db, &account).unwrap();
            assert_eq!(projected(), expected);
        }
        account.enabled = false;
        connectors::save(&db, &account).unwrap();
        assert_eq!(projected(), Availability::ConnectionDisabled);
        let disabled = set_allowed(&db, &info.summary.reference(), "github", false).unwrap();
        assert_eq!(
            project(&db, host.profile(), disabled.clone()).unwrap().mcp[0].availability,
            Availability::ServerDisabled
        );
        let mut disabled = disabled;
        disabled.summary.enabled = false;
        assert_eq!(
            project(&db, host.profile(), disabled).unwrap().mcp[0].availability,
            Availability::PackageDisabled
        );
    }
}
