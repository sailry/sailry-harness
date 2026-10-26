//! Role services project public definitions, never frozen provider configuration.
use super::*;
use sailry_protocol::{Output, role};

pub(super) fn handles(operation: Operation) -> bool {
    matches!(
        operation,
        Operation::ListRoles | Operation::SaveRole | Operation::RemoveRole | Operation::ApplyRoles
    )
}

pub(super) fn parameters(operation: Operation) -> Value {
    let revision = json!({"type":"string","pattern":"^(0|[1-9][0-9]*)$"});
    let (properties, required) = match operation {
        Operation::ListRoles => (json!({}), json!([])),
        Operation::SaveRole => (
            json!({"role":{"type":"object"},"expected_revision":revision}),
            json!(["role", "expected_revision"]),
        ),
        Operation::RemoveRole => (
            json!({"role":{"type":"string"},"expected_revision":revision}),
            json!(["role", "expected_revision"]),
        ),
        Operation::ApplyRoles => (
            json!({"roles":{"type":"array","items":{"type":"object","additionalProperties":false,"required":["id","revision"],"properties":{"id":{"type":"string"},"revision":revision}}},"expected_revision":revision}),
            json!(["roles", "expected_revision"]),
        ),
        _ => unreachable!(),
    };
    json!({"type":"object","additionalProperties":false,"properties":properties,"required":required})
}

fn revision(value: &str) -> Result<u64, Fault> {
    let number = value
        .parse::<u64>()
        .map_err(|_| Fault::invalid("invalid role revision"))?;
    if number.to_string() != value || number > i64::MAX as u64 {
        return Err(Fault::invalid("invalid role revision"));
    }
    Ok(number)
}

pub(super) fn command(
    operation: Operation,
    arguments: Value,
    context: &plugin::Context,
) -> Result<Command, Fault> {
    let invalid = |_| Fault::invalid("invalid role arguments");
    match operation {
        Operation::ListRoles => {
            #[derive(serde::Deserialize)]
            #[serde(deny_unknown_fields)]
            struct Input {}
            let _: Input = serde_json::from_value(arguments).map_err(invalid)?;
            Ok(Command::ListRoles)
        }
        Operation::SaveRole => {
            #[derive(serde::Deserialize)]
            #[serde(deny_unknown_fields)]
            struct Input {
                role: Value,
                expected_revision: String,
            }
            let mut input: Input = serde_json::from_value(arguments).map_err(invalid)?;
            let current = input
                .role
                .get("revision")
                .and_then(Value::as_str)
                .ok_or_else(|| Fault::invalid("role revision must be a decimal string"))?;
            input.role["revision"] = json!(revision(current)?);
            let role: role::Profile = serde_json::from_value(input.role).map_err(invalid)?;
            role.validate()?;
            Ok(Command::PutRole {
                role,
                expected_revision: revision(&input.expected_revision)?,
            })
        }
        Operation::RemoveRole => {
            #[derive(serde::Deserialize)]
            #[serde(deny_unknown_fields)]
            struct Input {
                role: sailry_protocol::RoleId,
                expected_revision: String,
            }
            let input: Input = serde_json::from_value(arguments).map_err(invalid)?;
            Ok(Command::RemoveRole {
                role: input.role,
                expected_revision: revision(&input.expected_revision)?,
            })
        }
        Operation::ApplyRoles => {
            #[derive(serde::Deserialize)]
            #[serde(deny_unknown_fields)]
            struct Reference {
                id: sailry_protocol::RoleId,
                revision: String,
            }
            #[derive(serde::Deserialize)]
            #[serde(deny_unknown_fields)]
            struct Input {
                roles: Vec<Reference>,
                expected_revision: String,
            }
            let input: Input = serde_json::from_value(arguments).map_err(invalid)?;
            let ids: std::collections::BTreeSet<_> =
                input.roles.iter().map(|role| role.id).collect();
            if ids.len() != input.roles.len() {
                return Err(Fault::invalid("session roles must be distinct"));
            }
            let roles = input
                .roles
                .into_iter()
                .map(|reference| {
                    Ok(role::Reference {
                        id: reference.id,
                        revision: revision(&reference.revision)?,
                    })
                })
                .collect::<Result<Vec<_>, Fault>>()?;
            Ok(Command::SetSessionRoles {
                session: context
                    .session
                    .ok_or_else(|| Fault::invalid("role application requires a session"))?,
                expected_revision: revision(&input.expected_revision)?,
                roles,
            })
        }
        _ => unreachable!(),
    }
}

pub(super) fn output(output: Output) -> Result<Value, Fault> {
    let profile = |profile: role::Profile| {
        let mut value = json!(profile);
        value["revision"] = json!(profile.revision.to_string());
        value
    };
    match output {
        Output::Roles(roles) => {
            Ok(json!({"kind":"roles","data":roles.into_iter().map(profile).collect::<Vec<_>>()}))
        }
        Output::Role(role) => Ok(json!({"kind":"role","data":profile(role)})),
        Output::Session(session) => Ok(
            json!({"kind":"session_roles","data":{"session":session.id,"revision":session.revision.to_string(),"roles":session.roles.profiles.iter().map(|role| json!({"id":role.id,"revision":role.revision.to_string()})).collect::<Vec<_>>()}}),
        ),
        _ => Err(Fault::new(
            ErrorCode::Internal,
            "role outcome is unavailable",
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scope() -> plugin::Context {
        plugin::Context {
            invocation: None,
            turn: Some(TurnId::new()),
            surface: Default::default(),
            package: plugin::Reference {
                name: "roles-example".into(),
                digest: "a".repeat(64),
                settings_revision: 0,
            },
            worktree: Some(sailry_protocol::WorktreeId::new()),
            session: Some(sailry_protocol::SessionId::new()),
        }
    }

    #[test]
    fn application_preserves_captured_session() {
        let context = scope();
        let id = sailry_protocol::RoleId::new();
        let arguments = json!({"roles":[{"id":id,"revision":"9007199254740993"}],"expected_revision":"9007199254740993"});
        assert!(
            matches!(command(Operation::ApplyRoles,arguments.clone(),&context).unwrap(),Command::SetSessionRoles {session,expected_revision:9007199254740993,roles} if Some(session)==context.session && roles[0].revision==9007199254740993)
        );
        for field in ["session", "turn", "worktree", "package"] {
            let mut changed = arguments.clone();
            changed[field] = json!("foreign");
            assert_eq!(
                command(Operation::ApplyRoles, changed, &context)
                    .unwrap_err()
                    .code,
                ErrorCode::InvalidRequest
            );
        }
        for value in [
            json!(9007199254740992_u64),
            json!("01"),
            json!("9223372036854775808"),
        ] {
            let mut changed = arguments.clone();
            changed["expected_revision"] = value;
            assert_eq!(
                command(Operation::ApplyRoles, changed, &context)
                    .unwrap_err()
                    .code,
                ErrorCode::InvalidRequest
            );
        }
        let mut duplicate = arguments.clone();
        duplicate["roles"] = json!([{"id":id,"revision":"1"},{"id":id,"revision":"1"}]);
        assert_eq!(
            command(Operation::ApplyRoles, duplicate, &context)
                .unwrap_err()
                .code,
            ErrorCode::InvalidRequest
        );
        assert!(
            matches!(command(Operation::ApplyRoles,json!({"roles":[],"expected_revision":"1"}),&context).unwrap(),Command::SetSessionRoles {roles,..} if roles.is_empty())
        );
    }

    #[test]
    fn definitions_keep_exact_revisions() {
        let profile = role::Profile {
            id: sailry_protocol::RoleId::new(),
            revision: 9007199254740993,
            key: "review".into(),
            name: "Review".into(),
            appearance: None,
            description: String::new(),
            read_only: true,
            model: None,
            max_turns: None,
            skills: vec![],
            tools: None,
            instructions: "Inspect".into(),
        };
        let rendered = output(Output::Role(profile.clone())).unwrap();
        assert_eq!(rendered["data"]["revision"], "9007199254740993");
        let prepared = command(
            Operation::SaveRole,
            json!({"role":rendered["data"],"expected_revision":"9007199254740993"}),
            &scope(),
        )
        .unwrap();
        assert!(
            matches!(prepared,Command::PutRole {role,expected_revision:9007199254740993} if role==profile)
        );
        assert!(Operation::ListRoles.read_only());
        for operation in [
            Operation::SaveRole,
            Operation::RemoveRole,
            Operation::ApplyRoles,
        ] {
            assert!(!operation.read_only());
            assert_eq!(operation.action(), Some(plugin::Action::WriteRoles));
        }
    }
}
