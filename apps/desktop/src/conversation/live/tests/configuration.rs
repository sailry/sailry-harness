use super::*;
mod accounts;
mod remembered;

#[gpui::test]
fn remote_defaults_and_resume(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    cx.update(|cx| {
        gpui_kit::init(cx);
        crate::theme::init(cx);
    });
    for remote in [false, true] {
        let directory = tempfile::tempdir().unwrap();
        let runtime = Arc::new(tokio::runtime::Runtime::new().unwrap());
        let source = runtime
            .block_on(Node::start(directory.path().join("source")))
            .unwrap();
        let target = runtime
            .block_on(Node::start(directory.path().join("target")))
            .unwrap();
        runtime
            .block_on(source.link().pair(target.link().invite().unwrap().ticket()))
            .unwrap();
        let controller = runtime
            .block_on(Link::controller(
                directory.path().join("controller"),
                NetworkScope::default(),
            ))
            .unwrap();
        let address = runtime
            .block_on(
                controller
                    .handle()
                    .pair(target.link().invite().unwrap().ticket()),
            )
            .unwrap();
        let client = Arc::new(Client::new(if remote {
            controller.handle().remote(address)
        } else {
            target.local()
        }));
        let origin = Arc::new(Client::new(source.local()));
        let source_server = runtime.block_on(support::Server::start(false));
        let target_server = runtime.block_on(support::Server::start(false));
        let selected = provider(&source_server.endpoint, "source-model");
        let mut other = provider(&target_server.endpoint, "target-model");
        other.models[0].efforts = vec![Effort::Default];
        other.models[0].default_effort = Effort::Default;
        for (client, provider) in [(&origin, &selected), (&client, &other)] {
            runtime
                .block_on(client.execute(client.prepare(Command::SaveProvider {
                    provider: provider.clone(),
                    expected_revision: 0,
                    secret: None,
                })))
                .unwrap();
        }
        let Output::Project(project) = runtime
            .block_on(client.execute(client.prepare(Command::RegisterProject {
                name: "Configuration fixture".into(),
                path: directory.path().to_str().unwrap().into(),
            })))
            .unwrap()
        else {
            panic!("project expected")
        };
        let Output::Snapshot(snapshot) = runtime
            .block_on(client.execute(client.prepare(Command::Snapshot)))
            .unwrap()
        else {
            panic!("snapshot expected")
        };
        let binding = Binding {
            client: client.clone(),
            defaults: origin.clone(),
            runtime: runtime.clone(),
            project: Some(project.id),
            worktree: Some(snapshot.worktrees[0].id),
            host: "Target fixture".into(),
            project_name: "Configuration fixture".into(),
            branch: "main".into(),
        };
        let mut entity = None;
        let (_, visual) = cx.add_window_view(|window, cx| {
            let view = cx.new(|cx| View::new(binding.clone(), None, window, cx));
            entity = Some(view.clone());
            Root::new(cx.new(|_| fixture::Harness(view)), window, cx)
        });
        let view = entity.unwrap();
        wait(visual, |cx| {
            view.read(cx).configured()
                && view.read(cx).connected()
                && view.read(cx).defaults.connected
        });
        assert_eq!(
            view.read_with(visual, |view, _| view.config.as_ref().unwrap().provider),
            other.id
        );
        visual.update(|window, cx| {
            view.update(cx, |view, cx| {
                let choice = crate::conversation::models::Selection {
                    channel: view.provider_ids[&(source.id(), selected.id)],
                    model: selected.default_model.clone(),
                };
                view.select_model(&choice, Effort::High, window, cx);
            })
        });
        assert_eq!(
            view.read_with(visual, |view, _| view.config.as_ref().unwrap().provider),
            selected.id
        );
        assert!(visual.debug_bounds("live-chat-effort").is_some());
        assert_eq!(
            view.read_with(visual, |view, _| view.available_efforts()),
            vec![Effort::Low, Effort::High]
        );
        let mut custom = selected.clone();
        custom.models[0].efforts = vec![Effort::Default];
        custom.models[0].default_effort = Effort::Default;
        custom.models[0].custom_efforts = true;
        runtime
            .block_on(origin.execute(origin.prepare(Command::PutProvider {
                provider: custom.clone(),
                expected_revision: 1,
            })))
            .unwrap();
        wait(visual, |cx| {
            view.read(cx).config.as_ref().unwrap().effort == Effort::Default
        });
        assert!(
            view.read_with(visual, |view, _| view.available_efforts())
                .is_empty()
        );
        assert!(visual.debug_bounds("live-chat-effort").is_none());
        custom.models[0].efforts = vec![Effort::Medium, Effort::High];
        custom.models[0].default_effort = Effort::Medium;
        runtime
            .block_on(origin.execute(origin.prepare(Command::PutProvider {
                provider: custom,
                expected_revision: 2,
            })))
            .unwrap();
        wait(visual, |cx| {
            view.read(cx).config.as_ref().unwrap().effort == Effort::Medium
        });
        assert_eq!(
            view.read_with(visual, |view, _| view.available_efforts()),
            vec![Effort::Medium, Effort::High]
        );
        assert!(visual.debug_bounds("live-chat-effort").is_some());
        open_models(visual);
        assert!(visual.debug_bounds("live-chat-config-source").is_none());
        let source_option = view.read_with(visual, |view, _| {
            match view.provider_ids[&(source.id(), selected.id)] {
                0 => "composer-model-option-0-source-model",
                1 => "composer-model-option-1-source-model",
                _ => panic!("unexpected source channel"),
            }
        });
        assert!(visual.debug_bounds(source_option).is_some());
        visual.simulate_keystrokes("escape");
        click(visual, "live-chat-mode");
        click(visual, "composer_mode_plan-option");
        click(visual, "live-chat-permission");
        click(visual, "composer_permission_project-option");
        click(visual, "live-chat-input");
        visual.simulate_input("use desktop configuration");
        visual.simulate_keystrokes("enter");
        wait(visual, |cx| {
            view.read(cx)
                .history
                .snapshot
                .as_ref()
                .is_some_and(|snapshot| {
                    snapshot
                        .page
                        .runs
                        .iter()
                        .any(|run| run.status == Status::Completed)
                })
        });
        let session = view.read_with(visual, |view, cx| {
            assert_eq!(view.error, None);
            assert!(view.input.read(cx).value().is_empty());
            assert!(view._defaults.is_none());
            let session = view.session.clone().unwrap();
            assert_eq!(session.config.mode, sailry_protocol::WorkMode::Plan);
            assert_eq!(
                session.config.permission,
                sailry_protocol::Permission::Project
            );
            assert_eq!(session.profile.as_ref().unwrap().source, source.id());
            session
        });
        assert_eq!(source_server.requests.lock().unwrap().len(), 1);
        assert!(target_server.requests.lock().unwrap().is_empty());
        runtime
            .block_on(client.execute(client.prepare(Command::SetDefaults {
                expected_revision: 0,
                config: SessionConfig {
                    assistant: None,
                    resource: None,
                    provider: other.id,
                    model: other.default_model.clone(),
                    effort: Effort::Default,
                    mode: sailry_protocol::WorkMode::Code,
                    permission: sailry_protocol::Permission::Ask,
                    credential: None,
                },
            })))
            .unwrap();
        visual.update(|window, _| window.remove_window());
        drop(view);
        runtime.block_on(source.shutdown()).unwrap();
        let mut entity = None;
        let (_, visual) = cx.add_window_view(|window, cx| {
            let view = cx.new(|cx| View::new(binding, Some(session.clone()), window, cx));
            entity = Some(view.clone());
            Root::new(cx.new(|_| fixture::Harness(view)), window, cx)
        });
        let view = entity.unwrap();
        wait(visual, |cx| {
            view.read(cx).configured() && view.read(cx).connected()
        });
        assert_eq!(
            view.read_with(visual, |view, _| view.config.clone().unwrap()),
            session.config
        );
        open_models(visual);
        assert!(visual.debug_bounds("live-chat-config-source").is_none());
        assert!(
            visual
                .debug_bounds("composer-model-option-0-source-model")
                .is_some()
        );
        assert!(
            visual
                .debug_bounds("composer-model-option-1-target-model")
                .is_some()
        );
        visual.simulate_keystrokes("escape");
        click(visual, "live-chat-input");
        visual.simulate_input("source is offline");
        visual.simulate_keystrokes("enter");
        wait(visual, |cx| {
            view.read(cx)
                .history
                .snapshot
                .as_ref()
                .is_some_and(|snapshot| {
                    snapshot
                        .page
                        .runs
                        .iter()
                        .filter(|run| run.status == Status::Completed)
                        .count()
                        == 2
                })
        });
        assert_eq!(source_server.requests.lock().unwrap().len(), 2);
        assert!(target_server.requests.lock().unwrap().is_empty());
        open_models(visual);
        click(visual, "composer-model-option-1-target-model");
        wait(visual, |cx| {
            view.read(cx)
                .session
                .as_ref()
                .is_some_and(|session| session.revision == 2 && session.profile.is_none())
        });
        assert!(visual.debug_bounds("live-chat-effort").is_none());
        assert!(
            view.read_with(visual, |view, _| view.available_efforts())
                .is_empty()
        );
        assert_eq!(
            view.read_with(visual, |view, _| view.config.as_ref().unwrap().effort),
            Effort::Default
        );
        assert_eq!(
            view.read_with(visual, |view, _| view.config.as_ref().unwrap().permission),
            sailry_protocol::Permission::Project
        );
        assert_eq!(
            view.read_with(visual, |view, _| view.config.as_ref().unwrap().mode),
            sailry_protocol::WorkMode::Plan
        );
        other.models[0].efforts = vec![Effort::Low, Effort::High];
        other.models[0].default_effort = Effort::Low;
        other.models[0].custom_efforts = true;
        runtime
            .block_on(client.execute(client.prepare(Command::PutProvider {
                provider: other.clone(),
                expected_revision: 1,
            })))
            .unwrap();
        wait(visual, |cx| {
            view.read(cx).available_efforts() == [Effort::Low, Effort::High]
        });
        assert_eq!(
            view.read_with(visual, |view, _| view.config.as_ref().unwrap().effort),
            Effort::Default
        );
        visual.update(|window, cx| {
            view.update(cx, |view, cx| view.select_effort(Effort::High, window, cx))
        });
        wait(visual, |cx| {
            view.read(cx).session.as_ref().unwrap().config.effort == Effort::High
        });
        other.models[0].efforts = vec![Effort::Default];
        other.models[0].default_effort = Effort::Default;
        runtime
            .block_on(client.execute(client.prepare(Command::PutProvider {
                provider: other,
                expected_revision: 2,
            })))
            .unwrap();
        wait(visual, |cx| view.read(cx).available_efforts().is_empty());
        assert!(visual.debug_bounds("live-chat-effort").is_none());
        assert_eq!(
            view.read_with(visual, |view, _| view.config.as_ref().unwrap().effort),
            Effort::High
        );
        visual.update(|window, _| window.remove_window());
        runtime.block_on(target.shutdown()).unwrap();
        runtime.block_on(controller.close()).unwrap();
    }
}
