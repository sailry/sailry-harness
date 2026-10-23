//! Node-owned standard MCP configuration shares the plugin revision owner.
use super::*;
use crate::store::{Ingress, Job, unavailable};
use sailry_link::Admission;
use sailry_protocol::{
    Receipt, Request, VERSION,
    plugin::mcp::{Configuration, State},
};
use tokio::sync::oneshot;
mod availability;
pub(super) use availability::project;

pub(in crate::store) fn prepare(
    db: &Connection,
    package: &plugin::Reference,
    server: &str,
) -> Result<(Info, crate::plugins::settings::Resolved), Fault> {
    let info = settings::current(db, package)?;
    if !info.summary.enabled {
        return Err(Fault::new(
            ErrorCode::NotConfigured,
            "MCP package is disabled",
        ));
    }
    if !info.mcp.iter().any(|item| item.name == server) {
        return Err(Fault::new(ErrorCode::NotFound, "MCP server is unavailable"));
    }
    let settings = settings::resolve(db, &info)?;
    Ok((info, settings))
}

pub(super) fn set_allowed(
    db: &Connection,
    package: &plugin::Reference,
    server: &str,
    allowed: bool,
) -> Result<Info, Fault> {
    let mut info = settings::current(db, package)?;
    if !info.mcp.iter().any(|item| item.name == server) {
        return Err(Fault::new(ErrorCode::NotFound, "MCP server is unavailable"));
    }
    let mut saved = settings::stored(db, &info)?;
    if saved.mcp_allowed.get(server).copied().unwrap_or(true) != allowed {
        saved.mcp_allowed.insert(server.into(), allowed);
        settings::persist(db, &mut info, &saved)?;
    }
    Ok(info)
}

pub(in crate::store) fn configuration(
    db: &Connection,
    package: &plugin::Reference,
) -> Result<Option<Configuration>, Fault> {
    let info = settings::current(db, package)?;
    if info.mcp.is_empty() {
        return Err(Fault::new(
            ErrorCode::NotConfigured,
            "plugin has no MCP servers",
        ));
    }
    Ok(settings::stored(db, &info)?.mcp)
}

pub(super) fn save(
    db: &Connection,
    profile: Option<&std::path::Path>,
    package: &plugin::Reference,
    configuration: &Configuration,
) -> Result<Info, Fault> {
    let mut info = settings::current(db, package)?;
    if info.mcp_source.is_some() {
        return Err(Fault::invalid(
            "standalone MCP settings use their declared form",
        ));
    }
    let mut saved = settings::stored(db, &info)?;
    // Bound form fields remain the sole editor for their declared MCP servers.
    let bound = info
        .settings
        .iter()
        .flat_map(|schema| schema.properties.values())
        .filter_map(|field| field.secret.as_ref())
        .filter(|field| field.header.is_some() || field.env.is_some())
        .filter_map(|field| field.server.as_deref())
        .collect::<Vec<_>>();
    if !bound.is_empty() {
        let current = match &saved.mcp {
            Some(current) => current.clone(),
            None => crate::plugins::Host::new(profile.map(std::path::Path::to_owned))
                .mcp_configuration(package, sailry_link::CancellationToken::new())?,
        };
        if bound.into_iter().any(|name| {
            let previous = current.servers.get(name);
            previous.is_none() || previous != configuration.servers.get(name)
        }) {
            return Err(Fault::invalid("MCP slots use the plugin settings form"));
        }
    }
    saved.mcp = Some(configuration.clone());
    settings::persist(db, &mut info, &saved)?;
    Ok(info)
}

impl Ingress {
    async fn plugin_mcp_configuration(
        &self,
        package: plugin::Reference,
    ) -> Result<Option<Configuration>, Fault> {
        let (reply, response) = oneshot::channel();
        self.sender
            .try_send(Job::McpCredentials(
                crate::store::mcp::Operation::Configuration { package, reply },
            ))
            .map_err(|_| unavailable())?;
        response.await.map_err(|_| unavailable())?
    }

    pub(in crate::store) async fn read_plugin_mcp(
        &self,
        request: Request,
    ) -> Result<Admission, Fault> {
        if request.target != self.node || request.version != VERSION {
            return Err(Fault::new(
                ErrorCode::WrongTarget,
                "Node or protocol version mismatch",
            ));
        }
        if request.plugin.is_some() {
            return Err(Fault::new(
                ErrorCode::PermissionDenied,
                "MCP configuration is not exposed to plugin actions",
            ));
        }
        let Command::ReadPluginMcp { package } = &request.command else {
            return Err(Fault::new(
                ErrorCode::Internal,
                "MCP configuration command expected",
            ));
        };
        let saved = self.plugin_mcp_configuration(package.clone()).await?;
        let declared = self
            .plugins
            .read_mcp(package.clone(), self.closed.clone())
            .await?;
        self.plugin_mcp_configuration(package.clone()).await?;
        if self.closed.is_cancelled() {
            return Err(unavailable());
        }
        let configuration = saved.unwrap_or(declared);
        let (send, completion) = oneshot::channel();
        let _ = send.send(Ok(Output::PluginMcp(State {
            package: package.clone(),
            configuration,
        })));
        Ok(Admission {
            receipt: Receipt {
                id: request.id,
                durable: false,
            },
            completion,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sailry_protocol::{CredentialId, Secret};
    use std::collections::BTreeMap;

    #[test]
    fn rolls_back_failed_save() {
        let directory = tempfile::tempdir().unwrap();
        let host = crate::plugins::Host::new(Some(directory.path().canonicalize().unwrap()));
        let mut info = host.install_bundled("github").unwrap();
        info.settings = None;
        if let Some(extension) = info.extension.as_mut() {
            extension.settings_schema = None;
        }
        let db = Connection::open_in_memory().unwrap();
        db.execute_batch(include_str!("../schema.sql")).unwrap();
        super::super::save(&db, &info, true).unwrap();
        let package = info.summary.reference();
        let configuration = host
            .mcp_configuration(&package, sailry_link::CancellationToken::new())
            .unwrap();
        db.execute_batch("CREATE TEMP TRIGGER fail_save BEFORE UPDATE ON plugins WHEN NEW.revision>OLD.revision BEGIN SELECT RAISE(ABORT,'fixture failure'); END;").unwrap();
        let command = Command::SavePluginMcp {
            package: package.clone(),
            configuration: configuration.clone(),
        };
        let mut result = Ok(Output::PluginMcp(State {
            package,
            configuration,
        }));
        assert!(
            super::super::finish(&db, None, &command, &mut result)
                .unwrap()
                .is_none()
        );
        assert!(result.is_err());
        assert_eq!(super::super::required(&db, "github").unwrap(), info);
        assert_eq!(
            db.query_row(
                "SELECT settings_revision FROM plugins WHERE name='github'",
                [],
                |row| row.get::<_, i64>(0)
            )
            .unwrap(),
            0
        );
        assert_eq!(
            db.query_row("SELECT count(*) FROM plugin_settings", [], |row| row
                .get::<_, i64>(0))
                .unwrap(),
            0
        );
    }

    #[test]
    fn replacement_preserves_private_history() {
        for endpoint_changed in [false, true] {
            let directory = tempfile::tempdir().unwrap();
            let host = crate::plugins::Host::new(Some(directory.path().canonicalize().unwrap()));
            let mut info = host.install_bundled("github").unwrap();
            info.settings = None;
            if let Some(extension) = info.extension.as_mut() {
                extension.settings_schema = None;
            }
            let db = Connection::open_in_memory().unwrap();
            db.execute_batch(include_str!("../schema.sql")).unwrap();
            super::super::save(&db, &info, true).unwrap();
            let mut saved = settings::stored(&db, &info).unwrap();
            let id = CredentialId::new();
            saved.authorizations.insert(
                "github".into(),
                super::super::settings::history::McpGrant {
                    id,
                    endpoint: "https://api.githubcopilot.com/mcp/".into(),
                    revision: 1,
                    credentials: Some(Secret::new("old-oauth-grant".into())),
                },
            );
            settings::persist(&db, &mut info, &saved).unwrap();
            let admitted = info.clone();
            let configuration = Configuration {
                schema: plugin::mcp::SCHEMA.into(),
                servers: BTreeMap::from([(
                    "github".into(),
                    plugin::mcp::Server::Http {
                        exclusive: false,
                        keep_images: None,
                        reuse: true,
                        url: if endpoint_changed {
                            "https://another.example.com/mcp"
                        } else {
                            "https://api.githubcopilot.com/mcp/"
                        }
                        .into(),
                        headers: if endpoint_changed {
                            BTreeMap::new()
                        } else {
                            BTreeMap::from([("authorization".into(), "Bearer new-token".into())])
                        },
                    },
                )]),
            };
            let latest = save(&db, None, &info.summary.reference(), &configuration).unwrap();
            assert_eq!(
                settings::stored(&db, &latest).unwrap().authorizations["github"].id,
                id
            );
            assert_eq!(
                settings::stored(&db, &admitted).unwrap().authorizations["github"].id,
                id
            );
        }
    }

    #[test]
    fn preserves_server_choices() {
        let directory = tempfile::tempdir().unwrap();
        let host = crate::plugins::Host::new(Some(directory.path().canonicalize().unwrap()));
        let mut info = host.install_bundled("github").unwrap();
        info.settings = Some(
            serde_json::from_value(serde_json::json!({
                "$schema": plugin::settings::SCHEMA, "type": "object",
                "additionalProperties": false, "properties": {}
            }))
            .unwrap(),
        );
        info.extension.as_mut().unwrap().settings_schema = None;
        let db = Connection::open_in_memory().unwrap();
        db.execute_batch(include_str!("../schema.sql")).unwrap();
        super::super::save(&db, &info, true).unwrap();
        let original = info.clone();
        let disabled = set_allowed(&db, &info.summary.reference(), "github", false).unwrap();
        assert_eq!(disabled.summary.settings_revision, 1);
        let unchanged = set_allowed(&db, &disabled.summary.reference(), "github", false).unwrap();
        assert_eq!(unchanged, disabled);
        let configuration = host
            .mcp_configuration(
                &disabled.summary.reference(),
                sailry_link::CancellationToken::new(),
            )
            .unwrap();
        let configured = save(&db, None, &disabled.summary.reference(), &configuration).unwrap();
        let updated = settings::save(
            &db,
            &configured.summary.reference(),
            &BTreeMap::new(),
            &BTreeMap::new(),
        )
        .unwrap();
        let current = settings::current(&db, &updated.package).unwrap();
        assert!(!settings::stored(&db, &current).unwrap().mcp_allowed["github"]);
        assert!(!settings::resolve(&db, &current).unwrap().mcp_allowed["github"]);
        assert!(
            settings::stored(&db, &original)
                .unwrap()
                .mcp_allowed
                .is_empty()
        );
        assert!(!settings::stored(&db, &disabled).unwrap().mcp_allowed["github"]);
        assert_eq!(
            settings::stored(&db, &current).unwrap().mcp.unwrap(),
            configuration
        );
    }
}
