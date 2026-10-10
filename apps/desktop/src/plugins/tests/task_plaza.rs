//! The ordinary workbench package uses canonical Node reads on local and Link paths.
use super::*;
use crate::{preview::Page, shell::Shell};
use sailry_protocol::{Output, plugin::Info};

mod approvals;
mod previews;
#[cfg(feature = "design-capture")]
pub(in crate::plugins) mod rendering;

fn install(fixture: &Fixture) -> Info {
    let source = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../plugins/task-plaza");
    let directory = fixture.directory.path().join("project/plaza-package");
    crate::plugins::fixture::copy_package(&source, &directory);
    // Script ids are not GPUI debug selectors. The absolute test probe leaves
    // each original styled parent and every native control/event path intact.
    for file in ["view.js", "scene.js", "dialogs.js"] {
        let path = directory.join("dev.sailry.platform/desktop").join(file);
        let source = std::fs::read_to_string(&path).unwrap();
        std::fs::write(
            path,
            format!(
                "import {{Bounds}} from 'sailry/test';\n{}\nfunction measured(id) {{return div().id(id).relative().child(Bounds.new(id));}}\nfunction measuredButton(id) {{return new Button(id).relative().child(Bounds.new(id));}}\n",
                source.replace("div().id(", "measured(").replace("new Button(", "measuredButton(")
            ),
        ).unwrap();
    }
    let Output::Plugin(info) = fixture.execute(Command::InstallPlugin {
        worktree: fixture.session.worktree,
        path: "plaza-package".into(),
        name: "task-plaza".into(),
        expected_revision: 0,
    }) else {
        panic!("task plaza package expected");
    };
    assert!(info.issues.is_empty(), "{:?}", info.issues);
    info
}

fn open(shell: &Entity<Shell>, fixture: &Fixture, visual: &mut VisualTestContext) -> Entity<Panel> {
    wait(visual, |cx| {
        shell
            .read(cx)
            .extension_entries(cx)
            .iter()
            .any(|entry| entry.package.name == "task-plaza")
    });
    visual.update(|window, cx| {
        shell.update(cx, |shell, cx| {
            let entry = shell
                .extension_entries(cx)
                .into_iter()
                .find(|entry| entry.package.name == "task-plaza")
                .unwrap();
            shell.open_extension(entry, window, cx);
        })
    });
    let panel = shell.read_with(visual, |shell, _| {
        shell.extensions.as_ref().unwrap().panel.clone().unwrap()
    });
    wait(visual, |cx| {
        snapshot(&panel, cx).contains("plaza-board")
            && snapshot(&panel, cx).contains(&format!("plaza-{}", fixture.session.id))
    });
    panel
}

fn selector(value: impl Into<String>) -> &'static str {
    Box::leak(value.into().into_boxed_str())
}

fn chat(panel: &Entity<Panel>, cx: &App) -> Option<Entity<crate::conversation::live::View>> {
    panel.read(cx).mounted.as_ref()?.conversations.first()
}

#[gpui::test]
fn rooms_dialogs_and_lifecycle(cx: &mut TestAppContext) {
    init(cx);
    for remote in [false, true] {
        let fixture = Fixture::new(remote);
        let package = install(&fixture);
        let (shell, visual) = files::mount(&fixture, remote, cx);
        let panel = open(&shell, &fixture, visual);
        assert!(shell.read_with(visual, |shell, cx| {
            shell
                .extension_entries(cx)
                .iter()
                .any(|entry| entry.package.name == "progress")
        }));
        let project = fixture.session.project.unwrap();
        let room = selector(format!("plaza-room-{project}"));
        let initial = selector(format!("plaza-{}", fixture.session.id));
        let mut sessions = vec![fixture.session.clone()];
        for _ in 0..6 {
            let Output::Session(session) = fixture.execute(Command::CreateSession {
                project: fixture.session.project,
                worktree: Some(fixture.session.worktree),
                config: Some(fixture.session.config.clone()),
            }) else {
                panic!("session expected");
            };
            sessions.push(session);
        }
        let next = selector(format!("plaza-room-next-{project}"));
        wait(visual, |cx| snapshot(&panel, cx).contains(next));
        let handle = visual.update(|window, _| window.window_handle());
        for width in [1440., 900.] {
            visual.simulate_window_resize(handle, size(px(width), px(1000.)));
            visual.update(|window, cx| window.draw(cx).clear(cx));
            let world = visual.debug_bounds("plaza-world").unwrap();
            let plaza = visual.debug_bounds("plaza-atrium").unwrap();
            assert!((world.size.width / world.size.height - 1120. / 565.).abs() < 0.01);
            assert!(
                (visual.debug_bounds(initial).unwrap().size.width
                    - world.size.width * (40. / 1120.))
                    .abs()
                    < px(1.)
            );
            assert!(world.contains(&plaza.center()));
            assert!((plaza.center().x - world.center().x).abs() < world.size.width * 0.1);
            assert!((plaza.center().y - world.center().y).abs() < world.size.height * 0.12);
            for index in 0..8 {
                let key = if index == 0 {
                    project.to_string()
                } else {
                    index.to_string()
                };
                for seat in 0..6 {
                    assert!(
                        visual
                            .debug_bounds(selector(format!("plaza-desk-{key}-{seat}")))
                            .is_some()
                    );
                }
            }
            assert!(visual.debug_bounds(room).is_some());
            assert!(visual.debug_bounds("shell-feature-rail").is_some());
            assert!(visual.debug_bounds("plaza-host-filter").is_none());
        }
        visual.simulate_window_resize(handle, size(px(1440.), px(1000.)));
        visual.update(|window, cx| window.draw(cx).clear(cx));
        let before = visual.debug_bounds("plaza-world").unwrap().size;
        click(visual, "plaza-zoom-in");
        assert!(visual.debug_bounds("plaza-world").unwrap().size.width > before.width);
        click(visual, "plaza-zoom-reset");
        assert_eq!(visual.debug_bounds("plaza-world").unwrap().size, before);
        let latest = sessions.last().unwrap();
        let last = selector(format!("plaza-{}", latest.id));
        assert!(visual.debug_bounds(last).is_none());
        click(visual, next);
        assert!(visual.debug_bounds(last).is_some());
        assert!(visual.debug_bounds(initial).is_none());
        click(visual, room);
        wait(visual, |cx| {
            snapshot(&panel, cx).contains("plaza-detail-list")
        });
        for session in &sessions {
            assert!(
                visual
                    .debug_bounds(selector(format!("plaza-detail-session-{}", session.id)))
                    .is_some()
            );
        }
        click(
            visual,
            selector(format!("plaza-detail-session-{}", latest.id)),
        );
        wait(visual, |cx| {
            chat(&panel, cx).is_some_and(|chat| chat.read(cx).connected())
        });
        assert_eq!(shell.read_with(visual, |shell, _| shell.page), Page::Plugin);
        let embedded = panel.read_with(visual, |_, cx| chat(&panel, cx).unwrap());
        embedded.read_with(visual, |chat, _| {
            assert_eq!(chat.session(), Some(latest.id));
            assert_eq!(chat.binding().client.target(), fixture.node.id());
            assert_eq!(chat.binding().worktree, Some(latest.worktree));
        });
        let weak = embedded.downgrade();
        drop(embedded);
        click(visual, "plaza-dialog-close");
        wait(visual, |cx| {
            chat(&panel, cx).is_none() && weak.upgrade().is_none()
        });
        click(visual, selector(format!("plaza-room-prev-{project}")));
        click(visual, initial);
        wait(visual, |cx| {
            chat(&panel, cx).is_some_and(|chat| chat.read(cx).session() == Some(fixture.session.id))
        });
        click(visual, "plaza-open-conversation");
        wait(visual, |cx| {
            shell.read(cx).page == Page::Conversation
                && shell
                    .read(cx)
                    .current_chat()
                    .is_some_and(|chat| chat.read(cx).session() == Some(fixture.session.id))
        });
        let observed = fixture.transport.requests.lock().unwrap().clone();
        assert!(observed.iter().any(|request| matches!(
            request.command,
            Command::ReadActivityCatalog
        ) && request.plugin.as_ref().is_some_and(
            |context| context.package == package.summary.reference()
                && context.worktree.is_none()
                && context.session.is_none()
        )));
        let mounted = open(&shell, &fixture, visual);
        assert_eq!(mounted, panel);
        fixture.execute(Command::SetPluginEnabled {
            name: "task-plaza".into(),
            expected_revision: package.summary.revision,
            enabled: false,
        });
        wait(visual, |cx| {
            mounted.read(cx).mounted.is_none()
                && !shell
                    .read(cx)
                    .extension_entries(cx)
                    .iter()
                    .any(|entry| entry.package.name == "task-plaza")
        });
        visual.update(|window, _| window.remove_window());
        drop(mounted);
        drop(panel);
        drop(shell);
        fixture.close();
    }
}
