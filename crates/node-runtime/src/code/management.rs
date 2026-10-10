//! Conversation lifecycle tools reuse the controller's exact installation descriptors.
use sailry_protocol::{Command, Fault, code, plugin, tool::Operation};
use serde_json::{Value, json};

pub(crate) fn handles(operation: Operation) -> bool {
    matches!(
        operation,
        Operation::CodePackages | Operation::InstallCodeServer | Operation::UninstallCodeServer
    )
}

pub(crate) fn description(operation: Operation) -> &'static str {
    match operation {
        Operation::CodePackages => {
            "List language packages for this project, or managed installations on the execution Node"
        }
        Operation::InstallCodeServer => {
            "Install or update an exact language package on the execution Node using its returned install descriptor"
        }
        Operation::UninstallCodeServer => {
            "Uninstall an exact managed language package version and stop its running instances across projects"
        }
        _ => unreachable!("language management operation expected"),
    }
}

pub(crate) fn parameters(operation: Operation) -> Value {
    let string = json!({"type":"string","minLength":1});
    let (properties, required) = match operation {
        Operation::CodePackages => (
            json!({"installed":{"type":"boolean","description":"True lists managed installations and actual versions; false lists project candidates and installation descriptors"}}),
            vec!["installed"],
        ),
        Operation::InstallCodeServer => (
            json!({
                "package":string,"version":string,"purl":string,"revision":string,
                "urls":{"type":"array","items":string},
                "licenses":{"type":"array","items":{"type":"string"}},
                "command":{"type":["array","null"],"items":{"type":"string"}}
            }),
            vec![
                "package", "version", "purl", "revision", "urls", "licenses", "command",
            ],
        ),
        Operation::UninstallCodeServer => (
            json!({"package":string,"version":string}),
            vec!["package", "version"],
        ),
        _ => unreachable!("language management operation expected"),
    };
    json!({"type":"object","properties":properties,"required":required,"additionalProperties":false})
}

pub(crate) fn command(
    operation: Operation,
    arguments: Value,
    context: &plugin::Context,
) -> Result<Command, Fault> {
    let invalid = |error| Fault::invalid(format!("invalid language management arguments: {error}"));
    match operation {
        Operation::CodePackages => {
            #[derive(serde::Deserialize)]
            #[serde(deny_unknown_fields)]
            struct Input {
                installed: bool,
            }
            let input: Input = serde_json::from_value(arguments).map_err(invalid)?;
            Ok(if input.installed {
                Command::ListCodeInstallations
            } else {
                Command::ListCodePackages {
                    worktree: context
                        .worktree
                        .ok_or_else(|| Fault::invalid("language discovery requires a worktree"))?,
                }
            })
        }
        Operation::InstallCodeServer => {
            let install: code::Install = serde_json::from_value(arguments).map_err(invalid)?;
            Ok(Command::InstallCodeServer { install })
        }
        Operation::UninstallCodeServer => {
            #[derive(serde::Deserialize)]
            #[serde(deny_unknown_fields)]
            struct Input {
                package: String,
                version: String,
            }
            let input: Input = serde_json::from_value(arguments).map_err(invalid)?;
            Ok(Command::UninstallCodeServer {
                package: input.package,
                version: input.version,
            })
        }
        _ => unreachable!("language management operation expected"),
    }
}
