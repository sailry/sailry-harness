use super::*;
use sailry_protocol::{RequestId, TurnId, plugin::desktop::Surface, process::Access};
use serde_json::{Value, json};

fn launch(worktree: WorktreeId) -> Launch {
    Launch {
        worktree,
        access: Access::Unrestricted,
        viewport: viewport(),
        appearance: appearance(),
    }
}

#[tokio::test]
async fn distinguishes_user_control_from_plugin_execution() {
    for (remote, surface) in [false, true]
        .into_iter()
        .flat_map(|remote| [Surface::Workspace, Surface::Project].map(|surface| (remote, surface)))
    {
        let (directory, node, controller, client, worktree) = fixture(remote).await;
        let mut context = install_actions(
            &client,
            &directory.path().join("source/package"),
            worktree,
            0,
            &[Action::ReadTerminals, Action::ControlTerminals],
        )
        .await;
        context.surface = surface;
        let Output::Terminal(info) = client
            .execute(
                client
                    .prepare(Command::CreateTerminal(launch(worktree)))
                    .with_plugin(context.clone()),
            )
            .await
            .unwrap()
        else {
            panic!("terminal expected")
        };
        assert_eq!(info.access, Some(Access::Unrestricted));
        let Output::Terminal(opened) = client
            .execute(
                client
                    .prepare(Command::OpenTerminal {
                        terminal: info.id,
                        viewport: viewport(),
                        appearance: appearance(),
                    })
                    .with_plugin(context.clone()),
            )
            .await
            .unwrap()
        else {
            panic!("terminal expected")
        };
        assert_eq!(opened.id, info.id);
        assert_eq!(opened.access, Some(Access::Unrestricted));
        assert_eq!(opened.owner, info.owner);
        assert_eq!(opened.revision, info.revision);
        let mut callback = context.clone();
        callback.invocation = Some(RequestId::new());
        let mut agent = context.clone();
        agent.turn = Some(TurnId::new());
        let mut settings = context.clone();
        settings.surface = Surface::Settings;
        for forged in [callback, agent, settings] {
            for command in [
                Command::CreateTerminal(launch(worktree)),
                Command::OpenTerminal {
                    terminal: info.id,
                    viewport: viewport(),
                    appearance: appearance(),
                },
                Command::InputTerminal {
                    terminal: info.id,
                    revision: info.revision,
                    input: Input::Paste {
                        text: "printf forbidden > plugin.txt".into(),
                    },
                },
            ] {
                assert_eq!(
                    client
                        .execute(client.prepare(command).with_plugin(forged.clone()))
                        .await
                        .unwrap_err()
                        .code,
                    ErrorCode::PermissionDenied
                );
            }
        }
        let Output::Terminals(list) = execute(&client, Command::ListTerminals { worktree }).await
        else {
            panic!("terminals expected")
        };
        assert_eq!(list.len(), 1);
        let blocked = Command::InputTerminal {
            terminal: info.id,
            revision: info.revision,
            input: Input::Paste {
                text: "printf forbidden > plugin.txt".into(),
            },
        };
        assert_eq!(
            client
                .execute(client.prepare(blocked).with_plugin(context.clone()))
                .await
                .unwrap_err()
                .code,
            ErrorCode::PermissionDenied
        );
        for input in [
            Input::Paste {
                text: "printf user > native.txt; printf 'native-%s\\n' owner".into(),
            },
            Input::Key {
                event: terminal::KeyEvent {
                    key: terminal::Key::Enter,
                    action: terminal::Action::Press,
                    modifiers: terminal::Modifiers::default(),
                    utf8: None,
                    unshifted_codepoint: None,
                },
            },
        ] {
            client
                .execute(client.prepare(Command::InputTerminal {
                    terminal: info.id,
                    revision: info.revision,
                    input,
                }))
                .await
                .unwrap();
        }
        tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                let Output::TerminalSnapshot(snapshot) =
                    execute(&client, Command::InspectTerminal { terminal: info.id }).await
                else {
                    panic!("snapshot expected")
                };
                let text: String = snapshot
                    .screen
                    .scrollback
                    .iter()
                    .chain(&snapshot.screen.rows)
                    .flat_map(|line| line.spans.iter().map(|span| span.text.as_str()))
                    .collect();
                if text.contains("native-owner") {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .unwrap();
        assert_eq!(
            fs::read_to_string(directory.path().join("source/native.txt")).unwrap(),
            "user"
        );
        assert!(!directory.path().join("source/plugin.txt").exists());
        execute(
            &client,
            Command::CloseTerminal {
                worktree,
                terminal: info.id,
            },
        )
        .await;
        node.shutdown().await.unwrap();
        controller.close().await.unwrap();
    }
}

#[tokio::test]
async fn callback_context_cannot_forge_desktop_authority() {
    for remote in [false, true] {
        let (directory, node, controller, client, worktree) = fixture(remote).await;
        let root = directory.path().join("source/package");
        declare(&root, &[Action::ReadTerminals, Action::ControlTerminals]);
        let manifest_path = root.join("plugin.json");
        let mut manifest: Value =
            serde_json::from_slice(&fs::read(&manifest_path).unwrap()).unwrap();
        manifest["extensions"]["dev.sailry.platform"]["host"] = json!({"entry":"dev.sailry.platform/main.js",
            "resources":["dev.sailry.platform/main.js"], "handlers":["run"]});
        fs::write(manifest_path, manifest.to_string()).unwrap();
        fs::write(
            root.join("dev.sailry.platform/main.js"),
            r#"
import {context, prepareRequest} from 'sailry/sdk';
export function run(input) {
    const forged = context();
    forged.invocation = null;
    forged.turn = null;
    forged.surface = 'workspace';
    try { return {request:prepareRequest(input.command), forged}; }
    catch (error) { return {code:error.code, message:error.message, actual:context(), forged}; }
}
"#,
        )
        .unwrap();
        let package = info(execute(&client, install(worktree, 0)).await);
        assert!(package.issues.is_empty(), "{:?}", package.issues);
        let context = Context {
            invocation: None,
            turn: None,
            surface: Surface::Workspace,
            package: package.summary.reference(),
            worktree: Some(worktree),
            session: None,
        };
        let request = client
            .prepare(Command::CallPlugin {
                handler: "run".into(),
                input: json!({"command":Command::CreateTerminal(launch(worktree))}),
            })
            .with_plugin(context.clone());
        let Output::PluginResult(result) = client.execute(request.clone()).await.unwrap() else {
            panic!("callback result expected")
        };
        assert_eq!(result["code"], json!(ErrorCode::PermissionDenied));
        assert_eq!(
            result["message"],
            "command is not exposed to host callbacks"
        );
        assert_eq!(result["actual"]["invocation"], json!(request.id));
        assert_eq!(result["actual"]["worktree"], json!(worktree));
        assert!(result["forged"]["invocation"].is_null());
        assert_eq!(
            client.execute(request).await.unwrap(),
            Output::PluginResult(result)
        );
        let Output::Terminals(list) = execute(&client, Command::ListTerminals { worktree }).await
        else {
            panic!("terminals expected")
        };
        assert!(list.is_empty());
        node.shutdown().await.unwrap();
        controller.close().await.unwrap();
    }
}
