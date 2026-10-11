use super::*;

#[gpui::test]
fn failure_uses_activity_spacing(cx: &mut TestAppContext) {
    fixture::init(cx);
    for remote in [false, true] {
        let fixture = fixture::Fixture::with_tools(
            remote,
            vec![(
                crate::agent_fixture::plugin_tool("commands", "run_command"),
                serde_json::json!({"command":"exit 0"}),
            )],
        );
        let mut config = fixture.session.config.clone();
        config.permission = sailry_protocol::Permission::Full;
        let Output::Session(session) = fixture.execute(Command::SetSessionConfig {
            session: fixture.session.id,
            expected_revision: fixture.session.revision,
            config,
        }) else {
            panic!("session expected")
        };
        let (view, visual) = fixture::open(cx, fixture.binding.clone(), session.clone());
        fixture.execute(Command::SubmitTurn {
            session: session.id,
            expected_revision: session.revision,
            message: "Run the fixture tools".into(),
        });
        wait(visual, |cx| {
            view.read(cx).history.calls.first().is_some_and(|call| {
                call.state == sailry_client::conversation::tools::State::Returned
            }) && view.read(cx).active().is_none()
        });
        let (turn, key) = view.read_with(visual, |view, _| {
            (
                view.history.calls[0].turn,
                view.history.calls[0].source.key(),
            )
        });
        view.update(visual, |view, cx| {
            let mut history = view.history.clone();
            let page = Arc::make_mut(&mut Arc::make_mut(history.snapshot.as_mut().unwrap()).page);
            let run = page.runs.iter_mut().find(|run| run.turn == turn).unwrap();
            run.status = Status::Failed;
            run.error = Some(sailry_protocol::Fault::new(
                sailry_protocol::ErrorCode::Unavailable,
                "Response unavailable",
            ));
            for entry in page
                .entries
                .iter_mut()
                .filter(|entry| entry.author != "user")
            {
                entry.parts.retain(|part| !matches!(part, Part::Text(_)));
            }
            view.update_history(history, cx);
        });
        fixture::tap(visual, &format!("live-turn-work-{turn}"));
        wait(visual, |_| true);
        let tool = visual
            .debug_bounds(Box::leak(
                format!("live-{turn}-tools-{key}").into_boxed_str(),
            ))
            .unwrap();
        let failure = visual
            .debug_bounds(Box::leak(
                format!("live-turn-error-{turn}").into_boxed_str(),
            ))
            .unwrap();
        let expected = visual.update(|_, cx| cx.theme().spacing_tokens().lg);
        assert_eq!(
            failure.top() - tool.bottom(),
            expected,
            "failure spacing: remote={remote}, tool={tool:?}, failure={failure:?}"
        );
        fixture::tap(visual, &format!("live-turn-error-{turn}"));
        assert!(
            visual
                .debug_bounds(Box::leak(
                    format!("live-turn-error-reason-{turn}").into_boxed_str()
                ))
                .is_some()
        );
        visual.update(|window, _| window.remove_window());
        fixture.close();
    }
}
