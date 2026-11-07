//! Team selection applies its lead model through the same Node-owned configuration.
use crate::store::{agent::providers, commands, roles, teams};
use sailry_protocol::{Effort, ErrorCode, Fault, NodeId, SessionConfig, team};
use turso::Connection;

/// Returns whether a fixed lead was selected, including an unchanged model.
pub(in crate::store) async fn configure(
    db: &Connection,
    node: NodeId,
    config: &mut SessionConfig,
    selection: Option<&team::Selection>,
) -> Result<bool, Fault> {
    let Some(selection) = selection else {
        return Ok(false);
    };
    let team = teams::get(db, selection.id)
        .await?
        .ok_or_else(|| Fault::new(ErrorCode::NotFound, "team does not exist"))?;
    if let Some(revision) = selection.revision {
        commands::check_revision(team.revision, revision)?;
    }
    let Some(model) = team.model else {
        return Ok(false);
    };
    let provider = providers::get(db, model.provider)
        .await?
        .ok_or_else(|| Fault::new(ErrorCode::NotConfigured, "team provider is unavailable"))?;
    providers::validate(db, node, &provider).await?;
    roles::validate_model(&provider, &model)?;
    let definition = provider
        .models
        .iter()
        .find(|definition| definition.id == model.model)
        .expect("team model was validated");
    let mut next = config.clone();
    next.provider = provider.id;
    next.model = model.model;
    next.credential = provider.credential.clone();
    next.effort = model.effort.unwrap_or_else(|| {
        if definition.reasoning {
            Effort::initial(&definition.efforts, definition.default_effort)
        } else {
            Effort::Default
        }
    });
    if let Some(features) = model.features {
        next.features = features;
    }
    definition.retain_features(provider.api, &mut next.features);
    providers::selection(&provider, &next)?;
    commands::validate_config(db, node, &next).await?;
    *config = next;
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::{database::Database, providers::configuration};
    use sailry_protocol::{ProviderId, TeamId, conversation::Provider, role};
    use serde_json::json;

    async fn fixture() -> (tempfile::TempDir, Database, Provider, team::Profile) {
        let root = tempfile::tempdir().unwrap();
        let database = Database::open(&root.path().join("node.turso"), NodeId([72; 32]), None)
            .await
            .unwrap();
        let provider: Provider = serde_json::from_value(json!({
            "id": ProviderId::new(), "revision": 1, "name": "Execution fixture",
            "api": "responses", "authentication": "api_key",
            "endpoint": "https://example.invalid/v1", "enabled": true,
            "models": [{
                "id": "lead", "context": 32000, "output": 8000,
                "reasoning": true, "efforts": ["default", "low", "high"],
                "default_effort": "high", "custom_efforts": true,
                "capabilities": {"features": [{
                    "id": "speed", "options": [{"id": "fast"}]
                }]}
            }],
            "default_model": "lead", "credential": null
        }))
        .unwrap();
        providers::validate(&database.connection, database.node, &provider)
            .await
            .unwrap();
        configuration::publish(&database.connection, &provider)
            .await
            .unwrap();
        let team = teams::put(
            &database.connection,
            &team::Profile {
                id: TeamId::new(),
                revision: 0,
                key: "execution".into(),
                name: "Execution fixture".into(),
                appearance: None,
                description: String::new(),
                model: Some(role::Model {
                    provider: provider.id,
                    model: "lead".into(),
                    effort: None,
                    features: None,
                }),
                instructions: String::new(),
                members: Vec::new(),
            },
            0,
        )
        .await
        .unwrap();
        (root, database, provider, team)
    }

    fn config() -> SessionConfig {
        serde_json::from_value(json!({
            "provider": ProviderId::new(), "model": "previous", "effort": "low",
            "mode": "flagship", "permission": "ask", "credential": null,
            "features": {"speed": "fast", "removed": "choice"},
            "options": {"example": {"format": "summary"}},
            "assistant": {
                "package": {"name": "example", "digest": "a".repeat(64), "settings_revision": 3},
                "id": "assistant"
            }
        }))
        .unwrap()
    }

    #[tokio::test]
    async fn applies_lead_and_preserves_scope() {
        let (_root, database, mut provider, team) = fixture().await;
        let credential = crate::store::providers::authentication::put(
            &database.connection,
            sailry_protocol::Credential {
                id: sailry_protocol::CredentialId::new(),
                provider: provider.id,
                authentication: provider.authentication,
                account_label: None,
                revision: 0,
                expires_at_ms: None,
                revoked: false,
            },
            &sailry_protocol::Secret::new("execution-fixture-key".into()),
        )
        .await
        .unwrap();
        provider.credential = Some(sailry_protocol::CredentialRef {
            node: database.node,
            id: credential.id,
        });
        configuration::publish(&database.connection, &provider)
            .await
            .unwrap();
        let mut config = config();
        let mut expected = config.clone();
        expected.provider = provider.id;
        expected.model = "lead".into();
        expected.credential = provider.credential;
        expected.effort = Effort::High;
        expected.features.remove("removed");
        let selection = team.reference().into();
        assert!(
            configure(
                &database.connection,
                database.node,
                &mut config,
                Some(&selection)
            )
            .await
            .unwrap()
        );
        assert_eq!(config, expected);
        assert!(
            configure(
                &database.connection,
                database.node,
                &mut config,
                Some(&selection)
            )
            .await
            .unwrap()
        );
        assert_eq!(config, expected);

        config.assistant = None;
        config.resource = Some(sailry_protocol::connection::Resource::Ssh(
            sailry_protocol::SshId::new(),
        ));
        let resource = config.resource;
        assert!(
            configure(
                &database.connection,
                database.node,
                &mut config,
                Some(&selection)
            )
            .await
            .unwrap()
        );
        assert_eq!(config.resource, resource);
        assert!(config.assistant.is_none());
    }

    #[tokio::test]
    async fn resolves_current_team_and_effort() {
        let (_root, database, mut provider, mut team) = fixture().await;
        let mut config = config();
        provider.models[0].default_effort = Effort::Default;
        configuration::publish(&database.connection, &provider)
            .await
            .unwrap();
        let selection = team::Selection {
            id: team.id,
            revision: None,
        };
        assert!(
            configure(
                &database.connection,
                database.node,
                &mut config,
                Some(&selection)
            )
            .await
            .unwrap()
        );
        assert_eq!(config.effort, Effort::Low);
        team.model.as_mut().unwrap().effort = Some(Effort::High);
        team = teams::put(&database.connection, &team, team.revision)
            .await
            .unwrap();
        assert!(
            configure(
                &database.connection,
                database.node,
                &mut config,
                Some(&selection)
            )
            .await
            .unwrap()
        );
        assert_eq!(config.effort, Effort::High);
        team.model.as_mut().unwrap().effort = None;
        teams::put(&database.connection, &team, team.revision)
            .await
            .unwrap();
        provider.models[0].reasoning = false;
        provider.models[0].efforts.clear();
        configuration::publish(&database.connection, &provider)
            .await
            .unwrap();
        assert!(
            configure(
                &database.connection,
                database.node,
                &mut config,
                Some(&selection)
            )
            .await
            .unwrap()
        );
        assert_eq!(config.effort, Effort::Default);
    }

    #[tokio::test]
    async fn absent_lead_preserves_configuration() {
        let (_root, database, _provider, mut team) = fixture().await;
        let mut config = config();
        let expected = config.clone();
        assert!(
            !configure(&database.connection, database.node, &mut config, None)
                .await
                .unwrap()
        );
        assert_eq!(config, expected);
        team.model = None;
        let team = teams::put(&database.connection, &team, team.revision)
            .await
            .unwrap();
        assert!(
            !configure(
                &database.connection,
                database.node,
                &mut config,
                Some(&team.reference().into())
            )
            .await
            .unwrap()
        );
        assert_eq!(config, expected);
    }

    #[tokio::test]
    async fn failed_selection_preserves_configuration() {
        let (_root, database, mut provider, team) = fixture().await;
        let mut config = config();
        let expected = config.clone();
        let stale = team::Selection {
            id: team.id,
            revision: Some(0),
        };
        assert_eq!(
            configure(
                &database.connection,
                database.node,
                &mut config,
                Some(&stale)
            )
            .await
            .unwrap_err()
            .code,
            ErrorCode::RevisionConflict
        );
        assert_eq!(config, expected);
        let selection = team.reference().into();
        provider.enabled = false;
        configuration::publish(&database.connection, &provider)
            .await
            .unwrap();
        assert_eq!(
            configure(
                &database.connection,
                database.node,
                &mut config,
                Some(&selection)
            )
            .await
            .unwrap_err()
            .code,
            ErrorCode::NotConfigured
        );
        assert_eq!(config, expected);
        provider.enabled = true;
        provider.credential = Some(sailry_protocol::CredentialRef {
            node: NodeId([73; 32]),
            id: sailry_protocol::CredentialId::new(),
        });
        configuration::publish(&database.connection, &provider)
            .await
            .unwrap();
        assert_eq!(
            configure(
                &database.connection,
                database.node,
                &mut config,
                Some(&selection)
            )
            .await
            .unwrap_err()
            .code,
            ErrorCode::PermissionDenied
        );
        assert_eq!(config, expected);
    }
}
