use super::*;
use core::prelude::v1::test;

#[test]
fn distinguishes_task_and_queue_busy() {
    let mut error = Fault::new(ErrorCode::Busy, "fixture queue is full");
    assert_eq!(failure(&error), ("project_remove_busy", false));
    error.details = Some(FaultDetails::ProjectRemoval {
        tasks_active: true,
        terminals_may_be_closed: false,
    });
    assert_eq!(failure(&error), ("project_remove_tasks", false));
    error.details = Some(FaultDetails::ProjectRemoval {
        tasks_active: true,
        terminals_may_be_closed: true,
    });
    assert_eq!(failure(&error), ("project_remove_tasks_partial", false));
}

#[test]
fn distinguishes_cleanup_and_transport_failures() {
    for code in [ErrorCode::NotFound, ErrorCode::Unavailable] {
        let mut error = Fault::new(code, "fixture terminal is unavailable");
        error.details = Some(FaultDetails::ProjectRemoval {
            tasks_active: false,
            terminals_may_be_closed: true,
        });
        assert_eq!(failure(&error), ("project_remove_failed", false));
        error.details = None;
        assert_eq!(
            failure(&error),
            if code == ErrorCode::Unavailable {
                ("project_remove_unknown", true)
            } else {
                ("project_changed", false)
            },
        );
    }
}

#[test]
fn preserves_uncertain_identity() {
    let mut error = Fault::new(ErrorCode::OutcomeUnknown, "fixture result was not recorded");
    error.details = Some(FaultDetails::ProjectRemoval {
        tasks_active: false,
        terminals_may_be_closed: true,
    });
    assert_eq!(failure(&error), ("project_remove_unknown", true));
    error.code = ErrorCode::RevisionConflict;
    assert_eq!(failure(&error), ("project_remove_changed_partial", false));
}

fn launch(worktree: sailry_protocol::WorktreeId) -> sailry_protocol::terminal::Launch {
    use sailry_protocol::terminal::*;
    Launch {
        access: sailry_protocol::process::Access::Workspace,
        worktree,
        viewport: Viewport {
            columns: 80,
            rows: 24,
            pixel_width: 640,
            pixel_height: 480,
        },
        appearance: Appearance {
            foreground: Rgb {
                red: 240,
                green: 240,
                blue: 240,
            },
            background: Rgb {
                red: 20,
                green: 20,
                blue: 20,
            },
            palette: [Rgb {
                red: 128,
                green: 128,
                blue: 128,
            }; 16],
            color_scheme: ColorScheme::Dark,
        },
    }
}

fn wait(cx: &mut VisualTestContext, form: &Entity<Removal>, predicate: impl Fn(&Removal) -> bool) {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    loop {
        cx.run_until_parked();
        if form.read_with(cx, |form, _| predicate(form)) {
            return;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "project removal response deadline"
        );
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
}

#[gpui::test]
fn confirms_before_closing(cx: &mut TestAppContext) {
    use crate::activity::fixture::Fixture;
    use gpui_kit::component::Root;
    use sailry_protocol::terminal::Status;
    cx.executor().allow_parking();
    let fixture = Fixture::new();
    cx.update(|cx| {
        gpui_kit::init(cx);
        crate::theme::init(cx);
        crate::shell::init(cx);
        cx.set_global(fixture.services());
    });
    let mut owner = None;
    let (_, visual) = cx.add_window_view(|window, cx| {
        let shell = cx.new(|cx| Shell::new(window, cx));
        owner = Some(shell.clone());
        Root::new(shell, window, cx)
    });
    let owner = owner.unwrap();
    for index in 0..2 {
        let transport = if index == 0 {
            fixture.nodes[0].local()
        } else {
            fixture.nodes[0]
                .link()
                .remote(fixture.nodes[1].link().address())
        };
        let client = Arc::new(Client::new(transport));
        let execute = |command| {
            fixture
                .runtime
                .block_on(client.execute(client.prepare(command)))
                .unwrap()
        };
        let Output::Snapshot(snapshot) = execute(Command::Snapshot) else {
            panic!("snapshot expected")
        };
        let session = &fixture.sessions[index];
        let expected = snapshot
            .projects
            .into_iter()
            .find(|project| Some(project.id) == session.project)
            .unwrap();
        let Output::Terminal(terminal) = execute(Command::CreateTerminal(launch(session.worktree)))
        else {
            panic!("terminal expected")
        };
        let request = client.prepare(Command::RemoveProject {
            expected: expected.clone(),
        });
        let form = visual.update(|_, cx| {
            cx.new(|_| Removal {
                owner: owner.downgrade(),
                client: client.clone(),
                expected: expected.clone(),
                request,
                pending: false,
                error: None,
            })
        });
        visual.update(|window, cx| form.update(cx, |form, cx| form.prompt(window, cx)));
        let (title, detail) = crate::prompts::tests::wait(visual);
        assert_eq!(title, tr("project_remove").as_ref());
        assert_eq!(
            detail,
            rust_i18n::t!("project_remove_named", name = expected.path).to_string()
        );
        crate::prompts::tests::answer(visual, "settings_cancel");
        let Output::Snapshot(snapshot) = execute(Command::Snapshot) else {
            panic!("snapshot expected")
        };
        assert_eq!(
            snapshot
                .terminals
                .iter()
                .find(|info| info.id == terminal.id)
                .unwrap()
                .status,
            Status::Running
        );

        let Output::QueuedTurn(turn) = execute(Command::QueueTurn {
            session: session.id,
            expected_revision: session.revision,
            message: "Hold the isolated task".into(),
        }) else {
            panic!("turn expected")
        };
        let original = form.read_with(visual, |form, _| form.request.id);
        visual.update(|window, cx| form.update(cx, |form, cx| form.prompt(window, cx)));
        crate::prompts::tests::answer(visual, "project_remove");
        wait(visual, &form, |form| {
            form.error == Some("project_remove_tasks") && !form.pending
        });
        assert_eq!(
            visual.update(|window, cx| crate::feedback::tests::summary(window, cx)),
            tr("project_remove_tasks")
        );
        assert_ne!(form.read_with(visual, |form, _| form.request.id), original);
        let Output::Snapshot(snapshot) = execute(Command::Snapshot) else {
            panic!("snapshot expected")
        };
        assert!(
            snapshot
                .projects
                .iter()
                .any(|project| project.id == expected.id)
        );
        assert_eq!(
            snapshot
                .terminals
                .iter()
                .find(|info| info.id == terminal.id)
                .unwrap()
                .status,
            Status::Running
        );
        crate::prompts::tests::answer(visual, "settings_cancel");
        execute(Command::StopTurn { turn: turn.id });
        visual.update(|window, cx| form.update(cx, |form, cx| form.prompt(window, cx)));
        crate::prompts::tests::answer(visual, "project_remove");
        wait(visual, &form, |form| !form.pending && form.error.is_none());
        assert_eq!(
            visual.update(|window, cx| crate::feedback::tests::summary(window, cx)),
            tr("project_removed")
        );
        assert_eq!(
            visual.update(|window, cx| crate::feedback::tests::count(
                window,
                &tr("project_removed"),
                cx
            )),
            index + 1
        );
        assert!(visual.pending_prompt().is_none());
        let Output::Snapshot(snapshot) = execute(Command::Snapshot) else {
            panic!("snapshot expected")
        };
        assert!(
            !snapshot
                .projects
                .iter()
                .any(|project| project.id == expected.id)
        );
        assert_eq!(
            snapshot
                .terminals
                .iter()
                .find(|info| info.id == terminal.id)
                .unwrap()
                .status,
            Status::Closed
        );
        assert!(snapshot.sessions.iter().any(|value| value.id == session.id));
        assert!(std::path::Path::new(&expected.path).is_dir());
    }
    visual.update(|window, _| window.remove_window());
    fixture.close();
}
