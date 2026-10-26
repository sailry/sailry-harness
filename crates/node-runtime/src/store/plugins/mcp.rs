//! Node-owned standard MCP configuration shares the plugin revision owner.
use super::*;
use crate::store::{Ingress, Job, unavailable};
use sailry_link::Admission;
use sailry_protocol::{
    Receipt, Request,
    plugin::mcp::{Configuration, State},
};
use tokio::sync::oneshot;
mod availability;
pub(super) use availability::project;

pub(in crate::store) async fn prepare(
    db: &Connection,
    package: &plugin::Reference,
    server: &str,
) -> Result<(Info, crate::plugins::settings::Resolved), Fault> {
    let info = settings::current(db, package).await?;
    if !info.summary.enabled {
        return Err(Fault::new(
            ErrorCode::NotConfigured,
            "MCP package is disabled",
        ));
    }
    if !info.mcp.iter().any(|item| item.name == server) {
        return Err(Fault::new(ErrorCode::NotFound, "MCP server is unavailable"));
    }
    let settings = settings::resolve(db, &info).await?;
    Ok((info, settings))
}

pub(super) async fn set_allowed(
    db: &Connection,
    profile: Option<&std::path::Path>,
    package: &plugin::Reference,
    server: &str,
    allowed: bool,
) -> Result<Info, Fault> {
    let mut info = settings::current(db, package).await?;
    if !info.mcp.iter().any(|item| item.name == server) {
        return Err(Fault::new(ErrorCode::NotFound, "MCP server is unavailable"));
    }
    let mut saved = settings::stored(db, &info).await?;
    let enabled = allowed && connectors::mcp::enable(db, profile, &info, server).await?;
    if saved.mcp_allowed.get(server).copied().unwrap_or(true) != allowed || enabled {
        saved.mcp_allowed.insert(server.into(), allowed);
        settings::persist(db, &mut info, &saved).await?;
    }
    Ok(info)
}

pub(in crate::store) async fn configuration(
    db: &Connection,
    package: &plugin::Reference,
) -> Result<Option<Configuration>, Fault> {
    let info = settings::current(db, package).await?;
    if info.mcp.is_empty() {
        return Err(Fault::new(
            ErrorCode::NotConfigured,
            "plugin has no MCP servers",
        ));
    }
    Ok(settings::stored(db, &info).await?.mcp)
}

pub(super) async fn save(
    db: &Connection,
    profile: Option<&std::path::Path>,
    package: &plugin::Reference,
    configuration: &Configuration,
) -> Result<Info, Fault> {
    let mut info = settings::current(db, package).await?;
    if info.mcp_source.is_some() {
        return Err(Fault::invalid(
            "standalone MCP settings use their declared form",
        ));
    }
    let mut saved = settings::stored(db, &info).await?;
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
    settings::persist(db, &mut info, &saved).await?;
    Ok(info)
}

impl Ingress {
    async fn plugin_mcp_configuration(
        &self,
        package: plugin::Reference,
    ) -> Result<Option<Configuration>, Fault> {
        let (reply, response) = oneshot::channel();
        self.submit(Job::McpCredentials(
            crate::store::mcp::Operation::Configuration { package, reply },
        ))
        .await
        .map_err(|_| unavailable())?;
        response.await.map_err(|_| unavailable())?
    }

    pub(in crate::store) async fn read_plugin_mcp(
        &self,
        request: Request,
    ) -> Result<Admission, Fault> {
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

    #[tokio::test]
    async fn rolls_back_failed_save() {
        let directory = tempfile::tempdir().unwrap();
        let host = crate::plugins::Host::new(Some(directory.path().canonicalize().unwrap()));
        let mut info = host.install_bundled("github").unwrap();
        info.settings = None;
        if let Some(extension) = info.extension.as_mut() {
            extension.settings_schema = None;
        }
        let engine = crate::store::database::engine(std::path::Path::new(":memory:"))
            .await
            .unwrap();
        let db = crate::store::database::connect(&engine).await.unwrap();
        db.execute_batch(include_str!("../schema.sql"))
            .await
            .unwrap();
        super::super::save(&db, &info, true).await.unwrap();
        let package = info.summary.reference();
        let configuration = host
            .mcp_configuration(&package, sailry_link::CancellationToken::new())
            .unwrap();
        db.execute_batch("CREATE TEMP TRIGGER fail_save BEFORE UPDATE ON plugins WHEN NEW.revision>OLD.revision BEGIN SELECT RAISE(ABORT,'fixture failure'); END;").await.unwrap();
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
                .await
                .unwrap()
                .is_none()
        );
        assert!(result.is_err());
        assert_eq!(super::super::required(&db, "github").await.unwrap(), info);
        assert_eq!(
            (async {
                let mut statement = db
                    .prepare("SELECT settings_revision FROM plugins WHERE name='github'")
                    .await?;
                let row = statement.query_row(()).await?;
                let mapped: turso::Result<_> = row.get::<i64>(0);
                mapped
            })
            .await
            .unwrap(),
            0
        );
        assert_eq!(
            (async {
                let mut statement = db.prepare("SELECT count(*) FROM plugin_settings").await?;
                let row = statement.query_row(()).await?;
                let mapped: turso::Result<_> = row.get::<i64>(0);
                mapped
            })
            .await
            .unwrap(),
            0
        );
    }

    #[tokio::test]
    async fn replacement_preserves_private_history() {
        for endpoint_changed in [false, true] {
            let directory = tempfile::tempdir().unwrap();
            let host = crate::plugins::Host::new(Some(directory.path().canonicalize().unwrap()));
            let mut info = host.install_bundled("github").unwrap();
            info.settings = None;
            if let Some(extension) = info.extension.as_mut() {
                extension.settings_schema = None;
            }
            let engine = crate::store::database::engine(std::path::Path::new(":memory:"))
                .await
                .unwrap();
            let db = crate::store::database::connect(&engine).await.unwrap();
            db.execute_batch(include_str!("../schema.sql"))
                .await
                .unwrap();
            super::super::save(&db, &info, true).await.unwrap();
            let mut saved = settings::stored(&db, &info).await.unwrap();
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
            settings::persist(&db, &mut info, &saved).await.unwrap();
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
            let latest = save(&db, None, &info.summary.reference(), &configuration)
                .await
                .unwrap();
            assert_eq!(
                settings::stored(&db, &latest).await.unwrap().authorizations["github"].id,
                id
            );
            assert_eq!(
                settings::stored(&db, &admitted)
                    .await
                    .unwrap()
                    .authorizations["github"]
                    .id,
                id
            );
        }
    }

    #[tokio::test]
    async fn preserves_server_choices() {
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
        let engine = crate::store::database::engine(std::path::Path::new(":memory:"))
            .await
            .unwrap();
        let db = crate::store::database::connect(&engine).await.unwrap();
        db.execute_batch(include_str!("../schema.sql"))
            .await
            .unwrap();
        super::super::save(&db, &info, true).await.unwrap();
        let original = info.clone();
        let disabled = set_allowed(
            &db,
            host.profile(),
            &info.summary.reference(),
            "github",
            false,
        )
        .await
        .unwrap();
        assert_eq!(disabled.summary.settings_revision, 1);
        let unchanged = set_allowed(
            &db,
            host.profile(),
            &disabled.summary.reference(),
            "github",
            false,
        )
        .await
        .unwrap();
        assert_eq!(unchanged, disabled);
        let configuration = host
            .mcp_configuration(
                &disabled.summary.reference(),
                sailry_link::CancellationToken::new(),
            )
            .unwrap();
        let configured = save(&db, None, &disabled.summary.reference(), &configuration)
            .await
            .unwrap();
        let transaction = db.unchecked_transaction().await.unwrap();
        let updated = settings::save(
            &transaction,
            &configured.summary.reference(),
            &BTreeMap::new(),
            &BTreeMap::new(),
        )
        .await
        .unwrap();
        transaction.commit().await.unwrap();
        let current = settings::current(&db, &updated.package).await.unwrap();
        assert!(!settings::stored(&db, &current).await.unwrap().mcp_allowed["github"]);
        assert!(!settings::resolve(&db, &current).await.unwrap().mcp_allowed["github"]);
        assert!(
            settings::stored(&db, &original)
                .await
                .unwrap()
                .mcp_allowed
                .is_empty()
        );
        assert!(!settings::stored(&db, &disabled).await.unwrap().mcp_allowed["github"]);
        assert_eq!(
            settings::stored(&db, &current).await.unwrap().mcp.unwrap(),
            configuration
        );
    }
}
