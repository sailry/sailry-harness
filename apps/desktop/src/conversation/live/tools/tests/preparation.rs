use super::*;
use sailry_protocol::{
    conversation::Part,
    python::{Preparation, Stage},
};

#[gpui_kit::test]
fn scopes_dependency_status_to_running_skill(cx: &mut TestAppContext) {
    fixture::init(cx);
    for remote in [false, true] {
        let fixture = fixture::Fixture::with_tools(
            remote,
            vec![(
                crate::agent_fixture::plugin_tool("commands", "run_command"),
                json!({"command":"printf ready"}),
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
            message: "Inspect dependency preparation".into(),
        });
        wait(visual, |cx| {
            view.read(cx)
                .history
                .snapshot
                .as_ref()
                .is_some_and(|snapshot| {
                    snapshot
                        .page
                        .runs
                        .first()
                        .is_some_and(|run| run.status == Status::Completed)
                })
        });
        let mut history = view.read_with(visual, |view, _| view.history.clone());
        let call = &mut Arc::make_mut(&mut history.calls)[0];
        call.name = "load_skill".into();
        call.state = State::Running;
        call.response = None;
        call.content = None;
        call.resolved = None;
        let page = Arc::make_mut(&mut Arc::make_mut(history.snapshot.as_mut().unwrap()).page);
        page.runs[0].status = Status::Running;
        let entry = page
            .entries
            .iter_mut()
            .find(|entry| entry.id == call.source.entry)
            .unwrap();
        let Part::ToolCall {
            name,
            arguments,
            display,
            ..
        } = &mut entry.parts[call.source.index]
        else {
            panic!("tool call expected")
        };
        *name = "load_skill".into();
        *arguments = json!({"skill":"office:documents"});
        *display = None;
        let call = &history.calls[0];
        let page = &history.snapshot.as_ref().unwrap().page;
        let mut python = sailry_client::python::View {
            connected: true,
            preparations: vec![Preparation {
                package: "other".into(),
                stage: Stage::Installing,
            }],
            ..Default::default()
        };
        assert_eq!(preparation(&[call], page, &python), None);
        python.preparations[0].package = "office".into();
        for stage in [Stage::Creating, Stage::Installing, Stage::Validating] {
            python.preparations[0].stage = stage;
            view.update(visual, |view, cx| {
                view.python = python.clone();
                let summary = view.preparing_summary(summary(call, page), &[call], page);
                assert_eq!(
                    summary.detail.state,
                    Some(crate::settings::python_stage(stage))
                );
                assert!(summary.detail.running);
                assert!(!summary.detail.failed);
                view.update_history(history.clone(), cx);
                view.expanded.insert((call.turn, "work".into()), true);
            });
            redraw(visual);
            let selector = format!("live-{}-tools-{}", call.turn, call.source.key());
            assert!(
                visual
                    .debug_bounds(Box::leak(selector.into_boxed_str()))
                    .is_some()
            );
        }
        python.connected = false;
        assert_eq!(preparation(&[call], page, &python), None);
        python.connected = true;
        let returned = Call {
            state: State::Returned,
            ..call.clone()
        };
        assert_eq!(preparation(&[&returned], page, &python), None);
        fixture.close();
    }
}
