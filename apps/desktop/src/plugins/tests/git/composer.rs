use super::*;
use crate::conversation::live::tests::{contributions, fixture as chat};

fn install(fixture: &Fixture) {
    let source = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../plugins/git");
    fixture::copy_package(
        &source,
        &fixture.directory.path().join("project/git-package"),
    );
    let Output::Plugin(info) = fixture.execute(Command::InstallPlugin {
        worktree: fixture.session.worktree,
        path: "git-package".into(),
        name: "git".into(),
        expected_revision: package(fixture).summary.revision,
    }) else {
        panic!("Git package expected")
    };
    assert!(info.issues.is_empty(), "{:?}", info.issues);
}

fn reads(fixture: &Fixture) -> Vec<Request> {
    fixture
        .transport
        .requests
        .lock()
        .unwrap()
        .iter()
        .filter(|request| {
            request
                .plugin
                .as_ref()
                .is_some_and(|scope| scope.package.name == "git")
                && matches!(
                    request.command,
                    Command::InspectGit { .. }
                        | Command::ListGitBranches { .. }
                        | Command::ReadGitDiff { .. }
                )
        })
        .cloned()
        .collect()
}

fn ready(view: &Entity<crate::conversation::live::View>, visual: &mut VisualTestContext) {
    wait(visual, |cx| {
        view.read(cx).connected()
            && contributions(view.read(cx)).read(cx).ready(cx)
            && view
                .read(cx)
                .plugin_panel("git", cx)
                .is_some_and(|panel| !panel.read(cx).loading)
    });
}

#[gpui::test]
fn skips_reads_without_a_repository(cx: &mut TestAppContext) {
    init(cx);
    for remote in [false, true] {
        let fixture = Fixture::directory(remote);
        install(&fixture);
        let Output::Session(unassigned) = fixture.execute(Command::CreateSession {
            project: None,
            worktree: None,
            config: Some(fixture.session.config.clone()),
        }) else {
            panic!("session expected")
        };
        for session in [fixture.session.clone(), unassigned] {
            let mut binding = fixture.binding.clone();
            binding.project = session.project;
            binding.worktree = Some(session.worktree);
            if session.project.is_none() {
                binding.project_name = "".into();
                binding.branch = "".into();
            }
            fixture.transport.requests.lock().unwrap().clear();
            let (view, visual) = chat::open(cx, binding, session);
            ready(&view, visual);
            assert!(reads(&fixture).is_empty());
            assert!(visual.update(|window, cx| window.notifications(cx).is_empty()));
            visual.update(|window, _| window.remove_window());
            drop(view);
        }
        fixture.close();
    }
}

#[gpui::test]
fn reads_after_repository_detection(cx: &mut TestAppContext) {
    init(cx);
    for remote in [false, true] {
        let fixture = Fixture::new(remote);
        prepare(&fixture);
        fixture.transport.requests.lock().unwrap().clear();
        let (view, visual) = chat::open(cx, fixture.binding.clone(), fixture.session.clone());
        ready(&view, visual);
        wait(visual, |_| !reads(&fixture).is_empty());
        assert!(
            reads(&fixture)
                .iter()
                .any(|request| matches!(request.command, Command::InspectGit { .. }))
        );
        assert!(visual.update(|window, cx| window.notifications(cx).is_empty()));
        visual.update(|window, _| window.remove_window());
        drop(view);
        fixture.close();
    }
}
