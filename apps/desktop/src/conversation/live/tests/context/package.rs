//! Keep the isolated conversation harness while attaching the real Shell event host.
use super::*;
use crate::shell::Shell;

pub(super) fn install(fixture: &fixture::Fixture) {
    let root = fixture.directory.path().join("project/worktrees-package");
    crate::plugins::fixture::copy_package(
        &std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../plugins/worktrees"),
        &root,
    );
    let path = root.join("dev.sailry.platform/desktop/view.js");
    let source = std::fs::read_to_string(&path).unwrap()
        .replace("return div().id(id).child(control);", "return Anchor.new(id).child(control);")
        .replace("div().id(owner.pending || !owner.status ? 'location-branch-loading' : 'location-branch-name')", "Anchor.new(owner.pending || !owner.status ? 'location-branch-loading' : 'location-branch-name')")
        .replace("div().id('location-copy-changes').child", "Anchor.new('location-copy-changes').child");
    std::fs::write(
        path,
        format!("import {{Anchor}} from 'sailry/test';\n{source}"),
    )
    .unwrap();
    let Output::Plugin(info) = fixture.execute(Command::ReadPlugin {
        name: "worktrees".into(),
    }) else {
        panic!("Worktrees package expected")
    };
    let Output::Plugin(info) = fixture.execute(Command::InstallPlugin {
        worktree: fixture.session.worktree,
        path: "worktrees-package".into(),
        name: "worktrees".into(),
        expected_revision: info.summary.revision,
    }) else {
        panic!("Worktrees package expected")
    };
    assert!(info.issues.is_empty(), "{:?}", info.issues);
}

pub(super) fn open<'a>(
    fixture: &fixture::Fixture,
    cx: &'a mut TestAppContext,
    binding: Binding,
    session: Option<Session>,
) -> (Entity<Shell>, Entity<View>, &'a mut VisualTestContext) {
    cx.update(|cx| {
        crate::shell::init(cx);
        cx.set_global(crate::backend::Services {
            runtime: fixture.runtime.clone(),
            link: fixture.node.link(),
            local: fixture.node.local(),
            relay_enabled: false,
        });
    });
    let (view, visual) = fixture::open_session(cx, binding, session);
    let shell = visual.update(|window, cx| {
        let shell = cx.new(|cx| Shell::new(window, cx));
        shell.update(cx, |_, cx| {
            cx.subscribe_in(&view, window, |_, _, event, window, cx| {
                if let Event::PluginMounted(panel) = event {
                    Shell::observe_plugin_conversations(panel, window, cx);
                }
            })
            .detach();
        });
        shell
    });
    (shell, view, visual)
}
