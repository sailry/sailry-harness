//! Operation parameters map directly to the existing public commands.
use super::*;

pub(super) fn description(operation: Operation) -> &'static str {
    match operation {
        Operation::Unsupported => "Unsupported operation",
        Operation::InspectGit
        | Operation::ReadGitDiff
        | Operation::ReadGitLog
        | Operation::ListWorktrees
        | Operation::CreateManagedWorktree
        | Operation::RegisterWorktree
        | Operation::RemoveWorktree => repository::description(operation),
        Operation::BrowseDatabase
        | Operation::QueryDatabase
        | Operation::ExecuteDatabase
        | Operation::RunSsh
        | Operation::TransferSsh => connections::description(operation),
        Operation::ListDirectory
        | Operation::ReadFile
        | Operation::WriteFile
        | Operation::SearchFiles
        | Operation::ReadOffice
        | Operation::ExportPdf => files::description(operation),
        Operation::InspectMedia => "Inspect an image with the model captured by this turn",
        Operation::GenerateImage => "Generate a PNG with the model captured by this turn",
        Operation::GenerateVideo => "Generate an MP4 with the model captured by this turn",
        Operation::ReadComputer | Operation::ControlComputer => {
            "Use native computer resources on the execution Node"
        }
        Operation::ReadBrowser | Operation::ControlBrowser => {
            "Use this session's browser on the initiating controller"
        }
        Operation::ReadExternalBrowser | Operation::ControlExternalBrowser => {
            "Use this session's managed browser on the execution Node"
        }
        Operation::Http => {
            "Send a bounded HTTP request on the execution Node using an optional declared credential"
        }
        Operation::GetValue => "Read this plugin's stored value and revision on the execution Node",
        Operation::ListKeys => "List this plugin's stored keys on the execution Node",
        Operation::SetValue => {
            "Save this plugin's value using its last read revision, or zero for a new key"
        }
        Operation::DeleteValue => "Delete this plugin's stored value using its last read revision",
        Operation::StorageTransaction => {
            "Atomically update this plugin's private values and declared schedules using their last read revisions"
        }
        Operation::ReadSettings => "Read this plugin's public settings without credential values",
        Operation::RunCommand => "Execute a command in the captured turn's worktree on the Node",
        Operation::ReadCommand => "Read or list background commands in the captured session",
        Operation::StopCommand => "Stop a background command in the captured session",
        Operation::Progress | Operation::DelegateAgent => {
            unreachable!("workflow uses its core owner")
        }
    }
}

pub(super) fn parameters(operation: Operation) -> Value {
    if repository::handles(operation) {
        return repository::parameters(operation);
    }
    if connections::handles(operation) {
        return connections::parameters(operation);
    }
    if matches!(
        operation,
        Operation::ListDirectory
            | Operation::ReadFile
            | Operation::WriteFile
            | Operation::SearchFiles
            | Operation::ReadOffice
            | Operation::ExportPdf
    ) {
        return files::parameters(operation);
    }
    let key = json!({"type":"string", "minLength":1, "maxLength":plugin::storage::MAX_KEY_BYTES});
    let revision = json!({"type":"integer", "minimum":0});
    let (properties, required) = match operation {
        Operation::InspectGit
        | Operation::ReadGitDiff
        | Operation::ReadGitLog
        | Operation::ListWorktrees
        | Operation::CreateManagedWorktree
        | Operation::RegisterWorktree
        | Operation::RemoveWorktree => unreachable!("repository parameters handled above"),
        Operation::InspectMedia => (
            json!({"prompt":{"type":"string"},"source":{"type":"object","properties":{"kind":{"enum":["path","attachment"]},"value":{"type":"string"}},"required":["kind","value"],"additionalProperties":false}}),
            vec!["prompt", "source"],
        ),
        Operation::GenerateImage | Operation::GenerateVideo => (
            json!({"prompt":{"type":"string"},"path":{"type":"string"}}),
            vec!["prompt", "path"],
        ),
        Operation::ReadComputer | Operation::ControlComputer => (
            json!({"name":{"type":"string"},"arguments":{"type":"object"}}),
            vec!["name", "arguments"],
        ),
        Operation::ReadBrowser
        | Operation::ControlBrowser
        | Operation::ReadExternalBrowser
        | Operation::ControlExternalBrowser => (
            json!({"action":{"type":"string"},"arguments":{"type":"object"}}),
            vec!["action", "arguments"],
        ),
        Operation::Http => (
            json!({
                "method":{"type":"string", "enum":["GET","HEAD","POST","PUT","PATCH","DELETE","OPTIONS"]},
                "url":{"type":"string", "minLength":1, "maxLength":4096},
                "headers":{"type":"object", "additionalProperties":{"type":"string"}},
                "body":{"type":["string","null"]},
                "credential":{"type":["string","null"]},
                "timeout_ms":{"type":"integer", "minimum":1, "maximum":plugin::http::MAX_TIMEOUT_MS}
            }),
            vec!["method", "url"],
        ),
        Operation::GetValue => (json!({"key":key}), vec!["key"]),
        Operation::ListKeys => (
            json!({"prefix":{"type":"string"}, "after":{"type":["string","null"]}, "limit":{"type":"integer", "minimum":1, "maximum":plugin::storage::MAX_PAGE_LIMIT}}),
            vec!["prefix", "after", "limit"],
        ),
        Operation::SetValue => (
            json!({"key":key, "value":{}, "expected_revision":revision}),
            vec!["key", "value", "expected_revision"],
        ),
        Operation::DeleteValue => (
            json!({"key":key, "expected_revision":revision}),
            vec!["key", "expected_revision"],
        ),
        Operation::StorageTransaction => (
            json!({"operations":{"type":"array","minItems":1,"maxItems":plugin::transaction::MAX_OPERATIONS,"items":{"type":"object"}}}),
            vec!["operations"],
        ),
        Operation::RunCommand => (
            json!({
                "command":{"type":"string"}, "cwd":{"type":"string"},
                "timeout_ms":{"type":"integer", "minimum":1, "maximum":900000},
                "background":{"type":"boolean"},
                "attachments":{"type":"array", "items":{"type":"string"}, "maxItems":8, "uniqueItems":true}
            }),
            vec!["command", "cwd", "timeout_ms", "background", "attachments"],
        ),
        Operation::ReadCommand => (json!({"id":{"type":["string", "null"]}}), vec![]),
        Operation::StopCommand => (json!({"id":{"type":"string"}}), vec!["id"]),
        Operation::ReadSettings => (json!({}), vec![]),
        Operation::Unsupported => return json!({"not": {}}),
        Operation::BrowseDatabase
        | Operation::QueryDatabase
        | Operation::ExecuteDatabase
        | Operation::RunSsh
        | Operation::TransferSsh => unreachable!("connection parameters handled above"),
        Operation::Progress | Operation::DelegateAgent => {
            unreachable!("workflow uses its core owner")
        }
        Operation::ListDirectory
        | Operation::ReadFile
        | Operation::WriteFile
        | Operation::SearchFiles
        | Operation::ReadOffice
        | Operation::ExportPdf => unreachable!("file parameters handled above"),
    };
    json!({"type":"object", "additionalProperties":false, "properties":properties, "required":required})
}

pub(super) fn command(
    operation: Operation,
    arguments: Value,
    context: &plugin::Context,
) -> Result<Command, Fault> {
    match operation {
        Operation::ListDirectory
        | Operation::ReadFile
        | Operation::WriteFile
        | Operation::SearchFiles
        | Operation::ReadOffice
        | Operation::ExportPdf => return files::command(operation, arguments, context),
        Operation::InspectMedia | Operation::GenerateImage | Operation::GenerateVideo => {
            let mut arguments = arguments.as_object().cloned().ok_or_else(|| {
                Fault::new(
                    ErrorCode::InvalidRequest,
                    "media arguments must be an object",
                )
            })?;
            if arguments.contains_key("action") {
                return Err(Fault::new(
                    ErrorCode::InvalidRequest,
                    "media action is fixed by the declared operation",
                ));
            }
            arguments.insert(
                "action".into(),
                json!(match operation {
                    Operation::InspectMedia => "inspect",
                    Operation::GenerateImage => "image",
                    Operation::GenerateVideo => "video",
                    _ => unreachable!(),
                }),
            );
            let action: sailry_protocol::media::Action =
                serde_json::from_value(arguments.into()).map_err(invalid)?;
            crate::agent::media::validate(&action)?;
            return Ok(Command::UseMedia {
                turn: context.turn.ok_or_else(|| {
                    Fault::new(ErrorCode::InvalidRequest, "operation requires a turn")
                })?,
                session: context.session.ok_or_else(|| {
                    Fault::new(ErrorCode::InvalidRequest, "operation requires a session")
                })?,
                worktree: context.worktree.ok_or_else(|| {
                    Fault::new(ErrorCode::InvalidRequest, "operation requires a worktree")
                })?,
                action,
            });
        }
        Operation::ReadComputer | Operation::ControlComputer => {
            #[derive(serde::Deserialize)]
            #[serde(deny_unknown_fields)]
            struct Input {
                name: String,
                arguments: Value,
            }
            let input: Input = serde_json::from_value(arguments).map_err(invalid)?;
            crate::computer::execute::validate(&input.name, &input.arguments)?;
            if crate::computer::read_only(&input.name).expect("validated computer tool")
                != operation.read_only()
            {
                return Err(Fault::new(
                    ErrorCode::PermissionDenied,
                    "computer tool does not match the declared operation",
                ));
            }
            return Ok(Command::UseComputer {
                session: context.session.ok_or_else(|| {
                    Fault::new(ErrorCode::InvalidRequest, "operation requires a session")
                })?,
                worktree: context.worktree.ok_or_else(|| {
                    Fault::new(ErrorCode::InvalidRequest, "operation requires a worktree")
                })?,
                name: input.name,
                arguments: input.arguments,
            });
        }
        Operation::ReadBrowser | Operation::ControlBrowser => {
            #[derive(serde::Deserialize)]
            #[serde(deny_unknown_fields)]
            struct Browser {
                action: String,
                arguments: serde_json::Map<String, Value>,
            }
            let mut browser: Browser = serde_json::from_value(arguments).map_err(invalid)?;
            browser
                .arguments
                .insert("action".into(), browser.action.into());
            let action: sailry_protocol::browser::Action =
                serde_json::from_value(browser.arguments.into()).map_err(invalid)?;
            if action.requires_approval() == operation.read_only() {
                return Err(Fault::new(
                    ErrorCode::PermissionDenied,
                    "browser action does not match the declared operation",
                ));
            }
            crate::browser::validate(&action)?;
            return Ok(Command::UseBrowser {
                session: context.session.ok_or_else(|| {
                    Fault::new(ErrorCode::InvalidRequest, "operation requires a session")
                })?,
                worktree: context.worktree.ok_or_else(|| {
                    Fault::new(ErrorCode::InvalidRequest, "operation requires a worktree")
                })?,
                action,
            });
        }
        Operation::ReadExternalBrowser | Operation::ControlExternalBrowser => {
            #[derive(serde::Deserialize)]
            #[serde(deny_unknown_fields)]
            struct Browser {
                action: sailry_protocol::external_browser::Action,
                arguments: Value,
            }
            let browser: Browser = serde_json::from_value(arguments).map_err(invalid)?;
            if browser.action.read_only() != operation.read_only() {
                return Err(Fault::new(
                    ErrorCode::PermissionDenied,
                    "browser action does not match the declared operation",
                ));
            }
            crate::external_browser::validate(browser.action, &browser.arguments)?;
            return Ok(Command::UseExternalBrowser {
                session: context.session.ok_or_else(|| {
                    Fault::new(ErrorCode::InvalidRequest, "operation requires a session")
                })?,
                worktree: context.worktree.ok_or_else(|| {
                    Fault::new(ErrorCode::InvalidRequest, "operation requires a worktree")
                })?,
                action: browser.action,
                arguments: browser.arguments,
            });
        }
        Operation::RunCommand => {
            #[derive(serde::Deserialize)]
            #[serde(deny_unknown_fields)]
            struct Launch {
                command: String,
                cwd: String,
                timeout_ms: u64,
                background: bool,
                attachments: Vec<sailry_protocol::AttachmentId>,
            }
            let launch: Launch = serde_json::from_value(arguments).map_err(invalid)?;
            return Ok(Command::RunCommand {
                turn: context.turn.ok_or_else(|| {
                    Fault::new(ErrorCode::InvalidRequest, "operation requires a turn")
                })?,
                command: launch.command,
                cwd: launch.cwd,
                timeout_ms: launch.timeout_ms,
                background: launch.background,
                attachments: launch.attachments,
            });
        }
        Operation::ReadCommand | Operation::StopCommand => {
            #[derive(serde::Deserialize)]
            #[serde(deny_unknown_fields)]
            struct Identity {
                id: Option<sailry_protocol::RequestId>,
            }
            let identity: Identity = serde_json::from_value(arguments).map_err(invalid)?;
            let session = context.session.ok_or_else(|| {
                Fault::new(ErrorCode::InvalidRequest, "operation requires a session")
            })?;
            return match (operation, identity.id) {
                (Operation::StopCommand, Some(id)) => Ok(Command::StopCommand { session, id }),
                (Operation::ReadCommand, Some(id)) => Ok(Command::ReadCommand { session, id }),
                (Operation::ReadCommand, None) => Ok(Command::ListCommands { session }),
                _ => Err(Fault::new(
                    ErrorCode::InvalidRequest,
                    "command id is required",
                )),
            };
        }
        _ => {}
    }
    let kind = match operation {
        Operation::InspectGit
        | Operation::ReadGitDiff
        | Operation::ReadGitLog
        | Operation::ListWorktrees
        | Operation::CreateManagedWorktree
        | Operation::RegisterWorktree
        | Operation::RemoveWorktree => {
            unreachable!("repository command requires the captured project")
        }
        Operation::BrowseDatabase
        | Operation::QueryDatabase
        | Operation::ExecuteDatabase
        | Operation::RunSsh
        | Operation::TransferSsh => {
            unreachable!("connection command requires the captured catalog")
        }
        Operation::Http => "request_plugin_http",
        Operation::GetValue => "read_plugin_value",
        Operation::StorageTransaction => {
            #[derive(serde::Deserialize)]
            #[serde(deny_unknown_fields)]
            struct Transaction {
                operations: Value,
            }
            let transaction: Transaction = serde_json::from_value(arguments).map_err(invalid)?;
            let operations = plugin::transaction::sdk::decode(transaction.operations)?;
            if operations.iter().any(|operation| {
                !matches!(
                    operation,
                    plugin::transaction::Operation::Write { .. }
                        | plugin::transaction::Operation::Index { .. }
                        | plugin::transaction::Operation::Remove { .. }
                        | plugin::transaction::Operation::Dispatch(_)
                )
            }) {
                return Err(Fault::new(
                    ErrorCode::PermissionDenied,
                    "storage tools can only mutate private values and declared schedules",
                ));
            }
            return Ok(Command::PluginTransaction { operations });
        }
        Operation::ListKeys => "list_plugin_keys",
        Operation::SetValue => "write_plugin_value",
        Operation::DeleteValue => "remove_plugin_value",
        Operation::ReadSettings => {
            return Ok(Command::ReadPluginSettings {
                package: context.package.clone(),
            });
        }
        Operation::Unsupported => {
            return Err(Fault::new(
                ErrorCode::Unavailable,
                "the declared operation is not supported",
            ));
        }
        Operation::Progress | Operation::DelegateAgent => {
            unreachable!("workflow uses its core owner")
        }
        Operation::ListDirectory
        | Operation::ReadFile
        | Operation::WriteFile
        | Operation::SearchFiles
        | Operation::ReadOffice
        | Operation::ExportPdf
        | Operation::InspectMedia
        | Operation::GenerateImage
        | Operation::GenerateVideo
        | Operation::ReadComputer
        | Operation::ControlComputer
        | Operation::RunCommand
        | Operation::ReadCommand
        | Operation::StopCommand
        | Operation::ReadExternalBrowser
        | Operation::ControlExternalBrowser
        | Operation::ReadBrowser
        | Operation::ControlBrowser => unreachable!(),
    };
    serde_json::from_value(json!({"kind":kind, "data":arguments})).map_err(invalid)
}

fn invalid(error: serde_json::Error) -> Fault {
    Fault::new(
        ErrorCode::InvalidRequest,
        format!("invalid plugin operation arguments: {error}"),
    )
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
                name: "process-example".into(),
                digest: "a".repeat(64),
                settings_revision: 0,
            },
            worktree: Some(sailry_protocol::WorktreeId::new()),
            session: Some(sailry_protocol::SessionId::new()),
        }
    }

    #[test]
    fn storage_transactions_keep_declared_private_operations() {
        let scope = scope();
        let arguments = json!({"operations":[{"kind":"index","data":{"key":"item","value":{},"index":{"fields":["title","body"],"tags":["opaque"],"order":1},"expected_revision":"9007199254740993"}}]});
        assert!(
            matches!(command(Operation::StorageTransaction,arguments.clone(),&scope).unwrap(),Command::PluginTransaction { operations } if matches!(operations[0],plugin::transaction::Operation::Index { expected_revision:9007199254740993,.. }))
        );
        for field in ["package", "namespace", "worktree", "session"] {
            let mut replaced = arguments.clone();
            replaced[field] = json!("other");
            assert_eq!(
                command(Operation::StorageTransaction, replaced, &scope)
                    .unwrap_err()
                    .code,
                ErrorCode::InvalidRequest
            );
        }
        assert_eq!(
            command(
                Operation::StorageTransaction,
                json!({"operations":[{"kind":"read","data":{"key":"item"}}]}),
                &scope
            )
            .unwrap_err()
            .code,
            ErrorCode::InvalidRequest
        );
        assert!(!Operation::StorageTransaction.read_only());
        assert_eq!(
            Operation::StorageTransaction.action(),
            Some(plugin::Action::WriteStorage)
        );
        assert_eq!(
            command(
                Operation::StorageTransaction,
                json!({"operations":[{"kind":"notify","data":sailry_protocol::notification::Draft {
                    title: "Notice".into(),
                    message: "Sent".into(),
                    kind: sailry_protocol::notification::Kind::Info,
                    session: None,
                }}]}),
                &scope,
            )
            .unwrap_err()
            .code,
            ErrorCode::PermissionDenied
        );
        let operation = plugin::transaction::Operation::Dispatch(
            sailry_protocol::dispatch::Command::RemoveHandler {
                name: "jobs".into(),
                expected_revision: 0,
                cancel_pending: true,
            },
        );
        assert!(matches!(command(
            Operation::StorageTransaction, json!({"operations":[operation]}), &scope,
        ).unwrap(), Command::PluginTransaction { operations } if operations == [operation]));
    }

    #[test]
    fn media_arguments_keep_captured_models_and_ownership() {
        let scope = scope();
        for (operation, arguments, kind) in [
            (
                Operation::InspectMedia,
                json!({"prompt":"Describe","source":{"kind":"path","value":"input.png"}}),
                sailry_protocol::media::Kind::Vision,
            ),
            (
                Operation::GenerateImage,
                json!({"prompt":"Create","path":"generated/output.png"}),
                sailry_protocol::media::Kind::Image,
            ),
            (
                Operation::GenerateVideo,
                json!({"prompt":"Create","path":"generated/output.mp4"}),
                sailry_protocol::media::Kind::Video,
            ),
        ] {
            let Command::UseMedia {
                turn,
                session,
                worktree,
                action,
            } = command(operation, arguments.clone(), &scope).unwrap()
            else {
                panic!("media command expected")
            };
            assert_eq!(Some(turn), scope.turn);
            assert_eq!(Some(session), scope.session);
            assert_eq!(Some(worktree), scope.worktree);
            assert_eq!(action.kind(), kind);
            for field in ["turn", "session", "worktree", "model", "provider", "action"] {
                let mut replaced = arguments.clone();
                replaced[field] = json!("replacement");
                assert_eq!(
                    command(operation, replaced, &scope).unwrap_err().code,
                    ErrorCode::InvalidRequest
                );
            }
        }
        for path in ["../outside.png", "/outside.png", ".git/config"] {
            assert!(
                command(
                    Operation::GenerateImage,
                    json!({"prompt":"Create","path":path}),
                    &scope
                )
                .is_err()
            );
        }
        let mut missing = scope;
        missing.turn = None;
        assert_eq!(
            command(
                Operation::GenerateImage,
                json!({"prompt":"Create","path":"image.png"}),
                &missing
            )
            .unwrap_err()
            .code,
            ErrorCode::InvalidRequest
        );
    }

    #[test]
    fn process_arguments_cannot_replace_captured_ownership() {
        let scope = scope();
        let arguments = json!({"command":"printf result", "cwd":"", "timeout_ms":1000, "background":false, "attachments":[]});
        assert!(
            matches!(command(Operation::RunCommand, arguments.clone(), &scope).unwrap(), Command::RunCommand {turn,..} if Some(turn) == scope.turn)
        );
        for field in ["turn", "session", "worktree"] {
            let mut replaced = arguments.clone();
            replaced[field] = json!(TurnId::new());
            assert!(command(Operation::RunCommand, replaced, &scope).is_err());
        }
        for operation in [Operation::ReadCommand, Operation::StopCommand] {
            assert!(command(operation, json!({"id":sailry_protocol::RequestId::new(),"session":sailry_protocol::SessionId::new()}), &scope).is_err());
        }
        assert!(command(Operation::StopCommand, json!({}), &scope).is_err());
    }

    #[test]
    fn browser_arguments_keep_captured_scope_and_grant() {
        let scope = scope();
        for (operation, action) in [
            (Operation::ReadExternalBrowser, "navigate"),
            (Operation::ControlExternalBrowser, "evaluate_js"),
        ] {
            let arguments = json!({"action":action,"arguments":{}});
            assert!(
                matches!(command(operation, arguments.clone(), &scope).unwrap(), Command::UseExternalBrowser {session,worktree,..} if Some(session) == scope.session && Some(worktree) == scope.worktree)
            );
            for field in ["turn", "session", "worktree"] {
                let mut replaced = arguments.clone();
                replaced[field] = json!(TurnId::new());
                assert_eq!(
                    command(operation, replaced, &scope).unwrap_err().code,
                    ErrorCode::InvalidRequest
                );
            }
            let opposite = if operation.read_only() {
                Operation::ControlExternalBrowser
            } else {
                Operation::ReadExternalBrowser
            };
            assert_eq!(
                command(opposite, arguments, &scope).unwrap_err().code,
                ErrorCode::PermissionDenied
            );
        }
        let mut unscoped = scope.clone();
        unscoped.session = None;
        assert_eq!(
            command(
                Operation::ControlExternalBrowser,
                json!({"action":"close_session","arguments":{}}),
                &unscoped
            )
            .unwrap_err()
            .code,
            ErrorCode::InvalidRequest
        );
    }
    #[test]
    fn computer_keeps_frozen_ownership() {
        let scope = scope();
        let arguments = json!({"name":"list_apps","arguments":{}});
        assert!(
            matches!(command(Operation::ReadComputer,arguments.clone(),&scope).unwrap(),Command::UseComputer {session,worktree,..} if Some(session)==scope.session && Some(worktree)==scope.worktree)
        );
        assert_eq!(
            command(Operation::ControlComputer, arguments.clone(), &scope)
                .unwrap_err()
                .code,
            ErrorCode::PermissionDenied
        );
        for field in ["session", "worktree", "turn"] {
            let mut replaced = arguments.clone();
            replaced[field] = json!(TurnId::new());
            assert_eq!(
                command(Operation::ReadComputer, replaced, &scope)
                    .unwrap_err()
                    .code,
                ErrorCode::InvalidRequest
            );
        }
    }

    #[test]
    fn computer_preserves_native_arguments() {
        let scope = scope();
        let native = json!({"nested":[{"read_only":false,"name":"unchanged"}]});
        let Command::UseComputer {
            name, arguments, ..
        } = command(
            Operation::ReadComputer,
            json!({"name":"list_apps","arguments":native}),
            &scope,
        )
        .unwrap()
        else {
            unreachable!()
        };
        assert_eq!(name, "list_apps");
        assert_eq!(arguments, native);
        for arguments in [
            json!({"name":"unknown_tool","arguments":{}}),
            json!({"name":"list_apps","arguments":[]}),
        ] {
            assert_eq!(
                command(Operation::ReadComputer, arguments, &scope)
                    .unwrap_err()
                    .code,
                ErrorCode::InvalidRequest,
            );
        }
        assert_eq!(
            command(
                Operation::ReadComputer,
                json!({"name":"click","arguments":{"read_only":true}}),
                &scope,
            )
            .unwrap_err()
            .code,
            ErrorCode::PermissionDenied,
        );
    }
}
