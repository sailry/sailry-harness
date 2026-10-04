//! Standalone servers use ordinary immutable packages and inline Node configuration.
use super::{Host, invalid, io_error, manifest, mcp};
use sailry_protocol::{
    Fault,
    plugin::{
        Info,
        mcp::Definition,
        settings::{self, SecretUpdate},
    },
};
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};

const SERVER: &str = "server";

impl Host {
    pub(crate) fn install_mcp(&self, name: &str, definition: &Definition) -> Result<Info, Fault> {
        super::validate_name(name)?;
        if name.strip_prefix("mcp-").is_none_or(str::is_empty) {
            return Err(invalid("standalone MCP names must start with mcp-"));
        }
        let (configuration, schema) = documents(definition)?;
        let temporary = tempfile::tempdir().map_err(io_error)?;
        let directory = temporary.path().join("package");
        std::fs::create_dir(&directory).map_err(io_error)?;
        std::fs::create_dir(directory.join(sailry_protocol::plugin::NAMESPACE))
            .map_err(io_error)?;
        for (path, value) in [
            (
                "plugin.json",
                json!({
                    "$schema": manifest::SCHEMA,
                    "name": name,
                    "extensions": { "dev.sailry.platform": {
                        "api_version": sailry_protocol::plugin::API_VERSION,
                        "actions": [], "desktop": null,
                        "settings_schema": "dev.sailry.platform/settings.json", "settings_page": null,
                    }},
                }),
            ),
            ("mcp.json", configuration),
            ("dev.sailry.platform/settings.json", schema),
        ] {
            let bytes =
                serde_json::to_vec(&value).map_err(|_| invalid("invalid MCP configuration"))?;
            std::fs::write(directory.join(path), bytes).map_err(io_error)?;
        }
        let root = temporary.path().canonicalize().map_err(io_error)?;
        let mut info = self.install(&root, "package", name)?;
        if !info.issues.is_empty() || info.mcp.len() != 1 || info.settings.is_none() {
            return Err(invalid("invalid standalone MCP configuration"));
        }
        info.mcp_source = Some(definition.clone());
        Ok(info)
    }
}

/// Every slot must carry explicit keep, replace, or clear intent.
pub(crate) fn updates(
    definition: &Definition,
    secrets: &BTreeMap<String, SecretUpdate>,
) -> Result<BTreeMap<String, SecretUpdate>, Fault> {
    let slots = definition.slots();
    if secrets.len() != slots.len() || slots.iter().any(|name| !secrets.contains_key(name)) {
        return Err(invalid(
            "every MCP slot requires an explicit credential update",
        ));
    }
    Ok(secrets
        .iter()
        .map(|(name, update)| (field_name(name), update.clone()))
        .collect())
}

fn field_name(name: &str) -> String {
    // Names remain stable when another slot is inserted or removed.
    format!("slot_{}", &blake3::hash(name.as_bytes()).to_hex()[..32])
}

fn documents(definition: &Definition) -> Result<(Value, Value), Fault> {
    let slots = definition.slots();
    if slots.len() > settings::MAX_FIELDS
        || slots.iter().collect::<BTreeSet<_>>().len() != slots.len()
    {
        return Err(invalid("invalid MCP credential slots"));
    }
    let empty: BTreeMap<_, _> = slots.iter().map(|name| (name, "")).collect();
    let server = match definition {
        Definition::Stdio { command, args, .. } => json!({
            "type": "stdio", "command": command, "args": args, "env": empty,
        }),
        Definition::StreamableHttp { url, .. } => json!({
            "type": "streamable-http", "url": url, "headers": empty,
        }),
        Definition::Sse { url, .. } => json!({
            "type": "sse", "url": url, "headers": empty,
        }),
    };
    let configuration = json!({"$schema": mcp::SCHEMA, "mcpServers": {SERVER: server}});
    let bytes =
        serde_json::to_vec(&configuration).map_err(|_| invalid("invalid MCP configuration"))?;
    let mut issues = Vec::new();
    let servers = mcp::parse(&bytes, &mut issues)?;
    if !issues.is_empty() || servers.len() != 1 {
        return Err(invalid("invalid MCP server configuration"));
    }
    let properties: BTreeMap<_, _> = slots
        .iter()
        .map(|name| {
            let binding = match definition {
                Definition::Stdio { .. } => json!({"server": SERVER, "env": name, "header": null}),
                _ => json!({"server": SERVER, "env": null, "header": name}),
            };
            (
                field_name(name),
                json!({
                    "type": "string", "title": name, "maxLength": settings::MAX_STRING,
                    "x-sailry-secret": binding,
                }),
            )
        })
        .collect();
    let schema = json!({
        "$schema": settings::SCHEMA, "type": "object", "properties": properties,
        "additionalProperties": false,
    });
    Ok((configuration, schema))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn declares_empty_slots() {
        let profile = tempfile::tempdir().unwrap();
        let host = Host::new(Some(profile.path().canonicalize().unwrap()));
        for definition in [
            Definition::Stdio {
                command: "npx".into(),
                args: vec!["-y".into(), "server".into()],
                env: vec!["TOKEN".into()],
            },
            Definition::StreamableHttp {
                url: "https://example.com/mcp".into(),
                headers: vec!["Authorization".into()],
            },
            Definition::Sse {
                url: "https://example.com/sse".into(),
                headers: vec!["X-Api-Key".into()],
            },
        ] {
            let info = host.install_mcp("mcp-test", &definition).unwrap();
            assert_eq!(info.mcp_source.as_ref(), Some(&definition));
            assert_eq!(info.mcp[0].transport, definition.transport());
            let schema = info.settings.unwrap();
            let slot = &definition.slots()[0];
            let field = &schema.properties[&field_name(slot)];
            assert!(field.default.is_none());
            assert!(field.secret.is_some());
            let bytes = std::fs::read(
                profile
                    .path()
                    .join("plugins/packages")
                    .join(info.summary.digest)
                    .join("mcp.json"),
            )
            .unwrap();
            let config: Value = serde_json::from_slice(&bytes).unwrap();
            let key = if matches!(definition, Definition::Stdio { .. }) {
                "env"
            } else {
                "headers"
            };
            assert_eq!(config["mcpServers"][SERVER][key][slot], "");
        }
    }

    #[test]
    fn preserves_slot_identity() {
        let definition = Definition::Stdio {
            command: "npx".into(),
            args: Vec::new(),
            env: vec!["TOKEN".into()],
        };
        let secrets = BTreeMap::from([("TOKEN".into(), SecretUpdate::Keep)]);
        let mapped = updates(&definition, &secrets).unwrap();
        let enlarged = Definition::Stdio {
            command: "npx".into(),
            args: Vec::new(),
            env: vec!["OTHER".into(), "TOKEN".into()],
        };
        let mut secrets = secrets;
        secrets.insert("OTHER".into(), SecretUpdate::Clear);
        assert_eq!(
            updates(&enlarged, &secrets).unwrap()[&field_name("TOKEN")],
            mapped[&field_name("TOKEN")]
        );
        assert!(updates(&definition, &secrets).is_err());
    }

    #[test]
    fn rejects_invalid_definitions() {
        let profile = tempfile::tempdir().unwrap();
        let host = Host::new(Some(profile.path().canonicalize().unwrap()));
        for definition in [
            Definition::Stdio {
                command: "npx server".into(),
                args: Vec::new(),
                env: Vec::new(),
            },
            Definition::Stdio {
                command: "npx".into(),
                args: Vec::new(),
                env: vec!["TOKEN".into(), "token".into()],
            },
            Definition::StreamableHttp {
                url: "https://example.com/mcp".into(),
                headers: vec!["Authorization".into(), "authorization".into()],
            },
        ] {
            assert!(host.install_mcp("mcp-test", &definition).is_err());
        }
    }
}
