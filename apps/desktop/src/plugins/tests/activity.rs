//! The ordinary workbench package uses canonical Node reads on local and Link paths.
use super::*;
use crate::{preview::Page, shell::Shell};
use sailry_protocol::{Output, Session, plugin::Info};

mod previews;
mod terminals;

fn install(fixture: &Fixture) -> Info {
    let Output::Plugin(current) = fixture.execute(Command::ReadPlugin {
        name: "progress".into(),
    }) else {
        panic!("progress package expected");
    };
    let source = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../plugins/progress");
    let directory = fixture.directory.path().join("project/progress-package");
    crate::plugins::fixture::copy_package(&source, &directory);
    // Script ids are not GPUI debug selectors. The absolute test probe leaves
    // each original styled parent and every native control/event path intact.
    let view = directory.join("dev.sailry.platform/desktop/view.js");
    let source = std::fs::read_to_string(&view).unwrap();
    std::fs::write(
        view,
        format!(
            "import {{Bounds}} from 'sailry/test';\n{}\nfunction measured(id) {{return div().id(id).relative().child(Bounds.new(id));}}\n",
            source.replace("div().id(", "measured(")
        ),
    )
    .unwrap();
    let Output::Plugin(info) = fixture.execute(Command::InstallPlugin {
        worktree: fixture.session.worktree,
        path: "progress-package".into(),
        name: "progress".into(),
        expected_revision: current.summary.revision,
    }) else {
        panic!("progress package expected");
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
            .any(|entry| entry.package.name == "progress")
    });
    visual.update(|window, cx| {
        shell.update(cx, |shell, cx| {
            let entry = shell
                .extension_entries(cx)
                .into_iter()
                .find(|entry| entry.package.name == "progress")
                .unwrap();
            shell.open_extension(entry, window, cx);
        })
    });
    let panel = shell.read_with(visual, |shell, _| {
        shell.extensions.as_ref().unwrap().panel.clone().unwrap()
    });
    wait(visual, |cx| {
        snapshot(&panel, cx).contains("activity-board")
            && snapshot(&panel, cx).contains(&format!("activity-{}", fixture.session.id))
    });
    panel
}

fn empty_lane(visual: &mut VisualTestContext, key: &str) {
    let selector =
        |suffix: &str| &*Box::leak(format!("{key}-{suffix}").into_boxed_str()) as &'static str;
    let column = visual.debug_bounds(selector("column")).unwrap();
    let items = visual.debug_bounds(selector("items")).unwrap();
    let body = visual
        .debug_bounds(Box::leak(format!("empty-{key}-empty").into_boxed_str()))
        .unwrap();
    assert!(
        visual
            .debug_bounds(Box::leak(
                format!("empty-card-{key}-empty").into_boxed_str()
            ))
            .is_none()
    );
    assert_eq!(body.size.width, column.size.width);
    assert!((body.top() - items.top()).abs() < px(1.));
    assert!(
        (body.bottom() - column.bottom()).abs() < px(1.),
        "empty lane {key}: body={body:?}, items={items:?}, column={column:?}"
    );
    assert!((body.size.height - items.size.height).abs() < px(1.));
    assert!(visual.debug_bounds(selector("heading")).unwrap().bottom() < body.top());
    let icon = visual
        .debug_bounds(Box::leak(
            format!("empty-icon-{key}-empty").into_boxed_str(),
        ))
        .unwrap();
    let title = visual
        .debug_bounds(Box::leak(
            format!("empty-title-{key}-empty").into_boxed_str(),
        ))
        .unwrap();
    assert!((icon.top() - body.top() - px(40.)).abs() < px(1.));
    assert!(icon.bottom() < title.top());
}

#[gpui::test]
fn cards_disclosures_and_lifecycle(cx: &mut TestAppContext) {
    init(cx);
    for remote in [false, true] {
        let fixture = Fixture::new(remote);
        let package = install(&fixture);
        let (shell, visual) = files::mount(&fixture, remote, cx);
        let panel = open(&shell, &fixture, visual);
        let project = format!(
            "activity-project-activity_idle-{}",
            fixture.session.project.unwrap()
        );
        let viewport: &'static str = Box::leak(format!("{project}-viewport").into_boxed_str());
        let header: &'static str = Box::leak(project.clone().into_boxed_str());
        let initial: &'static str =
            Box::leak(format!("activity-{}", fixture.session.id).into_boxed_str());
        let single = visual.debug_bounds(initial).unwrap();
        let short_list = visual.debug_bounds(viewport).unwrap();
        assert_eq!(short_list.size.height, single.size.height);
        assert_eq!(short_list.top(), single.top());
        assert!(visual.debug_bounds(header).unwrap().bottom() < short_list.top());
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
        // A post-init subscription refresh must use a live async notifier.
        wait(visual, |cx| {
            snapshot(&panel, cx).contains(&format!("activity-{}", sessions.last().unwrap().id))
        });
        assert_eq!(shell.read_with(visual, |shell, _| shell.page), Page::Plugin);
        assert!(visual.debug_bounds("activity-host-filter").is_none());
        let handle = visual.update(|window, _| window.window_handle());
        for width in [1440., 900.] {
            visual.simulate_window_resize(handle, size(px(width), px(1000.)));
            visual.update(|window, cx| window.draw(cx).clear(cx));
            let columns = [
                "activity_board_waiting-column",
                "activity_board_running-column",
                "activity_completed-column",
                "activity_idle-column",
            ]
            .map(|key| visual.debug_bounds(key).unwrap());
            for pair in columns.windows(2) {
                assert!(pair[0].right() < pair[1].left());
                assert_eq!(pair[0].top(), pair[1].top());
                assert!((pair[0].size.width - pair[1].size.width).abs() < px(1.));
            }
            assert!(columns.iter().all(|column| column.size.width >= px(240.)));
            for key in [
                "activity_board_waiting",
                "activity_board_running",
                "activity_completed",
            ] {
                empty_lane(visual, key);
            }
            let height = visual.debug_bounds(viewport).unwrap().size.height;
            let expected = visual.update(|window, _| window.rem_size() * 32.5);
            assert!(
                (height - expected).abs() < px(1.),
                "an overflowing project must use the compact maximum viewport height"
            );
            if width > 1000. {
                assert!(
                    columns.last().unwrap().right()
                        <= visual.debug_bounds("activity-board").unwrap().right() + px(1.)
                );
            } else {
                let board = visual.debug_bounds("activity-board").unwrap();
                let list = visual.debug_bounds(viewport).unwrap();
                let before = visual.debug_bounds(initial).unwrap();
                let window_size = visual.update(|window, _| window.viewport_size());
                let panel_bounds = visual.debug_bounds("plugin-panel").unwrap();
                // The board probe measures expanded scroll content, not its clipped viewport.
                let left = list.left().max(panel_bounds.left()).max(px(0.));
                let right = list
                    .right()
                    .min(panel_bounds.right())
                    .min(window_size.width);
                let top = list.top().max(panel_bounds.top()).max(px(0.));
                let bottom = list
                    .bottom()
                    .min(panel_bounds.bottom())
                    .min(window_size.height);
                assert!(left < right && top < bottom);
                let position = point((left + right) / 2., (top + bottom) / 2.);
                visual.simulate_event(ScrollWheelEvent {
                    position,
                    delta: ScrollDelta::Pixels(point(px(-100.), px(0.))),
                    ..Default::default()
                });
                visual.run_until_parked();
                visual.update(|window, cx| window.draw(cx).clear(cx));
                let after = visual.debug_bounds(initial).unwrap();
                assert_eq!(
                    after.top(),
                    before.top(),
                    "horizontal wheel must not scroll project cards vertically"
                );
                assert!(
                    after.left() < before.left(),
                    "horizontal wheel must reach the board: position={position:?}, window={window_size:?}, panel={panel_bounds:?}, board={board:?}, list={list:?}, before={before:?}, after={after:?}"
                );
                let moved_board = visual.debug_bounds("activity-board").unwrap();
                assert!(moved_board.left() < board.left());
                assert_eq!(moved_board.top(), board.top());
                assert_eq!(moved_board.size, board.size);
                assert_eq!(visual.debug_bounds("plugin-panel").unwrap(), panel_bounds);
                assert!(
                    visual
                        .debug_bounds("activity_board_waiting-column")
                        .unwrap()
                        .left()
                        < columns[0].left()
                );
                let list = visual.debug_bounds(viewport).unwrap();
                let heading = visual.debug_bounds(header).unwrap();
                let peer = visual
                    .debug_bounds("empty-activity_board_waiting-empty")
                    .unwrap();
                let position = point(
                    (list.left().max(panel_bounds.left()).max(px(0.))
                        + list
                            .right()
                            .min(panel_bounds.right())
                            .min(window_size.width))
                        / 2.,
                    (list.top().max(panel_bounds.top()).max(px(0.))
                        + list
                            .bottom()
                            .min(panel_bounds.bottom())
                            .min(window_size.height))
                        / 2.,
                );
                visual.simulate_event(ScrollWheelEvent {
                    position,
                    delta: ScrollDelta::Pixels(point(px(0.), px(-300.))),
                    ..Default::default()
                });
                visual.run_until_parked();
                visual.update(|window, cx| window.draw(cx).clear(cx));
                assert!(visual.debug_bounds(initial).unwrap().top() < after.top());
                let last: &'static str =
                    Box::leak(format!("activity-{}", sessions.last().unwrap().id).into_boxed_str());
                let last = visual.debug_bounds(last).unwrap();
                assert!(last.top() >= list.top() && last.bottom() <= list.bottom());
                assert_eq!(visual.debug_bounds(header).unwrap(), heading);
                assert_eq!(visual.debug_bounds(viewport).unwrap(), list);
                assert_eq!(
                    visual
                        .debug_bounds("empty-activity_board_waiting-empty")
                        .unwrap(),
                    peer
                );
                for delta in [point(px(0.), px(300.)), point(px(100.), px(0.))] {
                    visual.simulate_event(ScrollWheelEvent {
                        position,
                        delta: ScrollDelta::Pixels(delta),
                        ..Default::default()
                    });
                    visual.run_until_parked();
                    visual.update(|window, cx| window.draw(cx).clear(cx));
                }
                assert_eq!(visual.debug_bounds(initial).unwrap(), before);
            }
        }
        visual.simulate_window_resize(handle, size(px(1440.), px(1000.)));
        visual.update(|window, cx| window.draw(cx).clear(cx));
        assert!(visual.debug_bounds("shell-navigation").is_none());
        assert!(visual.debug_bounds("header-sidebar-toggle").is_none());
        assert!(visual.debug_bounds("shell-feature-rail").is_some());
        let list = visual.debug_bounds(viewport).unwrap();
        let heading = visual.debug_bounds(header).unwrap();
        let cards: Vec<&'static str> = sessions
            .iter()
            .map(|session| &*Box::leak(format!("activity-{}", session.id).into_boxed_str()))
            .collect();
        let first = visual.debug_bounds(cards[0]).unwrap();
        let last = visual.debug_bounds(cards.last().unwrap()).unwrap();
        assert!(heading.bottom() < list.top());
        assert_eq!(first.top(), list.top());
        assert!(last.bottom() > list.bottom());
        let rem = visual.update(|window, _| window.rem_size());
        for (session, selector) in sessions.iter().zip(&cards).take(5) {
            let bounds = visual.debug_bounds(selector).unwrap();
            let part = |name: &str| {
                Box::leak(format!("activity-{}-{name}", session.id).into_boxed_str())
                    as &'static str
            };
            let title = visual.debug_bounds(part("title")).unwrap();
            let content = visual.debug_bounds(part("content")).unwrap();
            let location = visual.debug_bounds(part("location")).unwrap();
            assert_eq!(title.size.height, rem * 1.25);
            assert_eq!(content.size.height, rem * 1.25);
            assert_eq!(
                bounds.size.height,
                title.size.height + content.size.height + rem * 2. + px(2.)
            );
            assert!(content.top() > title.bottom());
            assert_eq!(location.top(), title.top() + rem * 0.125);
            assert!(location.left() > title.right());
            assert!(bounds.contains(&location.center()));
            assert!(
                location.contains(&visual.debug_bounds(part("project-icon")).unwrap().center())
            );
            assert!(visual.debug_bounds(part("worktree-icon")).is_none());
        }
        let empty = visual
            .debug_bounds("empty-activity_board_waiting-empty")
            .unwrap();
        let empty_icon = visual
            .debug_bounds("empty-icon-activity_board_waiting-empty")
            .unwrap();
        let empty_column = visual
            .debug_bounds("activity_board_waiting-column")
            .unwrap();
        assert_eq!(empty.size.width, empty_column.size.width);
        assert!(empty.bottom() <= empty_column.bottom());
        assert!(
            visual
                .debug_bounds("activity_board_waiting-heading")
                .unwrap()
                .bottom()
                < empty.top()
        );
        assert_eq!(empty_icon.size, size(px(48.), px(48.)));
        empty_lane(visual, "activity_board_waiting");
        assert!(
            visual
                .debug_bounds("empty-title-activity_board_waiting-empty")
                .unwrap()
                .size
                .height
                <= px(24.)
        );
        visual.simulate_event(ScrollWheelEvent {
            position: list.center(),
            delta: ScrollDelta::Pixels(point(px(0.), px(-300.))),
            ..Default::default()
        });
        visual.run_until_parked();
        visual.update(|window, cx| window.draw(cx).clear(cx));
        let sixth = visual.debug_bounds(cards[5]).unwrap();
        assert!(sixth.top() >= list.top() && sixth.bottom() <= list.bottom());
        assert!(visual.debug_bounds(cards[0]).unwrap().top() < first.top());
        let last = visual.debug_bounds(cards.last().unwrap()).unwrap();
        assert!(last.top() >= list.top() && last.bottom() <= list.bottom());
        assert_eq!(visual.debug_bounds(viewport).unwrap(), list);
        assert_eq!(visual.debug_bounds(header).unwrap(), heading);
        assert_eq!(
            visual
                .debug_bounds("empty-activity_board_waiting-empty")
                .unwrap(),
            empty
        );
        visual.update(|window, cx| {
            shell.update(cx, |_, cx| cx.notify());
            window.draw(cx).clear(cx);
        });
        assert_eq!(visual.debug_bounds(cards[5]).unwrap(), sixth);
        click(visual, Box::leak(project.clone().into_boxed_str()));
        assert!(visual.debug_bounds(viewport).is_none());
        assert!(visual.debug_bounds(header).is_some());
        assert!(cards.iter().all(|card| visual.debug_bounds(card).is_none()));
        click(visual, Box::leak(project.into_boxed_str()));
        assert!(visual.debug_bounds(viewport).is_some());
        let latest: &Session = sessions.last().unwrap();
        let list = visual.debug_bounds(viewport).unwrap();
        visual.simulate_event(ScrollWheelEvent {
            position: list.center(),
            delta: ScrollDelta::Pixels(point(px(0.), px(-300.))),
            ..Default::default()
        });
        visual.run_until_parked();
        visual.update(|window, cx| window.draw(cx).clear(cx));
        click(
            visual,
            Box::leak(format!("activity-{}", latest.id).into_boxed_str()),
        );
        wait(visual, |cx| {
            shell.read(cx).page == Page::Conversation
                && shell.read(cx).current_chat().is_some_and(|chat| {
                    chat.read(cx)
                        .summary()
                        .is_some_and(|session| session.id == latest.id)
                })
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
        fixture.execute(Command::SetPluginEnabled {
            name: "progress".into(),
            expected_revision: package.summary.revision,
            enabled: false,
        });
        wait(visual, |cx| {
            mounted.read(cx).mounted.is_none()
                && !shell
                    .read(cx)
                    .extension_entries(cx)
                    .iter()
                    .any(|entry| entry.package.name == "progress")
        });
        visual.update(|window, _| window.remove_window());
        drop(mounted);
        drop(panel);
        drop(shell);
        fixture.close();
    }
}
