//! Connector reads classify captured declarations and stored presence without probing them.
use super::*;
use sailry_protocol::plugin::{
    Info, McpTransport,
    connection::{Authorization, Configuration, Credential, Status},
    settings::{Binding, Schema, State},
};

pub(in crate::store) fn read(
    db: &Connection,
    package: &plugin::Reference,
    card: &str,
) -> Result<Status, Fault> {
    let info = settings::current(db, package)?;
    read_admitted(db, &info, card)
}

pub(in crate::store) fn read_admitted(
    db: &Connection,
    info: &Info,
    card: &str,
) -> Result<Status, Fault> {
    let extension = info.extension.as_ref().ok_or_else(|| {
        Fault::new(
            ErrorCode::NotConfigured,
            "plugin has no connector declarations",
        )
    })?;
    let declaration = extension
        .cards
        .iter()
        .find(|declaration| declaration.key == card)
        .ok_or_else(|| Fault::new(ErrorCode::NotFound, "plugin card was not found"))?;
    let server = declaration
        .mcp_server
        .as_deref()
        .ok_or_else(|| Fault::new(ErrorCode::NotConfigured, "plugin card has no MCP server"))?;
    if !info
        .mcp
        .iter()
        .any(|entry| entry.name == server && entry.transport != McpTransport::Stdio)
    {
        return Err(Fault::new(
            ErrorCode::NotConfigured,
            "plugin card has no HTTP MCP server",
        ));
    }
    let stored = settings::stored(db, info)?;
    let state = info
        .settings
        .as_ref()
        .map(|_| settings::read_admitted(db, info))
        .transpose()?;
    let configuration = configuration(info, &stored, state.as_ref(), server);
    let authorization = if !declaration.mcp_authorization() {
        Authorization::NotDeclared
    } else if stored
        .authorizations
        .get(server)
        .is_some_and(|grant| grant.credentials.is_some())
    {
        Authorization::Configured
    } else {
        Authorization::Missing
    };
    let credential = credential(info, &stored, state.as_ref(), server);
    Ok(Status {
        package: info.summary.reference(),
        card: card.into(),
        server: server.into(),
        configuration,
        authorization,
        credential,
    })
}

fn configuration(
    info: &Info,
    stored: &settings::Stored,
    state: Option<&State>,
    server: &str,
) -> Configuration {
    let extension = info
        .extension
        .as_ref()
        .expect("connector declarations require an extension");
    let Some(schema) = info.settings.as_ref() else {
        return if extension.settings_schema.is_some() {
            Configuration::Unsupported
        } else {
            Configuration::Ready
        };
    };
    if schema.properties.values().any(|field| {
        field
            .secret
            .as_ref()
            .is_some_and(|binding| binding.unsupported)
    }) {
        return Configuration::Unsupported;
    }
    if stored.schema.as_ref() != Some(schema) {
        return Configuration::Changed;
    }
    let state = state.expect("settings state accompanies a parsed schema");
    if !state.ready || binding_missing(schema, state, server) {
        Configuration::Missing
    } else {
        Configuration::Ready
    }
}

fn binding_missing(schema: &Schema, state: &State, server: &str) -> bool {
    schema.properties.iter().any(|(name, field)| {
        let Some(binding) = field.secret.as_ref().filter(|binding| {
            binding.server.as_deref() == Some(server)
                && (binding.oauth.is_some() || binding.application.is_some())
        }) else {
            return false;
        };
        !state.configured.iter().any(|configured| configured == name)
            || client_id(binding)
                .and_then(|name| state.values.get(name))
                .and_then(serde_json::Value::as_str)
                .is_none_or(|id| {
                    id.is_empty() || id.len() > 2048 || id.chars().any(char::is_control)
                })
    })
}

fn client_id(binding: &Binding) -> Option<&str> {
    binding
        .oauth
        .as_ref()
        .map(|declaration| declaration.client_id.as_str())
        .or_else(|| {
            binding
                .application
                .as_ref()
                .map(|declaration| declaration.client_id.as_str())
        })
}

fn credential(
    info: &Info,
    stored: &settings::Stored,
    state: Option<&State>,
    server: &str,
) -> Credential {
    let mut declared = false;
    let mut present = false;
    if let Some(schema) = &info.settings {
        for (name, field) in &schema.properties {
            let Some(_binding) = field.secret.as_ref().filter(|binding| {
                !binding.unsupported
                    && binding.server.as_deref() == Some(server)
                    && (binding.header.is_some()
                        || binding.application.is_some()
                        || binding.oauth.is_some())
            }) else {
                continue;
            };
            declared = true;
            present |= state
                .is_some_and(|state| state.configured.iter().any(|configured| configured == name));
        }
    }
    if info
        .extension
        .as_ref()
        .is_some_and(|extension| extension.credential_source.is_some())
    {
        declared = true;
        present |= stored.github.is_some();
    }
    match (declared, present) {
        (false, _) => Credential::NotDeclared,
        (true, false) => Credential::Absent,
        (true, true) => Credential::Present,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sailry_protocol::Secret;
    use serde_json::json;
    use std::collections::BTreeMap;

    fn info(schema: Option<Schema>, authorization: bool) -> Info {
        let browser = if authorization {
            json!([{
                "name":"login",
                "origins":["https://example.test"],
                "source":{"kind":"mcp_authorization"},
                "modes":["external"]
            }])
        } else {
            json!([])
        };
        let mut value = json!({
            "summary":{
                "name":"connector",
                "revision":1,
                "digest":"a".repeat(64),
                "settings_revision":0,
                "enabled":true,
                "version":"0.1.0",
                "description":null
            },
            "icon":null,
            "extension":{
                "api_version":"v1",
                "actions":[],
                "desktop":{"resources":["dev.sailry.platform/card.js"]},
                "cards":[{
                    "key":"connection",
                    "entry":"dev.sailry.platform/card.js",
                    "mcp_server":"input",
                    "props_schema":{
                        "type":"object",
                        "additionalProperties":false,
                        "properties":{}
                    },
                    "capabilities":{"browser":browser}
                }]
            },
            "skills":[],
            "mcp":[{"name":"input","transport":"streamable-http"}],
            "settings":schema,
            "issues":[]
        });
        if value["settings"].is_object() {
            value["extension"]["settings_schema"] = json!("dev.sailry.platform/settings.json");
        }
        serde_json::from_value(value).unwrap()
    }

    fn schema(required: bool, oauth: bool) -> Schema {
        let binding = if oauth {
            json!({"server":"input","oauth":{"client_id":"client_id"}})
        } else {
            json!({"server":"input","header":"Authorization","prefix":"Bearer "})
        };
        let mut properties = json!({
            "token":{"type":"string","x-sailry-secret":binding}
        });
        if oauth {
            properties["client_id"] = json!({"type":"string","default":""});
        }
        serde_json::from_value(json!({
            "$schema":plugin::settings::SCHEMA,
            "type":"object",
            "additionalProperties":false,
            "properties":properties,
            "required":if required { vec!["token"] } else { Vec::<&str>::new() }
        }))
        .unwrap()
    }

    fn database(info: &Info) -> Connection {
        let db = Connection::open_in_memory().unwrap();
        db.execute_batch(include_str!("../schema.sql")).unwrap();
        super::super::save(&db, info, true).unwrap();
        db
    }

    #[test]
    fn classifies_declared_authorization_presence() {
        let info = info(None, true);
        let db = database(&info);
        let missing = read(&db, &info.summary.reference(), "connection").unwrap();
        assert_eq!(missing.configuration, Configuration::Ready);
        assert_eq!(missing.authorization, Authorization::Missing);
        assert_eq!(missing.credential, Credential::NotDeclared);
        let current = super::super::authorization::save(
            &db,
            &info.summary.reference(),
            "input",
            "https://example.test/mcp",
            Secret::new("private-grant".into()),
        )
        .unwrap();
        let configured = read(&db, &current.summary.reference(), "connection").unwrap();
        assert_eq!(configured.authorization, Authorization::Configured);
        let encoded = serde_json::to_string(&configured).unwrap();
        assert!(!encoded.contains("private-grant"));
        assert!(!encoded.contains("example.test/mcp"));
    }

    #[test]
    fn keeps_optional_credentials_ready() {
        let info = info(Some(schema(false, false)), false);
        let db = database(&info);
        let absent = read(&db, &info.summary.reference(), "connection").unwrap();
        assert_eq!(absent.configuration, Configuration::Ready);
        assert_eq!(absent.authorization, Authorization::NotDeclared);
        assert_eq!(absent.credential, Credential::Absent);
        let state = super::super::settings::save(
            &db,
            &info.summary.reference(),
            &BTreeMap::new(),
            &BTreeMap::from([(
                "token".into(),
                plugin::settings::SecretUpdate::Replace(Secret::new("private-token".into())),
            )]),
        )
        .unwrap();
        let present = read(&db, &state.package, "connection").unwrap();
        assert_eq!(present.configuration, Configuration::Ready);
        assert_eq!(present.credential, Credential::Present);
        assert!(
            !serde_json::to_string(&present)
                .unwrap()
                .contains("private-token")
        );
    }

    #[test]
    fn requires_declared_binding_inputs() {
        for schema in [schema(true, false), schema(false, true)] {
            let info = info(Some(schema), false);
            let db = database(&info);
            let status = read(&db, &info.summary.reference(), "connection").unwrap();
            assert_eq!(status.configuration, Configuration::Missing);
            assert_eq!(status.credential, Credential::Absent);
        }
    }

    #[test]
    fn distinguishes_changed_and_unsupported_schemas() {
        let mut info = info(Some(schema(false, false)), false);
        let db = database(&info);
        let saved = super::super::settings::save(
            &db,
            &info.summary.reference(),
            &BTreeMap::new(),
            &BTreeMap::from([("token".into(), plugin::settings::SecretUpdate::Clear)]),
        )
        .unwrap();
        info.summary = super::super::required(&db, "connector").unwrap().summary;
        info.settings.as_mut().unwrap().title = Some("Changed".into());
        super::super::save(&db, &info, true).unwrap();
        let changed = read(&db, &info.summary.reference(), "connection").unwrap();
        assert_eq!(changed.configuration, Configuration::Changed);
        assert_eq!(
            saved.package.settings_revision,
            info.summary.settings_revision
        );

        info.settings = None;
        super::super::save(&db, &info, true).unwrap();
        let unsupported = read(&db, &info.summary.reference(), "connection").unwrap();
        assert_eq!(unsupported.configuration, Configuration::Unsupported);
    }

    #[test]
    fn status_has_only_bounded_classification() {
        let status = Status {
            package: plugin::Reference {
                name: "connector".into(),
                digest: "a".repeat(64),
                settings_revision: 1,
            },
            card: "connection".into(),
            server: "input".into(),
            configuration: Configuration::Ready,
            authorization: Authorization::Configured,
            credential: Credential::Present,
        };
        let value = serde_json::to_value(status).unwrap();
        assert_eq!(value.as_object().unwrap().len(), 6);
        assert_eq!(value["configuration"], "ready");
        assert_eq!(value["authorization"], "configured");
        assert_eq!(value["credential"], "present");
    }
}
