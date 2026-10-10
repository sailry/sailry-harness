use super::*;

#[tokio::test]
async fn isolates_controller_capabilities() {
    for remote in [false, true] {
        let (_directory, node, controller, client, worktree) = fixture(remote).await;
        for (name, command) in [
            ("code", Command::ReadCodeSettings),
            ("web-search", Command::ReadSearchSettings),
        ] {
            let package = info(execute(&client, Command::ReadPlugin { name: name.into() }).await);
            let package = info(
                execute(
                    &client,
                    Command::SetPluginEnabled {
                        name: name.into(),
                        expected_revision: package.summary.revision,
                        enabled: false,
                    },
                )
                .await,
            );
            let context = Context {
                invocation: None,
                turn: None,
                surface: plugin::desktop::Surface::Settings,
                package: package.summary.reference(),
                worktree: None,
                session: None,
            };
            let output = client
                .execute(client.prepare(command.clone()).with_plugin(context.clone()))
                .await
                .unwrap();
            let save = match output {
                Output::CodeSettings(mut settings) => {
                    settings.after_edit = false;
                    Command::SaveCodeSettings {
                        expected_revision: settings.revision,
                        settings,
                    }
                }
                Output::SearchSettings(mut settings) => {
                    settings.fallback = false;
                    Command::SaveSearchSettings(sailry_protocol::search::Update {
                        settings,
                        credentials: Default::default(),
                    })
                }
                _ => panic!("settings expected"),
            };
            let request = client.prepare(save).with_plugin(context.clone());
            let saved = client.execute(request.clone()).await.unwrap();
            assert_eq!(client.execute(request).await.unwrap(), saved);
            for other in 0..6 {
                let mut denied = context.clone();
                match other {
                    0 => denied.surface = Default::default(),
                    1 => denied.worktree = Some(worktree),
                    2 => denied.turn = Some(sailry_protocol::TurnId::new()),
                    3 => denied.session = Some(sailry_protocol::SessionId::new()),
                    4 => denied.invocation = Some(sailry_protocol::RequestId::new()),
                    _ => {
                        denied.package = info(
                            execute(
                                &client,
                                Command::ReadPlugin {
                                    name: "media".into(),
                                },
                            )
                            .await,
                        )
                        .summary
                        .reference()
                    }
                }
                assert!(
                    client
                        .execute(client.prepare(command.clone()).with_plugin(denied))
                        .await
                        .is_err()
                );
            }
        }
        controller.close().await.unwrap();
        node.shutdown().await.unwrap();
    }
}
