use super::*;

#[gpui::test]
fn preserves_external_messages(cx: &mut TestAppContext) {
    fixture::init(cx);
    for remote in [false, true] {
        let name = crate::agent_fixture::plugin_tool("commands", "run_command");
        let command = "printf '完整内容 🙂\\n```rust\\n';\nprintf 'diagnostic\\377' >&2; exit 7";
        let fixture = fixture::Fixture::with_tools(
            remote,
            vec![
                (name.clone(), json!({"command":command, "cwd":"nested"})),
                (
                    name.clone(),
                    json!({"command":"printf before; sleep 30", "timeout_ms":100}),
                ),
                (name, json!({"command":"exit 0"})),
            ],
        );
        std::fs::create_dir(fixture.directory.path().join("project/nested")).unwrap();
        let Output::Plugin(package) = fixture.execute(Command::ReadPlugin {
            name: "commands".into(),
        }) else {
            panic!();
        };
        assert!(package.issues.is_empty());
        let (view, visual) = fixture::open(cx, fixture.binding.clone(), fixture.session.clone());
        fixture.start();
        for index in 0..3 {
            wait(visual, |cx| {
                view.read(cx).history.calls.get(index).is_some_and(|call| {
                    call.approval
                        .as_ref()
                        .is_some_and(|approval| approval.state == ApprovalState::Pending)
                })
            });
            let id = view.read_with(visual, |view, _| {
                let page = &view.history.snapshot.as_ref().unwrap().page;
                let call = &view.history.calls[index];
                let details = approvals::details(call, page);
                if index == 0 {
                    assert_eq!(details.description.as_deref(), Some(command));
                    assert_eq!(details.context.as_deref(), Some("nested"));
                    assert_eq!(
                        summary(call, page).detail.text.as_ref(),
                        command.lines().next().unwrap()
                    );
                }
                assert_eq!(details.prompt, details.description);
                call.approval.as_ref().unwrap().id
            });
            fixture::tap(visual, &format!("live-approval-approve-{id}"));
            wait(visual, |cx| {
                view.read(cx).history.calls.get(index).is_some_and(|call| {
                    call.approval
                        .as_ref()
                        .is_some_and(|approval| approval.state == ApprovalState::Approved)
                })
            });
        }
        wait(visual, |cx| {
            view.read(cx).active().is_none() && view.read(cx).history.calls.len() == 3
        });
        fixture.execute(Command::RemovePlugin {
            name: "commands".into(),
            expected_revision: package.summary.revision,
        });
        visual.update(|window, _| window.remove_window());
        drop(view);
        let (view, visual) = fixture::open(cx, fixture.binding.clone(), fixture.session.clone());
        wait(visual, |cx| view.read(cx).history.calls.len() == 3);
        let (turn, source, key) = view.read_with(visual, |view, _| {
            let page = &view.history.snapshot.as_ref().unwrap().page;
            let calls = &view.history.calls;
            assert_eq!(calls[0].display(page).unwrap().label("zh-CN"), "命令");
            assert!(!summary(&calls[0], page).detail.failed);
            assert!(summary(&calls[1], page).detail.failed);
            assert!(has_content(&calls[1], page));
            assert!(!has_content(&calls[2], page));
            let content = calls[0].content.as_ref().unwrap();
            let sailry_protocol::tool::Block::Text(stderr) = &content.blocks[1] else {
                panic!();
            };
            assert_eq!(stderr.notices[0].label("zh-CN"), "部分输出无法解码");
            let source = calls[0].source.key();
            (
                calls[0].turn,
                source.clone(),
                format!("{}-{source}", calls[0].turn),
            )
        });
        fixture::tap(visual, &format!("live-turn-work-{turn}"));
        fixture::tap(visual, &format!("live-{turn}-tool-group-{source}"));
        fixture::tap(visual, &format!("live-{turn}-tools-{source}"));
        redraw(visual);
        for index in 0..2 {
            assert!(
                visual
                    .debug_bounds(Box::leak(
                        format!("tool-content-{key}-{index}").into_boxed_str()
                    ))
                    .is_some()
            );
        }
        fixture::tap(visual, &format!("live-tool-copy-{key}"));
        assert_eq!(
            visual.update(|_, cx| cx.read_from_clipboard().unwrap().text().unwrap()),
            "完整内容 🙂\n```rust\n\ndiagnostic�"
        );
        assert!(
            visual
                .debug_bounds(Box::leak(
                    format!("tool-content-details-{key}").into_boxed_str()
                ))
                .is_none()
        );
        visual.update(|window, _| window.remove_window());
        drop(view);
        fixture.close();
    }
}

#[gpui::test]
fn shows_errors_without_details(cx: &mut TestAppContext) {
    fixture::init(cx);
    for remote in [false, true] {
        let fixture = fixture::Fixture::with_tools(
            remote,
            vec![(
                crate::agent_fixture::plugin_tool("commands", "run_command"),
                json!({"command":"scripts/context7.sh search \"react\"", "cwd":"invalid\u{0000}directory"}),
            )],
        );
        let mut config = fixture.session.config.clone();
        config.permission = sailry_protocol::Permission::Full;
        let Output::Session(session) = fixture.execute(Command::SetSessionConfig {
            session: fixture.session.id,
            expected_revision: 1,
            config,
        }) else {
            panic!("session expected")
        };
        let (view, visual) = fixture::open(cx, fixture.binding.clone(), session.clone());
        fixture.execute(Command::SubmitTurn {
            session: session.id,
            expected_revision: session.revision,
            message: "Inspect the command error".into(),
        });
        wait(visual, |cx| {
            view.read(cx).active().is_none() && view.read(cx).history.calls.len() == 1
        });
        let key = view.update(visual, |view, cx| {
            let page = &view.history.snapshot.as_ref().unwrap().page;
            let call = &view.history.calls[0];
            let error = output::fault(call.result(page)).unwrap();
            assert_eq!(error.message, "invalid command working directory");
            assert!(summary(call, page).detail.failed);
            let turn = call.turn;
            let source = call.source.key();
            let key = format!("{turn}-{source}");
            for entry in [
                "work".into(),
                format!("tool-group-{source}"),
                format!("tools-{source}"),
            ] {
                view.expanded.insert((turn, entry), true);
            }
            cx.notify();
            key
        });
        redraw(visual);
        assert!(
            visual
                .debug_bounds(Box::leak(format!("live-tool-error-{key}").into_boxed_str()))
                .is_some()
        );
        assert!(
            visual
                .debug_bounds(Box::leak(
                    format!("tool-content-details-{key}").into_boxed_str()
                ))
                .is_none()
        );
        fixture::tap(visual, &format!("live-tool-copy-{key}"));
        assert_eq!(
            visual.update(|_, cx| cx.read_from_clipboard().unwrap().text().unwrap()),
            "invalid command working directory"
        );
        visual.update(|window, _| window.remove_window());
        drop(view);
        fixture.close();
    }
}
