use super::*;
use sailry_protocol::{code, tool::Operation};

fn fixture(operation: Operation, mode: WorkMode) -> (Fixture, Binding, Command) {
    let mut fixture = Fixture::with_catalog(Permission::Ask, mode, true);
    let owner = super::super::super::plugins::read(&fixture.database.connection, fixture.turn)
        .unwrap()
        .into_iter()
        .find(|package| package.name == "code")
        .unwrap();
    let command = match operation {
        Operation::InstallCodeServer => Command::InstallCodeServer {
            install: code::Install {
                package: "rust-analyzer".into(),
                version: "1.0".into(),
                purl: "pkg:github/example/server@1.0".into(),
                urls: vec!["https://example.com/server".into()],
                licenses: vec!["MIT".into()],
                command: None,
                revision: "a".repeat(64),
            },
        },
        Operation::UninstallCodeServer => Command::UninstallCodeServer {
            package: "rust-analyzer".into(),
            version: "1.0".into(),
        },
        Operation::StartCodeServer | Operation::StopCodeServer => {
            let run =
                super::super::super::calls::active(&fixture.database.connection, fixture.turn)
                    .unwrap();
            let startup = code::Startup {
                worktree: run.worktree,
                root: String::new(),
                server: "rust-analyzer".into(),
                executable: "/fixture/rust-analyzer".into(),
                revision: "b".repeat(64),
            };
            if operation == Operation::StartCodeServer {
                Command::StartCodeServer { startup }
            } else {
                Command::StopCodeServer { startup }
            }
        }
        _ => unreachable!("lifecycle operation expected"),
    };
    let (name, args) = match &command {
        Command::InstallCodeServer { install } => {
            ("code_install", serde_json::to_value(install).unwrap())
        }
        Command::UninstallCodeServer { package, version } => (
            "code_uninstall",
            json!({"package":package,"version":version}),
        ),
        Command::StartCodeServer { startup } => {
            ("code_start", serde_json::to_value(startup).unwrap())
        }
        Command::StopCodeServer { startup } => {
            ("code_stop", serde_json::to_value(startup).unwrap())
        }
        _ => unreachable!(),
    };
    fixture.request = ToolConfirmationRequest {
        acting: None,
        origin: None,
        tool_name: crate::plugins::tools::alias("code", name),
        function_call_id: Some("management-call".into()),
        args,
    };
    let mut event = AdkEvent::new("code");
    event.author = "assistant".into();
    event.set_content(adk_core::Content {
        role: "model".into(),
        parts: vec![adk_core::Part::FunctionCall {
            id: fixture.request.function_call_id.clone(),
            name: fixture.request.tool_name.clone(),
            args: fixture.request.args.clone(),
            thought_signature: None,
        }],
    });
    history::append(
        &mut fixture.database,
        fixture.session,
        fixture.turn,
        event,
        &fixture.events,
    )
    .unwrap();
    let binding = Binding {
        owner,
        tool: fixture.request.tool_name.clone(),
        operation: Some(operation),
    };
    (fixture, binding, command)
}

#[test]
fn requires_exact_claimed_lifecycle_call() {
    for operation in [
        Operation::InstallCodeServer,
        Operation::UninstallCodeServer,
        Operation::StartCodeServer,
        Operation::StopCodeServer,
    ] {
        let (mut fixture, binding, command) = fixture(operation, WorkMode::Code);
        let run =
            super::super::super::calls::active(&fixture.database.connection, fixture.turn).unwrap();
        let context = plugin::Context {
            invocation: None,
            turn: Some(fixture.turn),
            surface: plugin::desktop::Surface::Workspace,
            package: binding.owner.clone(),
            worktree: Some(run.worktree),
            session: Some(fixture.session),
        };
        let mut request = Request::new(fixture.database.node, command.clone()).with_plugin(context);
        assert_eq!(
            crate::store::code::prepare(&fixture.database, &request)
                .unwrap_err()
                .code,
            ErrorCode::PermissionDenied
        );
        let approval = begin(
            &mut fixture.database,
            fixture.turn,
            &fixture.request,
            Some(&binding),
            &fixture.events,
        )
        .unwrap();
        fixture.approve(approval.id);
        request.id = authorize(
            &mut fixture.database.connection,
            fixture.turn,
            &fixture.request,
            Some(&command),
        )
        .unwrap();
        assert!(crate::store::code::prepare(&fixture.database, &request).is_ok());
        let mut forged = request.clone();
        match &mut forged.command {
            Command::InstallCodeServer { install } => {
                install.urls.push("https://example.com/changed".into())
            }
            Command::UninstallCodeServer { version, .. } => *version = "2.0".into(),
            Command::StartCodeServer { startup } | Command::StopCodeServer { startup } => {
                startup.executable = "/bin/sh".into();
            }
            _ => unreachable!(),
        }
        assert_eq!(
            crate::store::code::prepare(&fixture.database, &forged)
                .unwrap_err()
                .code,
            ErrorCode::PermissionDenied
        );
        request.plugin.as_mut().unwrap().session = Some(SessionId::new());
        assert_eq!(
            crate::store::code::prepare(&fixture.database, &request)
                .unwrap_err()
                .code,
            ErrorCode::PermissionDenied
        );
    }
}

#[test]
fn planning_rejects_lifecycle_changes() {
    for operation in [
        Operation::InstallCodeServer,
        Operation::UninstallCodeServer,
        Operation::StartCodeServer,
        Operation::StopCodeServer,
    ] {
        let (mut fixture, binding, _) = fixture(operation, WorkMode::Plan);
        assert_eq!(
            begin(
                &mut fixture.database,
                fixture.turn,
                &fixture.request,
                Some(&binding),
                &fixture.events
            )
            .unwrap_err()
            .code,
            ErrorCode::PermissionDenied
        );
    }
}
