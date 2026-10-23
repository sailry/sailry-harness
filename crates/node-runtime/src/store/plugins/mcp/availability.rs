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
    // Retained package versions can outlive their current installation.
    let installed = get(db, &info.summary.name)?.is_some();
    let configured = installed && settings::resolve(db, &info).is_ok();
    let declarations = connectors::declarations(&info);
    let records = if installed {
        connectors::records(db)?
    } else {
        Vec::new()
    };
    for server in &mut info.mcp {
        server.allowed = saved.mcp_allowed.get(&server.name).copied().unwrap_or(true);
        server.availability = if !installed {
            Availability::Unavailable
        } else if !info.summary.enabled {
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
                availability = match connectors::project(db, profile, record)?.state {
                    ConnectionState::Connected if !record.enabled => {
                        Availability::ConnectionDisabled
                    }
                    ConnectionState::Connected => Availability::Available,
                    ConnectionState::Expired => Availability::AuthorizationExpired,
                    ConnectionState::NeedsConfirmation => Availability::ConsentRequired,
                    ConnectionState::NotConnected | ConnectionState::Connecting => {
                        Availability::ConnectionRequired
                    }
                    ConnectionState::Failed => Availability::Unavailable,
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
    use crate::store::plugins;
    use plugin::connector::{Input, Kind, State};

    #[test]
    fn reads_removed_versions() {
        let directory = tempfile::tempdir().unwrap();
        let host = crate::plugins::Host::new(Some(directory.path().canonicalize().unwrap()));
        let db = Connection::open_in_memory().unwrap();
        db.execute_batch(include_str!("../../schema.sql")).unwrap();
        let mut result = Ok(Output::Plugin(host.install_bundled("github").unwrap()));
        plugins::finish(
            &db,
            host.profile(),
            &Command::InstallBundledPlugin {
                name: "github".into(),
                expected_revision: 0,
            },
            &mut result,
        )
        .unwrap();
        let Output::Plugin(mut info) = result.unwrap() else {
            panic!("plugin expected")
        };
        let mut saved = settings::stored(&db, &info).unwrap();
        let declared = host
            .mcp_configuration(
                &info.summary.reference(),
                sailry_link::CancellationToken::new(),
            )
            .unwrap();
        saved.mcp = Some(declared.clone());
        settings::persist(&db, &mut info, &saved).unwrap();
        let reference = info.summary.reference();
        assert!(reference.settings_revision > 0);
        let (mut account, _) = connectors::create(
            &db,
            host.profile(),
            &Command::StartConnectorLogin {
                package: reference.clone(),
                connector: "github".into(),
                connection: None,
                expected_revision: None,
                method: Kind::Secret,
                input: Input::default(),
            },
        )
        .unwrap();
        account.state = State::Connected;
        connectors::save(&db, &account).unwrap();
        assert!(account.enabled);
        assert_eq!(
            project(&db, host.profile(), info.clone()).unwrap().mcp[0].availability,
            Availability::Available
        );
        plugins::execute(
            &db,
            host.profile(),
            &Command::RemovePlugin {
                name: info.summary.name.clone(),
                expected_revision: info.summary.revision,
            },
        )
        .unwrap();
        assert!(plugins::get(&db, &info.summary.name).unwrap().is_none());
        assert_eq!(
            encode(&connectors::read(&db, account.id).unwrap()).unwrap(),
            encode(&account).unwrap()
        );
        let (Output::Plugin(retained), _) = plugins::execute(
            &db,
            host.profile(),
            &Command::ReadPluginVersion {
                package: reference.clone(),
            },
        )
        .unwrap() else {
            panic!("retained plugin expected")
        };
        assert_eq!(retained.summary.reference(), reference);
        assert!(retained.summary.enabled);
        assert!(retained.mcp[0].allowed);
        assert_eq!(retained.mcp[0].availability, Availability::Unavailable);
        assert_eq!(
            settings::stored(&db, &retained).unwrap().mcp,
            Some(declared.clone())
        );
        assert_eq!(
            host.mcp_configuration(&reference, sailry_link::CancellationToken::new())
                .unwrap(),
            declared
        );
        assert_eq!(
            super::super::prepare(&db, &reference, "github")
                .err()
                .expect("removed package must not prepare")
                .code,
            ErrorCode::NotFound
        );
    }

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
        assert_eq!(projected(), Availability::Unavailable);
        account.state = State::Connected;
        connectors::save(&db, &account).unwrap();
        assert_eq!(projected(), Availability::ConnectionDisabled);
        let disabled = set_allowed(
            &db,
            host.profile(),
            &info.summary.reference(),
            "github",
            false,
        )
        .unwrap();
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
