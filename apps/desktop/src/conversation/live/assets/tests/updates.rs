use super::lifecycle::Observed;
use super::*;
use std::{collections::BTreeSet, sync::atomic::Ordering};

#[gpui::test]
fn follows_session_events(cx: &mut TestAppContext) {
    init(cx);
    for remote in [false, true] {
        let mut fixture = Fixture::with_server(remote, |runtime| {
            runtime.block_on(crate::agent_fixture::Server::markdown(
                "[Report](report.md)".into(),
            ))
        });
        let Output::Session(other) = fixture.execute(Command::CreateSession {
            project: fixture.session.project,
            worktree: Some(fixture.session.worktree),
            config: Some(fixture.session.config.clone()),
        }) else {
            panic!("session expected")
        };
        let first = submit(&fixture, "[Input](input-0.txt)".into());
        let observed = Arc::new(Observed::new(fixture.transport.clone()));
        observed.fail.store(false, Ordering::SeqCst);
        fixture.binding.client = Arc::new(Client::new(observed.clone()));
        let (view, visual) = open(cx, fixture.binding.clone(), fixture.session.clone());
        wait(visual, |cx| view.read(cx).connected());
        let panel = view.read_with(visual, |view, _| view.assets.clone());
        assert!(observed.queries.lock().unwrap().is_empty());
        tap(visual, "live-chat-input");
        visual.simulate_input("Keep my update draft");

        let second = submit(&fixture, "[Input](input-1.txt)".into());
        wait(visual, |cx| view.read(cx).rows.len() == 2);
        assert!(observed.queries.lock().unwrap().is_empty());
        tap(visual, "live-assets");
        wait(visual, |cx| {
            let panel = panel.read(cx);
            panel.open && !panel.loading && panel.groups.len() == 4
        });
        assert!(visual.debug_bounds("asset-refresh").is_none());
        assert_eq!(
            visual.debug_bounds("asset-activity").unwrap().top(),
            visual.debug_bounds("asset-panel").unwrap().top()
        );
        let initial = panel.read_with(visual, |panel, _| {
            (
                panel.groups.clone(),
                panel.loaded_revision,
                panel.loaded_head,
            )
        });
        let queries = observed.queries.lock().unwrap().len();
        let unrelated = submit_to(&fixture, other.id, "[Hidden](hidden.txt)".into());
        wait(visual, |_| true);
        assert_eq!(observed.queries.lock().unwrap().len(), queries);
        assert_eq!(
            panel.read_with(visual, |panel, _| {
                (
                    panel.groups.clone(),
                    panel.loaded_revision,
                    panel.loaded_head,
                )
            }),
            initial
        );

        let third = submit(&fixture, "[Input](input-2.txt)".into());
        wait(visual, |cx| {
            let panel = panel.read(cx);
            panel.open
                && !panel.loading
                && panel.groups.len() == 6
                && panel.loaded_revision
                    == view
                        .read(cx)
                        .history
                        .snapshot
                        .as_ref()
                        .map(|snapshot| snapshot.page.revision)
                && panel.loaded_head
                    == view.read(cx).history.snapshot.as_ref().map(|snapshot| {
                        snapshot
                            .page
                            .entries
                            .last()
                            .map_or(0, |entry| entry.sequence)
                    })
        });
        assert!(observed.queries.lock().unwrap().len() > queries);
        panel.read_with(visual, |panel, _| {
            assert!(!panel.failed);
            assert_eq!(panel.loaded_revision, initial.1);
            assert!(panel.loaded_head > initial.2);
            assert_eq!(
                panel
                    .groups
                    .iter()
                    .map(|group| group.turn)
                    .collect::<BTreeSet<_>>(),
                [first, second, third].into_iter().collect()
            );
            assert!(panel.groups.iter().all(|group| {
                group.turn != unrelated && group.worktree == fixture.session.worktree
            }));
            assert_eq!(
                panel
                    .groups
                    .iter()
                    .filter(|group| group.kind == Kind::Artifact)
                    .count(),
                3
            );
            assert_eq!(
                panel
                    .groups
                    .iter()
                    .filter(|group| group.kind == Kind::Resource)
                    .count(),
                3
            );
        });
        let settled = observed.queries.lock().unwrap().len();
        for _ in 0..3 {
            wait(visual, |_| true);
        }
        assert_eq!(observed.queries.lock().unwrap().len(), settled);
        visual.simulate_keystrokes("escape");
        wait(visual, |cx| !panel.read(cx).open);
        let fourth = submit(&fixture, "[Input](input-3.txt)".into());
        wait(visual, |cx| view.read(cx).rows.len() == 4);
        assert_eq!(observed.queries.lock().unwrap().len(), settled);
        tap(visual, "live-assets");
        wait(visual, |cx| {
            let panel = panel.read(cx);
            panel.open && !panel.loading && panel.groups.len() == 8
        });
        panel.read_with(visual, |panel, _| {
            assert_eq!(panel.loaded_revision, initial.1);
            assert!(panel.loaded_head > initial.2);
            assert_eq!(
                panel
                    .groups
                    .iter()
                    .map(|group| group.turn)
                    .collect::<BTreeSet<_>>(),
                [first, second, third, fourth].into_iter().collect()
            );
            assert!(panel.groups.iter().all(|group| group.turn != unrelated));
        });
        assert_eq!(
            view.read_with(visual, |view, cx| view.draft(cx).to_string()),
            "Keep my update draft"
        );
        assert_eq!(fixture.task_requests(), 5);
        visual.update(|window, _| window.remove_window());
        fixture.close();
    }
}

#[gpui::test]
fn rejects_stale_preview_return(cx: &mut TestAppContext) {
    init(cx);
    cx.update(|cx| cx.set_reduce_motion(true));
    for remote in [false, true] {
        let mut fixture = Fixture::with_server(remote, |runtime| {
            runtime.block_on(crate::agent_fixture::Server::markdown(
                "![Image](result.png)".into(),
            ))
        });
        image::DynamicImage::new_rgb8(2, 2)
            .save(fixture.directory.path().join("project/result.png"))
            .unwrap();
        submit(&fixture, "[Input](input-0.txt)".into());
        let observed = Arc::new(Observed::new(fixture.transport.clone()));
        observed.fail.store(false, Ordering::SeqCst);
        fixture.binding.client = Arc::new(Client::new(observed.clone()));
        let (view, visual) = open(cx, fixture.binding.clone(), fixture.session.clone());
        wait(visual, |cx| view.read(cx).connected());
        tap(visual, "live-chat-input");
        visual.simulate_input("Keep my preview draft");
        tap(visual, "live-assets");
        let panel = view.read_with(visual, |view, _| view.assets.clone());
        wait(visual, |cx| {
            let panel = panel.read(cx);
            panel.open && !panel.loading && panel.groups.len() == 2
        });
        let initial = panel.read_with(visual, |panel, _| panel.stamp.unwrap());
        let selector = panel.read_with(visual, |panel, _| {
            let group = panel
                .groups
                .iter()
                .find(|group| group.kind == Kind::Artifact)
                .unwrap();
            format!("asset-{}-0", group.sequence)
        });
        tap(visual, Box::leak(selector.into_boxed_str()));
        assert!(visual.debug_bounds("attachment-image-preview").is_some());
        assert!(!panel.read_with(visual, |panel, _| panel.open));
        let queries = observed.queries.lock().unwrap().len();

        submit(&fixture, "[Input](input-1.txt)".into());
        wait(visual, |cx| {
            let panel = panel.read(cx);
            panel
                .stamp
                .is_some_and(|(revision, head)| revision == initial.0 && head > initial.1)
                && view.read(cx).rows.len() == 2
        });
        assert_eq!(observed.queries.lock().unwrap().len(), queries);
        assert!(!panel.read_with(visual, |panel, _| panel.open));
        visual.simulate_keystrokes("escape");
        wait(visual, |_| true);
        assert!(!visual.update(|window, cx| window.has_active_dialog(cx)));
        assert!(!panel.read_with(visual, |panel, _| panel.open));
        assert!(visual.debug_bounds("asset-panel").is_none());

        tap(visual, "live-assets");
        wait(visual, |cx| {
            let panel = panel.read(cx);
            panel.open && !panel.loading && panel.groups.len() == 4
        });
        panel.read_with(visual, |panel, _| {
            assert_eq!(panel.loaded_revision, Some(initial.0));
            assert!(panel.loaded_head.is_some_and(|head| head > initial.1));
            assert!(!panel.failed);
        });
        assert!(observed.queries.lock().unwrap().len() > queries);
        assert_eq!(
            view.read_with(visual, |view, cx| view.draft(cx).to_string()),
            "Keep my preview draft"
        );
        assert_eq!(fixture.task_requests(), 2);
        visual.update(|window, _| window.remove_window());
        fixture.close();
    }
}
