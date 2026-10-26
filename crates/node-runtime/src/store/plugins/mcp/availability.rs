//! Directory projection reads configuration and authorization without starting services.
use super::*;
use plugin::{
    connector::{Auth, State as ConnectionState},
    mcp::Availability,
};

pub(in crate::store::plugins) async fn project(
    db: &Connection,
    profile: Option<&std::path::Path>,
    mut info: Info,
) -> Result<Info, Fault> {
    if info.mcp.is_empty() {
        return Ok(info);
    }
    let saved = settings::stored(db, &info).await?;
    // Retained package versions can outlive their current installation.
    let installed = get(db, &info.summary.name).await?.is_some();
    let configured = installed && settings::resolve(db, &info).await.is_ok();
    let declarations = connectors::declarations(&info);
    let records = if installed {
        connectors::records(db).await?
    } else {
        Vec::new()
    };
    for server in &mut info.mcp {
        server.allowed = saved.mcp_allowed.get(&server.name).copied().unwrap_or(true);
        server.availability = if !installed {
            Availability::Unavailable
        } else if !info.summary.compatibility.is_compatible() {
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
                availability = match connectors::project(db, profile, record).await?.state {
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

    #[tokio::test]
    async fn reads_removed_versions() {
        let directory = tempfile::tempdir().unwrap();
        let host = crate::plugins::Host::new(Some(directory.path().canonicalize().unwrap()));
        let engine = crate::store::database::engine(std::path::Path::new(":memory:"))
            .await
            .unwrap();
        let db = crate::store::database::connect(&engine).await.unwrap();
        db.execute_batch(include_str!("../../schema.sql"))
            .await
            .unwrap();
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
        .await
        .unwrap();
        let Output::Plugin(mut info) = result.unwrap() else {
            panic!("plugin expected")
        };
        let mut saved = settings::stored(&db, &info).await.unwrap();
        let declared = host
            .mcp_configuration(
                &info.summary.reference(),
                sailry_link::CancellationToken::new(),
            )
            .unwrap();
        saved.mcp = Some(declared.clone());
        settings::persist(&db, &mut info, &saved).await.unwrap();
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
        .await
        .unwrap();
        account.state = State::Connected;
        connectors::save(&db, &account).await.unwrap();
        assert!(account.enabled);
        assert_eq!(
            project(&db, host.profile(), info.clone())
                .await
                .unwrap()
                .mcp[0]
                .availability,
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
        .await
        .unwrap();
        assert!(
            plugins::get(&db, &info.summary.name)
                .await
                .unwrap()
                .is_none()
        );
        assert_eq!(
            encode(&connectors::read(&db, account.id).await.unwrap()).unwrap(),
            encode(&account).unwrap()
        );
        let (Output::Plugin(retained), _) = plugins::execute(
            &db,
            host.profile(),
            &Command::ReadPluginVersion {
                package: reference.clone(),
            },
        )
        .await
        .unwrap() else {
            panic!("retained plugin expected")
        };
        assert_eq!(retained.summary.reference(), reference);
        assert!(retained.summary.available());
        assert!(retained.mcp[0].allowed);
        assert_eq!(retained.mcp[0].availability, Availability::Unavailable);
        assert_eq!(
            settings::stored(&db, &retained).await.unwrap().mcp,
            Some(declared.clone())
        );
        assert_eq!(
            host.mcp_configuration(&reference, sailry_link::CancellationToken::new())
                .unwrap(),
            declared
        );
        assert_eq!(
            super::super::prepare(&db, &reference, "github")
                .await
                .err()
                .expect("removed package must not prepare")
                .code,
            ErrorCode::NotFound
        );
    }

    #[tokio::test]
    async fn separates_configuration_and_authorization() {
        let directory = tempfile::tempdir().unwrap();
        let host = crate::plugins::Host::new(Some(directory.path().canonicalize().unwrap()));
        let mut info = host.install_bundled("github").unwrap();
        info.settings = None;
        let engine = crate::store::database::engine(std::path::Path::new(":memory:"))
            .await
            .unwrap();
        let db = crate::store::database::connect(&engine).await.unwrap();
        db.execute_batch(include_str!("../../schema.sql"))
            .await
            .unwrap();
        super::super::super::save(&db, &info, true).await.unwrap();
        let projected = async || {
            project(&db, host.profile(), info.clone())
                .await
                .unwrap()
                .mcp[0]
                .availability
        };
        assert_eq!(projected().await, Availability::ConnectionRequired);
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
        .await
        .unwrap();
        for (state, expected) in [
            (State::NotConnected, Availability::ConnectionRequired),
            (State::Connecting, Availability::ConnectionRequired),
            (State::Connected, Availability::Available),
            (State::Expired, Availability::AuthorizationExpired),
            (State::Failed, Availability::Unavailable),
        ] {
            account.state = state;
            connectors::save(&db, &account).await.unwrap();
            assert_eq!(projected().await, expected);
        }
        account.enabled = false;
        connectors::save(&db, &account).await.unwrap();
        assert_eq!(projected().await, Availability::Unavailable);
        account.state = State::Connected;
        connectors::save(&db, &account).await.unwrap();
        assert_eq!(projected().await, Availability::ConnectionDisabled);
        let disabled = set_allowed(
            &db,
            host.profile(),
            &info.summary.reference(),
            "github",
            false,
        )
        .await
        .unwrap();
        assert_eq!(
            project(&db, host.profile(), disabled.clone())
                .await
                .unwrap()
                .mcp[0]
                .availability,
            Availability::ServerDisabled
        );
        let mut disabled = disabled;
        disabled.summary.enabled = false;
        assert_eq!(
            project(&db, host.profile(), disabled).await.unwrap().mcp[0].availability,
            Availability::PackageDisabled
        );
    }
}
