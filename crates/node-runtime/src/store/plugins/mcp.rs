//! Node-owned standard MCP configuration shares the plugin revision owner.
use super::*;
use crate::store::{Ingress, Job, unavailable};
use sailry_link::Admission;
use sailry_protocol::{
    Receipt, Request, VERSION,
    plugin::mcp::{Configuration, State},
};
use tokio::sync::oneshot;

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
    package: &plugin::Reference,
    configuration: &Configuration,
) -> Result<Info, Fault> {
    let mut info = settings::current(db, package)?;
    if info.mcp_source.is_some() {
        return Err(Fault::new(
            ErrorCode::InvalidRequest,
            "standalone MCP settings use their declared form",
        ));
    }
    let mut saved = settings::stored(db, &info)?;
    saved.authorizations.retain(|server, grant| {
        configuration
            .servers
            .get(server)
            .is_some_and(|server| match server {
                plugin::mcp::Server::Http { url, headers }
                | plugin::mcp::Server::Sse { url, headers } => {
                    *url == grant.endpoint
                        && !headers.iter().any(|(name, value)| {
                            name.eq_ignore_ascii_case("authorization") && !value.is_empty()
                        })
                }
                plugin::mcp::Server::Stdio { .. } => false,
            })
    });
    // Bound form fields remain the sole editor for their declared MCP slots.
    if let Some(schema) = &info.settings {
        for field in schema
            .properties
            .values()
            .filter_map(|field| field.secret.as_ref())
        {
            if field.server.is_some() && (field.header.is_some() || field.env.is_some()) {
                return Err(Fault::new(
                    ErrorCode::InvalidRequest,
                    "MCP slots use the plugin settings form",
                ));
            }
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
        let info = host.install_bundled("github").unwrap();
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
            super::super::finish(&db, &command, &mut result)
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
    fn replacement_preserves_admitted_authorization() {
        for endpoint_changed in [false, true] {
            let directory = tempfile::tempdir().unwrap();
            let host = crate::plugins::Host::new(Some(directory.path().canonicalize().unwrap()));
            let mut info = host.install_bundled("github").unwrap();
            let db = Connection::open_in_memory().unwrap();
            db.execute_batch(include_str!("../schema.sql")).unwrap();
            super::super::save(&db, &info, true).unwrap();
            let mut saved = settings::stored(&db, &info).unwrap();
            let id = CredentialId::new();
            saved.authorizations.insert(
                "github".into(),
                super::super::grants::Grant {
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
            let latest = save(&db, &info.summary.reference(), &configuration).unwrap();
            assert!(
                settings::stored(&db, &latest)
                    .unwrap()
                    .authorizations
                    .is_empty()
            );
            assert_eq!(
                settings::stored(&db, &admitted).unwrap().authorizations["github"].id,
                id
            );
            assert_eq!(
                super::super::grants::load(
                    &db,
                    "github",
                    "github",
                    "https://api.githubcopilot.com/mcp/",
                    id
                )
                .unwrap()
                .1
                .expose(),
                "old-oauth-grant"
            );
        }
    }
}
